//! Autonomous Agent for periodic system review and decision making.

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tokio::time::{interval, Instant};

use edge_ai_core::eventbus::EventBus;
use edge_ai_core::event::NeoTalkEvent;

use super::config::{AutonomousConfig, ReviewType};
use super::review::{ReviewContext, ReviewResult, SystemReview, DeviceHealthReview, TrendAnalysisReview, AnomalyDetectionReview, EnergyOptimizationReview};

/// State of the autonomous agent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AgentState {
    /// Agent is stopped
    Stopped,
    /// Agent is starting
    Starting,
    /// Agent is running
    Running,
    /// Agent is stopping
    Stopping,
    /// Agent has encountered an error
    Error(String),
}

impl AgentState {
    /// Check if the agent is running.
    pub fn is_running(&self) -> bool {
        matches!(self, Self::Running)
    }

    /// Check if the agent can start.
    pub fn can_start(&self) -> bool {
        matches!(self, Self::Stopped | Self::Error(_))
    }
}

/// Autonomous agent for periodic system review and LLM-driven decision making.
pub struct AutonomousAgent {
    /// Agent configuration
    config: AutonomousConfig,
    /// Current agent state
    state: Arc<RwLock<AgentState>>,
    /// Event bus for publishing events
    event_bus: Arc<EventBus>,
    /// Review implementations
    reviews: Vec<Box<dyn SystemReview>>,
}

impl AutonomousAgent {
    /// Create a new autonomous agent.
    pub fn new(config: AutonomousConfig, event_bus: Arc<EventBus>) -> Self {
        let event_bus_clone = event_bus.clone();

        Self {
            config,
            state: Arc::new(RwLock::new(AgentState::Stopped)),
            event_bus,
            reviews: vec![
                Box::new(DeviceHealthReview::new(event_bus_clone.clone())),
                Box::new(TrendAnalysisReview::new(event_bus_clone.clone())),
                Box::new(AnomalyDetectionReview::new(event_bus_clone.clone())),
                Box::new(EnergyOptimizationReview::new(event_bus_clone)),
            ],
        }
    }

    /// Get the current agent state.
    pub async fn state(&self) -> AgentState {
        self.state.read().await.clone()
    }

    /// Get the agent configuration.
    pub fn config(&self) -> &AutonomousConfig {
        &self.config
    }

    /// Update the agent configuration.
    pub async fn update_config(&mut self, config: AutonomousConfig) {
        self.config = config;
    }

    /// Start the autonomous agent.
    pub async fn start(&self) -> Result<(), AgentError> {
        let mut state = self.state.write().await;

        if !state.can_start() {
            return Err(AgentError::InvalidState(format!(
                "Cannot start agent in state: {:?}",
                state
            )));
        }

        if !self.config.enabled {
            return Err(AgentError::Disabled("Agent is disabled in config".to_string()));
        }

        *state = AgentState::Starting;
        drop(state);

        // Spawn the agent task
        let agent = self.clone_for_task();
        tokio::spawn(async move {
            agent.run().await;
        });

        Ok(())
    }

    /// Stop the autonomous agent.
    pub async fn stop(&self) -> Result<(), AgentError> {
        let mut state = self.state.write().await;

        if !state.is_running() {
            return Err(AgentError::InvalidState(format!(
                "Cannot stop agent in state: {:?}",
                state
            )));
        }

        *state = AgentState::Stopping;
        *state = AgentState::Stopped;

        Ok(())
    }

    /// Perform a single review immediately.
    pub async fn trigger_review(&self, review_type: ReviewType) -> Result<ReviewResult, AgentError> {
        if !self.state.read().await.is_running() {
            return Err(AgentError::InvalidState(
                "Agent must be running to trigger reviews".to_string(),
            ));
        }

        self.perform_review(review_type).await
    }

    /// Get review results for all review types.
    pub async fn get_all_review_results(&self) -> Vec<ReviewResult> {
        let mut results = Vec::new();

        for review_type in &self.config.review_types {
            match self.perform_review(*review_type).await {
                Ok(result) => results.push(result),
                Err(_) => continue,
            }
        }

        results
    }

    /// Internal run loop for the agent.
    async fn run(&self) {
        // Set state to running
        {
            let mut state = self.state.write().await;
            *state = AgentState::Running;
        }

        let mut timer = interval(self.config.interval_duration());
        timer.tick().await; // Skip first immediate tick

        loop {
            // Check if still running
            {
                let state = self.state.read().await;
                if !state.is_running() {
                    break;
                }
            }

            timer.tick().await;

            // Perform reviews
            for review_type in self.config.review_types.clone() {
                match self.perform_review(review_type).await {
                    Ok(result) => {
                        self.publish_review_result(review_type, result).await;
                    }
                    Err(e) => {
                        tracing::error!("Review {:?} failed: {}", review_type, e);
                        self.publish_review_error(review_type, e.to_string()).await;
                    }
                }
            }
        }
    }

