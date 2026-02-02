//! Memory Importance Scoring System
//!
//! This module provides heat-based importance scoring for memory items.
//! Importance is calculated based on multiple factors:
//!
//! ## Scoring Factors
//!
//! - **Recency**: How recently the memory was accessed (exponential decay)
//! - **Frequency**: How often the memory is accessed (linear with boost)
//! - **Relevance**: Semantic similarity to recent queries (0.0 - 1.0)
//! - **Source**: The authority/trust level of the source
//! - **Emotional Impact**: User reactions and feedback
//! - **Cross-References**: How many other memories reference this one
//!
//! ## Heat Formula
//!
//! ```text
//! heat = (recency * RECENCY_WEIGHT) +
//!        (frequency * FREQUENCY_WEIGHT) +
//!        (relevance * RELEVANCE_WEIGHT) +
//!        (source * SOURCE_WEIGHT) +
//!        (emotional * EMOTIONAL_WEIGHT) +
//!        (cross_refs * CROSS_REF_WEIGHT)
//! ```
//!
//! ## Example
//!
//! ```rust,no_run
//! use edge_ai_memory::importance::{
//!     ImportanceScorer, MemoryItem, SourceType, ReactionType,
//! };
//!
//! let scorer = ImportanceScorer::new();
//!
//! let item = MemoryItem::new("item1", "Important conversation content")
//!     .with_source(SourceType::User)
//!     .with_reaction(ReactionType::Positive, 0.8);
//!
//! let score = scorer.calculate_heat(&item);
//! if score.heat > 0.7 {
//!     println!("High importance memory: {}", score);
//! }
//! ```

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Default weight for recency factor.
pub const DEFAULT_RECENCY_WEIGHT: f64 = 0.25;

/// Default weight for frequency factor.
pub const DEFAULT_FREQUENCY_WEIGHT: f64 = 0.20;

/// Default weight for relevance factor.
pub const DEFAULT_RELEVANCE_WEIGHT: f64 = 0.25;

/// Default weight for source factor.
pub const DEFAULT_SOURCE_WEIGHT: f64 = 0.10;

/// Default weight for emotional impact factor.
pub const DEFAULT_EMOTIONAL_WEIGHT: f64 = 0.10;

/// Default weight for cross-reference factor.
pub const DEFAULT_CROSS_REF_WEIGHT: f64 = 0.10;

/// Decay half-life for recency calculation (in seconds).
/// Default: 1 hour (3600 seconds)
pub const DEFAULT_DECAY_HALFLIFE: i64 = 3600;

/// Threshold for considering a memory "hot".
pub const HOT_THRESHOLD: f64 = 0.7;

/// Threshold for considering a memory "warm".
pub const WARM_THRESHOLD: f64 = 0.4;

/// Temperature category for memory importance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Temperature {
    /// Cold memory - low importance, can be compressed
    Cold = 0,
    /// Warm memory - medium importance
    Warm = 1,
    /// Hot memory - high importance, should be preserved
    Hot = 2,
    /// Critical memory - must not be lost
    Critical = 3,
}

impl Temperature {
    /// Get the temperature from a heat score.
    pub fn from_score(score: f64) -> Self {
        if score >= 0.85 {
            Self::Critical
        } else if score >= HOT_THRESHOLD {
            Self::Hot
        } else if score >= WARM_THRESHOLD {
            Self::Warm
        } else {
            Self::Cold
        }
    }

    /// Check if this temperature is at least as warm as another.
    pub fn is_at_least(&self, other: Temperature) -> bool {
        *self as u8 >= other as u8
    }
}

impl std::fmt::Display for Temperature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cold => write!(f, "Cold"),
            Self::Warm => write!(f, "Warm"),
            Self::Hot => write!(f, "Hot"),
            Self::Critical => write!(f, "Critical"),
        }
    }
}

/// Source type for a memory item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceType {
    /// System-generated content (lowest trust)
    System = 1,
    /// AI-generated content
    AI = 2,
    /// User input (high trust)
    User = 3,
    /// Expert/verified knowledge (highest trust)
    Expert = 4,
}

impl SourceType {
    /// Get the trust score for this source type (0.0 - 1.0).
    pub fn trust_score(&self) -> f64 {
        match self {
            Self::System => 0.3,
            Self::AI => 0.5,
            Self::User => 0.8,
            Self::Expert => 1.0,
        }
    }
}