    /// Perform a single review.
    async fn perform_review(&self, review_type: ReviewType) -> Result<ReviewResult, AgentError> {
        let start = Instant::now();

        // Publish review start event
        let review_id = uuid::Uuid::new_v4().to_string();
        self.publish_review_start(review_id, review_type).await;

        // Find the review implementation
        let review = self
            .reviews
            .iter()
            .find(|r| r.review_type() == review_type)
            .ok_or_else(|| AgentError::ReviewNotFound(review_type))?;

        // Create review context
        let mut context = ReviewContext::new(review_type);

        // TODO: Collect actual system data
        // For now, use empty context

        // Perform the review with timeout
        let review_result = tokio::time::timeout(
            self.config.timeout_duration(),
            review.review(&mut context),
        )
        .await
        .map_err(|_| AgentError::Timeout(format!("Review {:?} timed out", review_type)))?;

        context.complete();

        let duration = start.elapsed();

        tracing::info!(
            "Review {:?} completed in {:?}: {}",
            review_type,
            duration,
            review_result.summary()
        );

        Ok(review_result)
    }

    /// Publish review start event.
    async fn publish_review_start(&self, review_id: String, review_type: ReviewType) {
        let event = NeoTalkEvent::PeriodicReviewTriggered {
            review_id,
            review_type: review_type.as_str().to_string(),
            timestamp: chrono::Utc::now().timestamp(),
        };
        let _ = self.event_bus.publish_with_source(event, "autonomous_agent").await;
    }

    /// Publish review result event.
    async fn publish_review_result(&self, review_type: ReviewType, result: ReviewResult) {
        match result {
            ReviewResult::Findings {
                ref summary,
                ref issues,
                ref recommendations,
            } => {
                let event = NeoTalkEvent::LlmDecisionProposed {
                    decision_id: uuid::Uuid::new_v4().to_string(),
                    title: format!("{} Findings", review_type.display_name()),
                    description: summary.clone(),
                    reasoning: format!(
                        "Found {} issues and {} recommendations",
                        issues.len(),
                        recommendations.len()
                    ),
                    actions: Vec::new(), // TODO: Convert recommendations to actions
                    confidence: 80.0,
                    timestamp: chrono::Utc::now().timestamp(),
                };
                let _ = self.event_bus.publish_with_source(event, "autonomous_agent").await;
            }
            ReviewResult::NoFindings { .. } => {
                tracing::info!("Review {:?} found no issues", review_type);
            }
            ReviewResult::Failed { ref error } => {
                tracing::error!("Review {:?} failed: {}", review_type, error);
            }
        }
    }

    /// Publish review error event.
    async fn publish_review_error(&self, review_type: ReviewType, error: String) {
        tracing::error!("Review {:?} error: {}", review_type, error);
    }

    /// Clone the agent for running in a task.
    fn clone_for_task(&self) -> Self {
        // Create new review instances instead of cloning
        let event_bus_clone = self.event_bus.clone();
        let reviews: Vec<Box<dyn SystemReview>> = vec![
            Box::new(DeviceHealthReview::new(event_bus_clone.clone())),
            Box::new(TrendAnalysisReview::new(event_bus_clone.clone())),
            Box::new(AnomalyDetectionReview::new(event_bus_clone.clone())),
            Box::new(EnergyOptimizationReview::new(event_bus_clone)),
        ];

        Self {
            config: self.config.clone(),
            state: self.state.clone(),
            event_bus: self.event_bus.clone(),
            reviews,
        }
    }
}

/// Errors that can occur in the autonomous agent.
#[derive(Debug, Clone, thiserror::Error)]
pub enum AgentError {
    /// Invalid state for the requested operation
    #[error("Invalid state: {0}")]
    InvalidState(String),

    /// Agent is disabled
    #[error("Agent disabled: {0}")]
    Disabled(String),

    /// Review implementation not found
    #[error("Review not found: {0:?}")]
    ReviewNotFound(ReviewType),

    /// Review timed out
    #[error("Review timed out: {0}")]
    Timeout(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_agent_state_transitions() {
        let config = AutonomousConfig::default();
        let event_bus = Arc::new(EventBus::new());
        let agent = AutonomousAgent::new(config, event_bus);

        // Initial state should be Stopped
        let state = agent.state().await;
        assert_eq!(state, AgentState::Stopped);
        assert!(state.can_start());
    }

    #[tokio::test]
    async fn test_disabled_agent_cannot_start() {
        let config = AutonomousConfig::default();
        let event_bus = Arc::new(EventBus::new());
        let agent = AutonomousAgent::new(config, event_bus);

        // Agent is disabled by default
        let result = agent.start().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_enabled_agent_can_start() {
        let config = AutonomousConfig::default().with_enabled(true);
        let event_bus = Arc::new(EventBus::new());
        let agent = AutonomousAgent::new(config, event_bus);

        let result = agent.start().await;
        assert!(result.is_ok());
    }
}