/// User reaction type for memory items.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReactionType {
    /// Negative reaction (user rejected/corrected)
    Negative = 0,
    /// Neutral reaction (no feedback)
    Neutral = 1,
    /// Positive reaction (user liked/accepted)
    Positive = 2,
    /// Very positive reaction (user highlighted/bookmarked)
    VeryPositive = 3,
}

impl ReactionType {
    /// Get the impact score for this reaction type (0.0 - 1.0).
    pub fn impact_score(&self) -> f64 {
        match self {
            Self::Negative => 0.0,
            Self::Neutral => 0.5,
            Self::Positive => 0.8,
            Self::VeryPositive => 1.0,
        }
    }
}

/// Memory access record for tracking frequency and recency.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessRecord {
    /// Timestamp of access
    pub timestamp: i64,
    /// Access type (view, edit, query match, etc.)
    pub access_type: AccessType,
}

/// Type of memory access.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccessType {
    /// Memory was viewed
    View,
    /// Memory was edited/updated
    Edit,
    /// Memory matched a query
    QueryMatch,
    /// Memory was referenced by another
    Reference,
}

/// Memory item with importance tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryItem {
    /// Unique identifier
    pub id: String,
    /// Content of the memory
    pub content: String,
    /// When the memory was created
    pub created_at: i64,
    /// When the memory was last modified
    pub modified_at: i64,
    /// Access history
    pub accesses: Vec<AccessRecord>,
    /// Source type
    pub source: SourceType,
    /// User reactions
    pub reactions: Vec<(ReactionType, f64, i64)>, // (type, intensity, timestamp)
    /// Cross-reference count (how many other memories reference this)
    pub cross_references: usize,
    /// Manual importance boost (0.0 - 1.0)
    pub manual_boost: f64,
    /// Cached heat score
    #[serde(skip)]
    pub cached_heat: Option<f64>,
    /// Semantic embedding for relevance calculation
    #[serde(skip)]
    pub embedding: Option<Vec<f32>>,
}

impl MemoryItem {
    /// Create a new memory item.
    pub fn new(id: impl Into<String>, content: impl Into<String>) -> Self {
        let now = chrono::Utc::now().timestamp();
        Self {
            id: id.into(),
            content: content.into(),
            created_at: now,
            modified_at: now,
            accesses: Vec::new(),
            source: SourceType::AI,
            reactions: Vec::new(),
            cross_references: 0,
            manual_boost: 0.0,
            cached_heat: None,
            embedding: None,
        }
    }

    /// Set the source type.
    pub fn with_source(mut self, source: SourceType) -> Self {
        self.source = source;
        self
    }

    /// Add a user reaction.
    pub fn with_reaction(mut self, reaction: ReactionType, intensity: f64) -> Self {
        let now = chrono::Utc::now().timestamp();
        self.reactions.push((reaction, intensity, now));
        self
    }

    /// Set cross-reference count.
    pub fn with_cross_references(mut self, count: usize) -> Self {
        self.cross_references = count;
        self
    }

    /// Set manual importance boost.
    pub fn with_manual_boost(mut self, boost: f64) -> Self {
        self.manual_boost = boost.clamp(0.0, 1.0);
        self
    }

    /// Record an access.
    pub fn record_access(&mut self, access_type: AccessType) {
        let now = chrono::Utc::now().timestamp();
        self.accesses.push(AccessRecord {
            timestamp: now,
            access_type,
        });
        self.cached_heat = None; // Invalidate cache
        self.modified_at = now;
    }

    /// Add a reaction.
    pub fn add_reaction(&mut self, reaction: ReactionType, intensity: f64) {
        let now = chrono::Utc::now().timestamp();
        self.reactions.push((reaction, intensity.clamp(0.0, 1.0), now));
        self.cached_heat = None;
    }

    /// Increment cross-reference count.
    pub fn add_reference(&mut self) {
        self.cross_references += 1;
        self.cached_heat = None;
    }

    /// Get access count.
    pub fn access_count(&self) -> usize {
        self.accesses.len()
    }

    /// Get time since last access.
    pub fn time_since_last_access(&self) -> i64 {
        if let Some(last) = self.accesses.last() {
            chrono::Utc::now().timestamp() - last.timestamp
        } else {
            chrono::Utc::now().timestamp() - self.created_at
        }
    }
}

/// Configuration for importance scoring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportanceConfig {
    /// Weight for recency factor
    pub recency_weight: f64,
    /// Weight for frequency factor
    pub frequency_weight: f64,
    /// Weight for relevance factor
    pub relevance_weight: f64,
    /// Weight for source factor
    pub source_weight: f64,
    /// Weight for emotional impact factor
    pub emotional_weight: f64,
    /// Weight for cross-reference factor
    pub cross_ref_weight: f64,
    /// Decay half-life for recency (seconds)
    pub decay_halflife: i64,
    /// Minimum access count to apply frequency boost
    pub min_access_for_frequency: usize,
}

impl Default for ImportanceConfig {
    fn default() -> Self {
        Self {
            recency_weight: DEFAULT_RECENCY_WEIGHT,
            frequency_weight: DEFAULT_FREQUENCY_WEIGHT,
            relevance_weight: DEFAULT_RELEVANCE_WEIGHT,
            source_weight: DEFAULT_SOURCE_WEIGHT,
            emotional_weight: DEFAULT_EMOTIONAL_WEIGHT,
            cross_ref_weight: DEFAULT_CROSS_REF_WEIGHT,
            decay_halflife: DEFAULT_DECAY_HALFLIFE,
            min_access_for_frequency: 2,
        }
    }
}

impl ImportanceConfig {
    /// Create a new config with custom weights.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set recency weight.
    pub fn recency_weight(mut self, weight: f64) -> Self {
        self.recency_weight = weight.clamp(0.0, 1.0);
        self
    }

    /// Set frequency weight.
    pub fn frequency_weight(mut self, weight: f64) -> Self {
        self.frequency_weight = weight.clamp(0.0, 1.0);
        self
    }

    /// Set relevance weight.
    pub fn relevance_weight(mut self, weight: f64) -> Self {
        self.relevance_weight = weight.clamp(0.0, 1.0);
        self
    }

    /// Set decay half-life.
    pub fn decay_halflife(mut self, seconds: i64) -> Self {
        self.decay_halflife = seconds.max(1);
        self
    }
}

/// Heat score result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeatScore {
    /// Overall heat score (0.0 - 1.0)
    pub heat: f64,
    /// Temperature category
    pub temperature: Temperature,
    /// Individual factor scores
    pub factors: FactorScores,
}

/// Individual factor scores.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactorScores {
    /// Recency score (0.0 - 1.0)
    pub recency: f64,
    /// Frequency score (0.0 - 1.0)
    pub frequency: f64,
    /// Relevance score (0.0 - 1.0)
    pub relevance: f64,
    /// Source trust score (0.0 - 1.0)
    pub source: f64,
    /// Emotional impact score (0.0 - 1.0)
    pub emotional: f64,
    /// Cross-reference score (0.0 - 1.0)
    pub cross_ref: f64,
    /// Manual boost (0.0 - 1.0)
    pub manual_boost: f64,
}

impl std::fmt::Display for HeatScore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "HeatScore(heat={:.2}, temperature={}, recency={:.2}, frequency={:.2})",
            self.heat, self.temperature, self.factors.recency, self.factors.frequency
        )
    }
}

/// Importance scorer for memory items.
#[derive(Clone)]
pub struct ImportanceScorer {
    config: ImportanceConfig,
    /// Recent queries for relevance calculation
    recent_queries: Arc<RwLock<Vec<(String, i64)>>>,
}

impl ImportanceScorer {
    /// Create a new importance scorer.
    pub fn new() -> Self {
        Self {
            config: ImportanceConfig::default(),
            recent_queries: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Create with custom config.
    pub fn with_config(config: ImportanceConfig) -> Self {
        Self {
            config,
            recent_queries: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Get the current config.
    pub fn config(&self) -> &ImportanceConfig {
        &self.config
    }

    /// Update the config.
    pub fn set_config(&mut self, config: ImportanceConfig) {
        self.config = config;
    }

    /// Record a query for relevance tracking.
    pub async fn record_query(&self, query: impl Into<String>) {
        let now = chrono::Utc::now().timestamp();
        let mut queries = self.recent_queries.write().await;
        queries.push((query.into(), now));

        // Keep only recent queries (last 100)
        let len = queries.len();
        if len > 100 {
            queries.drain(0..len - 100);
        }
    }

    /// Calculate heat score for a memory item.
    pub fn calculate_heat(&self, item: &MemoryItem) -> HeatScore {
        // Return cached score if available
        if let Some(cached) = item.cached_heat {
            return HeatScore {
                heat: cached,
                temperature: Temperature::from_score(cached),
                factors: FactorScores {
                    recency: 0.0,
                    frequency: 0.0,
                    relevance: 0.0,
                    source: 0.0,
                    emotional: 0.0,
                    cross_ref: 0.0,
                    manual_boost: item.manual_boost,
                },
            };
        }

        let recency = self.calculate_recency(item);
        let frequency = self.calculate_frequency(item);
        let relevance = 0.5; // Default if no semantic search
        let source = item.source.trust_score();
        let emotional = self.calculate_emotional(item);
        let cross_ref = self.calculate_cross_ref(item);

        // Apply weights
        let mut heat = (recency * self.config.recency_weight)
            + (frequency * self.config.frequency_weight)
            + (relevance * self.config.relevance_weight)
            + (source * self.config.source_weight)
            + (emotional * self.config.emotional_weight)
            + (cross_ref * self.config.cross_ref_weight);

        // Add manual boost
        heat = (heat + item.manual_boost).clamp(0.0, 1.0);

        let factors = FactorScores {
            recency,
            frequency,
            relevance,
            source,
            emotional,
            cross_ref,
            manual_boost: item.manual_boost,
        };

        HeatScore {
            heat,
            temperature: Temperature::from_score(heat),
            factors,
        }
    }

    /// Calculate heat score with semantic relevance.
    pub async fn calculate_heat_with_relevance(
        &self,
        item: &MemoryItem,
        query_relevance: f64,
    ) -> HeatScore {
        let recency = self.calculate_recency(item);
        let frequency = self.calculate_frequency(item);
        let source = item.source.trust_score();
        let emotional = self.calculate_emotional(item);
        let cross_ref = self.calculate_cross_ref(item);

        // Apply weights with provided relevance
        let mut heat = (recency * self.config.recency_weight)
            + (frequency * self.config.frequency_weight)
            + (query_relevance * self.config.relevance_weight)
            + (source * self.config.source_weight)
            + (emotional * self.config.emotional_weight)
            + (cross_ref * self.config.cross_ref_weight);

        // Add manual boost
        heat = (heat + item.manual_boost).clamp(0.0, 1.0);

        let factors = FactorScores {
            recency,
            frequency,
            relevance: query_relevance,
            source,
            emotional,
            cross_ref,
            manual_boost: item.manual_boost,
        };

        HeatScore {
            heat,
            temperature: Temperature::from_score(heat),
            factors,
        }
    }

    /// Calculate recency score using exponential decay.
    fn calculate_recency(&self, item: &MemoryItem) -> f64 {
        let time_since = item.time_since_last_access();
        if time_since <= 0 {
            return 1.0;
        }

        // Exponential decay: e^(-ln(2) * t / half_life)
        let decay = (-std::f64::consts::LN_2 * time_since as f64 / self.config.decay_halflife as f64).exp();
        decay.clamp(0.0, 1.0)
    }

    /// Calculate frequency score based on access count.
    fn calculate_frequency(&self, item: &MemoryItem) -> f64 {
        let count = item.access_count();
        if count < self.config.min_access_for_frequency {
            return 0.0;
        }

        // Logarithmic scaling: log(count) / log(expected_max)
        // This gives diminishing returns for more accesses
        
        ((count as f64).ln() / 10.0_f64.ln()).clamp(0.0, 1.0)
    }

    /// Calculate emotional impact score.
    fn calculate_emotional(&self, item: &MemoryItem) -> f64 {
        if item.reactions.is_empty() {
            return 0.5; // Neutral default
        }

        // Average of all reactions, weighted by recency
        let now = chrono::Utc::now().timestamp();
        let mut total_weight = 0.0;
        let mut weighted_sum = 0.0;

        for (reaction, intensity, timestamp) in &item.reactions {
            let age = now - timestamp;
            let recency = (-age as f64 / 86400.0).exp(); // 1-day decay for reactions
            let weight = recency * intensity;
            weighted_sum += reaction.impact_score() * weight;
            total_weight += weight;
        }

        if total_weight > 0.0 {
            (weighted_sum / total_weight).clamp(0.0, 1.0)
        } else {
            0.5
        }
    }

    /// Calculate cross-reference score.
    fn calculate_cross_ref(&self, item: &MemoryItem) -> f64 {
        // Logarithmic scaling for cross-references
        // 0 refs = 0.0, 1 ref = 0.5, 10 refs = 1.0
        if item.cross_references == 0 {
            return 0.0;
        }

        
        ((item.cross_references as f64).ln() / 3.0_f64.ln()).clamp(0.0, 1.0)
    }

    /// Batch calculate heat scores.
    pub fn calculate_batch(&self, items: &[MemoryItem]) -> Vec<(String, HeatScore)> {
        items
            .iter()
            .map(|item| (item.id.clone(), self.calculate_heat(item)))
            .collect()
    }

    /// Sort items by heat score.
    pub fn sort_by_heat(&self, items: &mut [MemoryItem]) {
        items.sort_by(|a, b| {
            let score_a = self.calculate_heat(a);
            let score_b = self.calculate_heat(b);
            score_b
                .heat
                .partial_cmp(&score_a.heat)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    /// Get top N hottest items.
    pub fn get_hottest(&self, items: &[MemoryItem], n: usize) -> Vec<(String, HeatScore)> {
        let mut scored: Vec<_> = self.calculate_batch(items);
        scored.sort_by(|a, b| b.1.heat.partial_cmp(&a.1.heat).unwrap());
        scored.truncate(n);
        scored
    }

    /// Get coldest items (candidates for compression).
    pub fn get_coldest(&self, items: &[MemoryItem], n: usize) -> Vec<(String, HeatScore)> {
        let mut scored: Vec<_> = self.calculate_batch(items);
        scored.sort_by(|a, b| a.1.heat.partial_cmp(&b.1.heat).unwrap());
        scored.truncate(n);
        scored
    }
}

impl Default for ImportanceScorer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_temperature_from_score() {
        assert_eq!(Temperature::from_score(0.9), Temperature::Critical);
        assert_eq!(Temperature::from_score(0.75), Temperature::Hot);
        assert_eq!(Temperature::from_score(0.5), Temperature::Warm);
        assert_eq!(Temperature::from_score(0.2), Temperature::Cold);
    }

    #[test]
    fn test_temperature_display() {
        assert_eq!(format!("{}", Temperature::Hot), "Hot");
        assert_eq!(format!("{}", Temperature::Cold), "Cold");
    }

    #[test]
    fn test_temperature_is_at_least() {
        assert!(Temperature::Hot.is_at_least(Temperature::Warm));
        assert!(Temperature::Warm.is_at_least(Temperature::Cold));
        assert!(!Temperature::Cold.is_at_least(Temperature::Hot));
    }

    #[test]
    fn test_source_type_trust_score() {
        assert_eq!(SourceType::System.trust_score(), 0.3);
        assert_eq!(SourceType::AI.trust_score(), 0.5);
        assert_eq!(SourceType::User.trust_score(), 0.8);
        assert_eq!(SourceType::Expert.trust_score(), 1.0);
    }

    #[test]
    fn test_reaction_type_impact_score() {
        assert_eq!(ReactionType::Negative.impact_score(), 0.0);
        assert_eq!(ReactionType::Neutral.impact_score(), 0.5);
        assert_eq!(ReactionType::Positive.impact_score(), 0.8);
        assert_eq!(ReactionType::VeryPositive.impact_score(), 1.0);
    }

    #[test]
    fn test_memory_item_creation() {
        let item = MemoryItem::new("test", "content");
        assert_eq!(item.id, "test");
        assert_eq!(item.content, "content");
        assert_eq!(item.source, SourceType::AI);
        assert_eq!(item.access_count(), 0);
    }

    #[test]
    fn test_memory_item_with_source() {
        let item = MemoryItem::new("test", "content").with_source(SourceType::User);
        assert_eq!(item.source, SourceType::User);
    }

    #[test]
    fn test_memory_item_with_reaction() {
        let item = MemoryItem::new("test", "content")
            .with_reaction(ReactionType::Positive, 0.8);
        assert_eq!(item.reactions.len(), 1);
        assert_eq!(item.reactions[0].0, ReactionType::Positive);
    }

    #[test]
    fn test_memory_item_record_access() {
        let mut item = MemoryItem::new("test", "content");
        item.record_access(AccessType::View);
        assert_eq!(item.access_count(), 1);
        assert_eq!(item.accesses[0].access_type, AccessType::View);
    }

    #[test]
    fn test_memory_item_add_reference() {
        let mut item = MemoryItem::new("test", "content");
        item.add_reference();
        assert_eq!(item.cross_references, 1);
        item.add_reference();
        assert_eq!(item.cross_references, 2);
    }

    #[test]
    fn test_importance_config_default() {
        let config = ImportanceConfig::default();
        assert_eq!(config.recency_weight, DEFAULT_RECENCY_WEIGHT);
        assert_eq!(config.frequency_weight, DEFAULT_FREQUENCY_WEIGHT);
        assert_eq!(config.decay_halflife, DEFAULT_DECAY_HALFLIFE);
    }

    #[test]
    fn test_importance_config_builder() {
        let config = ImportanceConfig::new()
            .recency_weight(0.5)
            .frequency_weight(0.3)
            .decay_halflife(7200);

        assert_eq!(config.recency_weight, 0.5);
        assert_eq!(config.frequency_weight, 0.3);
        assert_eq!(config.decay_halflife, 7200);
    }

    #[test]
    fn test_importance_sorer_creation() {
        let scorer = ImportanceScorer::new();
        assert_eq!(scorer.config().recency_weight, DEFAULT_RECENCY_WEIGHT);
    }

    #[test]
    fn test_calculate_heat_basic() {
        let scorer = ImportanceScorer::new();
        let item = MemoryItem::new("test", "content");
        let score = scorer.calculate_heat(&item);

        assert!(score.heat >= 0.0 && score.heat <= 1.0);
        // Score should be valid - recency may be high for new items
        assert!(score.factors.recency >= 0.0 && score.factors.recency <= 1.0);
    }

    #[test]
    fn test_calculate_heat_with_accesses() {
        let scorer = ImportanceScorer::new();
        let mut item = MemoryItem::new("test", "content");

        // Record multiple accesses
        for _ in 0..5 {
            item.record_access(AccessType::View);
        }

        let score = scorer.calculate_heat(&item);
        assert!(score.factors.frequency > 0.0);
    }

    #[test]
    fn test_calculate_heat_with_source() {
        let scorer = ImportanceScorer::new();
        let user_item = MemoryItem::new("test1", "content").with_source(SourceType::User);
        let expert_item = MemoryItem::new("test2", "content").with_source(SourceType::Expert);

        let user_score = scorer.calculate_heat(&user_item);
        let expert_score = scorer.calculate_heat(&expert_item);

        assert_eq!(user_score.factors.source, 0.8);
        assert_eq!(expert_score.factors.source, 1.0);
    }

    #[test]
    fn test_calculate_heat_with_cross_references() {
        let scorer = ImportanceScorer::new();
        let item = MemoryItem::new("test", "content").with_cross_references(5);

        let score = scorer.calculate_heat(&item);
        assert!(score.factors.cross_ref > 0.0);
    }

    #[test]
    fn test_get_hottest() {
        let scorer = ImportanceScorer::new();
        let items = vec![
            MemoryItem::new("cold", "cold content"),
            MemoryItem::new("hot", "hot content")
                .with_source(SourceType::Expert)
                .with_cross_references(10),
        ];

        let hottest = scorer.get_hottest(&items, 1);
        assert_eq!(hottest[0].0, "hot");
    }

    #[test]
    fn test_get_coldest() {
        let scorer = ImportanceScorer::new();
        let items = vec![
            MemoryItem::new("cold", "cold content"),
            MemoryItem::new("hot", "hot content")
                .with_source(SourceType::Expert)
                .with_cross_references(10),
        ];

        let coldest = scorer.get_coldest(&items, 1);
        assert_eq!(coldest[0].0, "cold");
    }

    #[test]
    fn test_sort_by_heat() {
        let scorer = ImportanceScorer::new();
        let mut items = vec![
            MemoryItem::new("cold", "cold content"),
            MemoryItem::new("hot", "hot content")
                .with_source(SourceType::Expert)
                .with_cross_references(10),
        ];

        scorer.sort_by_heat(&mut items);
        assert_eq!(items[0].id, "hot");
        assert_eq!(items[1].id, "cold");
    }

    #[tokio::test]
    async fn test_record_query() {
        let scorer = ImportanceScorer::new();
        scorer.record_query("test query").await;

        let queries = scorer.recent_queries.read().await;
        assert_eq!(queries.len(), 1);
        assert_eq!(queries[0].0, "test query");
    }
}
