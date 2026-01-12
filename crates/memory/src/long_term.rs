//! Long-term memory for device knowledge and best practices.
//!
//! Long-term memory stores structured knowledge like device manuals,
//! troubleshooting guides, and best practices.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use super::error::{MemoryError, Result};

/// Default max knowledge entries
pub const DEFAULT_MAX_KNOWLEDGE: usize = 10000;

/// Category of knowledge.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum KnowledgeCategory {
    /// Device manual
    DeviceManual,
    /// Troubleshooting guide
    Troubleshooting,
    /// Best practice
    BestPractice,
    /// FAQ
    FAQ,
    /// Configuration example
    Configuration,
    /// API documentation
    ApiDoc,
    /// Custom category
    Custom(String),
}

impl KnowledgeCategory {
    /// Convert to string.
    pub fn as_str(&self) -> &str {
        match self {
            Self::DeviceManual => "device_manual",
            Self::Troubleshooting => "troubleshooting",
            Self::BestPractice => "best_practice",
            Self::FAQ => "faq",
            Self::Configuration => "configuration",
            Self::ApiDoc => "api_doc",
            Self::Custom(s) => s,
        }
    }

    /// Parse from string.
    pub fn from_str(s: &str) -> Self {
        match s {
            "device_manual" => Self::DeviceManual,
            "troubleshooting" => Self::Troubleshooting,
            "best_practice" => Self::BestPractice,
            "faq" => Self::FAQ,
            "configuration" => Self::Configuration,
            "api_doc" => Self::ApiDoc,
            other => Self::Custom(other.to_string()),
        }
    }
}

/// A knowledge entry in long-term memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeEntry {
    /// Unique ID
    pub id: String,
    /// Title
    pub title: String,
    /// Content
    pub content: String,
    /// Category
    pub category: KnowledgeCategory,
    /// Tags for indexing
    pub tags: Vec<String>,
    /// Related device IDs
    pub device_ids: Vec<String>,
    /// Timestamp created
    pub created_at: i64,
    /// Timestamp updated
    pub updated_at: i64,
    /// Access count
    pub access_count: u64,
    /// Additional metadata
    pub metadata: Option<serde_json::Value>,
}

impl KnowledgeEntry {
    /// Create a new knowledge entry.
    pub fn new(
        title: impl Into<String>,
        content: impl Into<String>,
        category: KnowledgeCategory,
    ) -> Self {
        let now = chrono::Utc::now().timestamp();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            title: title.into(),
            content: content.into(),
            category,
            tags: Vec::new(),
            device_ids: Vec::new(),
            created_at: now,
            updated_at: now,
            access_count: 0,
            metadata: None,
        }
    }

    /// Add tags.
    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    /// Add device IDs.
    pub fn with_devices(mut self, device_ids: Vec<String>) -> Self {
        self.device_ids = device_ids;
        self
    }

    /// Set metadata.
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Increment access count.
    pub fn increment_access(&mut self) {
        self.access_count += 1;
    }

    /// Update content.
    pub fn update_content(&mut self, content: impl Into<String>) {
        self.content = content.into();
        self.updated_at = chrono::Utc::now().timestamp();
    }

    /// Get a summary.
    pub fn summary(&self) -> String {
        let content_preview = if self.content.len() > 100 {
            format!("{}...", &self.content[..100])
        } else {
            self.content.clone()
        };
        format!("[{}] {}: {}", self.category.as_str(), self.title, content_preview)
    }

    /// Check if matches a search query.
    pub fn matches(&self, query: &str) -> bool {
        let query_lower = query.to_lowercase();
        self.title.to_lowercase().contains(&query_lower)
            || self.content.to_lowercase().contains(&query_lower)
            || self.tags.iter().any(|t| t.to_lowercase().contains(&query_lower))
    }
}

/// A troubleshooting case.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TroubleshootingCase {
    /// Unique ID
    pub id: String,
    /// Problem description
    pub problem: String,
    /// Symptoms
    pub symptoms: Vec<String>,
    /// Possible causes
    pub causes: Vec<String>,
    /// Solutions
    pub solutions: Vec<SolutionStep>,
    /// Related device types
    pub device_types: Vec<String>,
    /// Success rate (0-1)
    pub success_rate: f32,
}

impl TroubleshootingCase {
    /// Create a new troubleshooting case.
    pub fn new(problem: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            problem: problem.into(),
            symptoms: Vec::new(),
            causes: Vec::new(),
            solutions: Vec::new(),
            device_types: Vec::new(),
            success_rate: 0.0,
        }
    }

    /// Add a symptom.
    pub fn with_symptom(mut self, symptom: impl Into<String>) -> Self {
        self.symptoms.push(symptom.into());
        self
    }

    /// Add a cause.
    pub fn with_cause(mut self, cause: impl Into<String>) -> Self {
        self.causes.push(cause.into());
        self
    }

    /// Add a solution step.
    pub fn with_solution(mut self, step: SolutionStep) -> Self {
        self.solutions.push(step);
        self
    }

    /// Add device type.
    pub fn with_device_type(mut self, device_type: impl Into<String>) -> Self {
        self.device_types.push(device_type.into());
        self
    }

    /// Set success rate.
    pub fn with_success_rate(mut self, rate: f32) -> Self {
        self.success_rate = rate.clamp(0.0, 1.0);
        self
    }
}

/// A solution step in troubleshooting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolutionStep {
    /// Step number
    pub step_number: usize,
    /// Description
    pub description: String,
    /// Expected outcome
    pub expected_outcome: Option<String>,
    /// Commands to run (if applicable)
    pub commands: Vec<String>,
}

impl SolutionStep {
    /// Create a new solution step.
    pub fn new(step_number: usize, description: impl Into<String>) -> Self {
        Self {
            step_number,
            description: description.into(),
            expected_outcome: None,
            commands: Vec::new(),
        }
    }

    /// Add expected outcome.
    pub fn with_outcome(mut self, outcome: impl Into<String>) -> Self {
        self.expected_outcome = Some(outcome.into());
        self
    }

    /// Add a command.
    pub fn with_command(mut self, command: impl Into<String>) -> Self {
        self.commands.push(command.into());
        self
    }
}

/// Long-term memory for knowledge storage.
pub struct LongTermMemory {
    /// All knowledge entries
    knowledge: Arc<RwLock<HashMap<String, KnowledgeEntry>>>,
    /// Troubleshooting cases
    cases: Arc<RwLock<HashMap<String, TroubleshootingCase>>>,
    /// Maximum knowledge entries
    max_knowledge: usize,
    /// Index by category
    category_index: Arc<RwLock<HashMap<KnowledgeCategory, Vec<String>>>>,
    /// Index by tag
    tag_index: Arc<RwLock<HashMap<String, Vec<String>>>>,
    /// Index by device
    device_index: Arc<RwLock<HashMap<String, Vec<String>>>>,
}

impl LongTermMemory {
    /// Create a new long-term memory.
    pub fn new() -> Self {
        Self {
            knowledge: Arc::new(RwLock::new(HashMap::new())),
            cases: Arc::new(RwLock::new(HashMap::new())),
            max_knowledge: DEFAULT_MAX_KNOWLEDGE,
            category_index: Arc::new(RwLock::new(HashMap::new())),
            tag_index: Arc::new(RwLock::new(HashMap::new())),
            device_index: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Set max knowledge entries.
    pub fn with_max_knowledge(mut self, max: usize) -> Self {
        self.max_knowledge = max;
        self
    }

    /// Add a knowledge entry.
    pub async fn add(&self, entry: KnowledgeEntry) -> Result<()> {
        let id = entry.id.clone();
        let category = entry.category.clone();
        let tags = entry.tags.clone();
        let device_ids = entry.device_ids.clone();

        // Check capacity
        {
            let mut knowledge = self.knowledge.write().await;
            if knowledge.len() >= self.max_knowledge {
                return Err(MemoryError::CapacityExceeded(format!(
                    "Knowledge limit reached: {}",
                    self.max_knowledge
                )));
            }
            knowledge.insert(id.clone(), entry);
        }

        // Update indices
        {
            let mut cat_idx = self.category_index.write().await;
            cat_idx.entry(category).or_default().push(id.clone());
        }

        for tag in tags {
            let mut tag_idx = self.tag_index.write().await;
            tag_idx.entry(tag).or_default().push(id.clone());
        }

        for device_id in device_ids {
            let mut dev_idx = self.device_index.write().await;
            dev_idx.entry(device_id).or_default().push(id.clone());
        }

        Ok(())
    }

    /// Get a knowledge entry by ID.
    pub async fn get(&self, id: &str) -> Option<KnowledgeEntry> {
        let mut knowledge = self.knowledge.write().await;
        if let Some(entry) = knowledge.get_mut(id) {
            entry.increment_access();
            Some(entry.clone())
        } else {
            None
        }
    }

    /// Search knowledge by query.
    pub async fn search(&self, query: &str) -> Vec<KnowledgeEntry> {
        let knowledge = self.knowledge.read().await;
        let mut results: Vec<_> = knowledge
            .values()
            .filter(|e| e.matches(query))
            .cloned()
            .collect();

        // Sort by access count (most accessed first)
        results.sort_by(|a, b| b.access_count.cmp(&a.access_count));
        results
    }

    /// Get knowledge by category.
    pub async fn get_by_category(&self, category: &KnowledgeCategory) -> Vec<KnowledgeEntry> {
        let cat_idx = self.category_index.read().await;
        if let Some(ids) = cat_idx.get(category) {
            let knowledge = self.knowledge.read().await;
            ids.iter()
                .filter_map(|id| knowledge.get(id).cloned())
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Get knowledge by tag.
    pub async fn get_by_tag(&self, tag: &str) -> Vec<KnowledgeEntry> {
        let tag_idx = self.tag_index.read().await;
        if let Some(ids) = tag_idx.get(tag) {
            let knowledge = self.knowledge.read().await;
            ids.iter()
                .filter_map(|id| knowledge.get(id).cloned())
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Get knowledge by device.
    pub async fn get_by_device(&self, device_id: &str) -> Vec<KnowledgeEntry> {
        let dev_idx = self.device_index.read().await;
        if let Some(ids) = dev_idx.get(device_id) {
            let knowledge = self.knowledge.read().await;
            ids.iter()
                .filter_map(|id| knowledge.get(id).cloned())
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Add a troubleshooting case.
    pub async fn add_case(&self, case: TroubleshootingCase) -> Result<()> {
        let id = case.id.clone();
        self.cases.write().await.insert(id, case);
        Ok(())
    }

    /// Get a troubleshooting case by ID.
    pub async fn get_case(&self, id: &str) -> Option<TroubleshootingCase> {
        self.cases.read().await.get(id).cloned()
    }

    /// Find troubleshooting cases by symptoms.
    pub async fn find_cases(&self, symptoms: &[String]) -> Vec<TroubleshootingCase> {
        let cases = self.cases.read().await;
        cases
            .values()
            .filter(|case| {
                symptoms
                    .iter()
                    .any(|s| case.symptoms.iter().any(|cs| cs.to_lowercase().contains(&s.to_lowercase())))
            })
            .cloned()
            .collect()
    }

    /// Update a knowledge entry.
    pub async fn update(&self, id: &str, content: String) -> Result<()> {
        let mut knowledge = self.knowledge.write().await;
        if let Some(entry) = knowledge.get_mut(id) {
            entry.update_content(content);
            Ok(())
        } else {
            Err(MemoryError::NotFound(id.to_string()))
        }
    }

    /// Delete a knowledge entry.
    pub async fn delete(&self, id: &str) -> Result<()> {
        let mut knowledge = self.knowledge.write().await;
        if knowledge.remove(id).is_some() {
            // Remove from indices
            let mut cat_idx = self.category_index.write().await;
            for ids in cat_idx.values_mut() {
                ids.retain(|x| x != id);
            }

            let mut tag_idx = self.tag_index.write().await;
            for ids in tag_idx.values_mut() {
                ids.retain(|x| x != id);
            }

            let mut dev_idx = self.device_index.write().await;
            for ids in dev_idx.values_mut() {
                ids.retain(|x| x != id);
            }

            Ok(())
        } else {
            Err(MemoryError::NotFound(id.to_string()))
        }
    }

    /// Get the number of knowledge entries.
    pub async fn len(&self) -> usize {
        self.knowledge.read().await.len()
    }

    /// Check if empty.
    pub async fn is_empty(&self) -> bool {
        self.knowledge.read().await.is_empty()
    }

    /// Clear all knowledge.
    pub async fn clear(&self) {
        self.knowledge.write().await.clear();
        self.cases.write().await.clear();
        self.category_index.write().await.clear();
        self.tag_index.write().await.clear();
        self.device_index.write().await.clear();
    }

    /// Get all knowledge entries.
    pub async fn get_all(&self) -> Vec<KnowledgeEntry> {
        self.knowledge.read().await.values().cloned().collect()
    }

    /// Get most accessed entries.
    pub async fn get_most_accessed(&self, n: usize) -> Vec<KnowledgeEntry> {
        let knowledge = self.knowledge.read().await;
        let mut entries: Vec<_> = knowledge.values().cloned().collect();
        entries.sort_by(|a, b| b.access_count.cmp(&a.access_count));
        entries.truncate(n);
        entries
    }
}

impl Default for LongTermMemory {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_knowledge_entry() {
        let entry = KnowledgeEntry::new(
            "Test Entry",
            "Test content",
            KnowledgeCategory::BestPractice,
        );
        assert_eq!(entry.title, "Test Entry");
        assert_eq!(entry.category, KnowledgeCategory::BestPractice);
        assert_eq!(entry.access_count, 0);
    }

    #[test]
    fn test_knowledge_entry_with_tags() {
        let entry = KnowledgeEntry::new(
            "Test",
            "Content",
            KnowledgeCategory::DeviceManual,
        )
        .with_tags(vec!["sensor".to_string(), "temperature".to_string()]);

        assert_eq!(entry.tags.len(), 2);
        assert!(entry.tags.contains(&"sensor".to_string()));
    }

    #[test]
    fn test_knowledge_category() {
        assert_eq!(KnowledgeCategory::DeviceManual.as_str(), "device_manual");
        assert_eq!(
            KnowledgeCategory::from_str("device_manual"),
            KnowledgeCategory::DeviceManual
        );
        assert_eq!(
            KnowledgeCategory::from_str("custom_cat"),
            KnowledgeCategory::Custom("custom_cat".to_string())
        );
    }

    #[test]
    fn test_solution_step() {
        let step = SolutionStep::new(1, "Restart the device")
            .with_outcome("Device starts successfully")
            .with_command("systemctl restart device");

        assert_eq!(step.step_number, 1);
        assert_eq!(step.description, "Restart the device");
        assert!(step.expected_outcome.is_some());
        assert_eq!(step.commands.len(), 1);
    }

    #[test]
    fn test_troubleshooting_case() {
        let case = TroubleshootingCase::new("Device not responding")
            .with_symptom("LED not blinking")
            .with_cause("Power supply issue")
            .with_solution(SolutionStep::new(1, "Check power cable"))
            .with_device_type("sensor")
            .with_success_rate(0.85);

        assert_eq!(case.problem, "Device not responding");
        assert_eq!(case.symptoms.len(), 1);
        assert_eq!(case.causes.len(), 1);
        assert_eq!(case.solutions.len(), 1);
        assert!((case.success_rate - 0.85).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_long_term_memory_add() {
        let memory = LongTermMemory::new();

        let entry = KnowledgeEntry::new(
            "Test",
            "Content",
            KnowledgeCategory::BestPractice,
        );

        memory.add(entry).await.unwrap();
        assert_eq!(memory.len().await, 1);
    }

    #[tokio::test]
    async fn test_long_term_memory_get() {
        let memory = LongTermMemory::new();

        let entry = KnowledgeEntry::new(
            "Test Entry",
            "Content",
            KnowledgeCategory::BestPractice,
        );

        memory.add(entry.clone()).await.unwrap();

        let retrieved = memory.get(&entry.id).await.unwrap();
        assert_eq!(retrieved.title, "Test Entry");
        assert_eq!(retrieved.access_count, 1);
    }

    #[tokio::test]
    async fn test_search() {
        let memory = LongTermMemory::new();

        let entry1 = KnowledgeEntry::new(
            "Temperature Sensor Guide",
            "How to use temperature sensors",
            KnowledgeCategory::DeviceManual,
        )
        .with_tags(vec!["temperature".to_string(), "sensor".to_string()]);

        let entry2 = KnowledgeEntry::new(
            "Humidity Guide",
            "How to measure humidity",
            KnowledgeCategory::DeviceManual,
        )
        .with_tags(vec!["humidity".to_string()]);

        memory.add(entry1).await.unwrap();
        memory.add(entry2).await.unwrap();

        let results = memory.search("temperature").await;
        assert_eq!(results.len(), 1);
        assert!(results[0].title.contains("Temperature"));
    }

    #[tokio::test]
    async fn test_get_by_category() {
        let memory = LongTermMemory::new();

        memory
            .add(
                KnowledgeEntry::new("Manual1", "Content", KnowledgeCategory::DeviceManual)
                    .with_tags(vec!["tag1".to_string()]),
            )
            .await
            .unwrap();

        memory
            .add(
                KnowledgeEntry::new("FAQ1", "Content", KnowledgeCategory::FAQ)
                    .with_tags(vec!["tag2".to_string()]),
            )
            .await
            .unwrap();

        let manuals = memory.get_by_category(&KnowledgeCategory::DeviceManual).await;
        assert_eq!(manuals.len(), 1);

        let faqs = memory.get_by_category(&KnowledgeCategory::FAQ).await;
        assert_eq!(faqs.len(), 1);
    }

    #[tokio::test]
    async fn test_get_by_tag() {
        let memory = LongTermMemory::new();

        memory
            .add(
                KnowledgeEntry::new("Entry1", "Content", KnowledgeCategory::DeviceManual)
                    .with_tags(vec!["sensor".to_string()]),
            )
            .await
            .unwrap();

        memory
            .add(
                KnowledgeEntry::new("Entry2", "Content", KnowledgeCategory::BestPractice)
                    .with_tags(vec!["sensor".to_string(), "temperature".to_string()]),
            )
            .await
            .unwrap();

        let sensor_entries = memory.get_by_tag("sensor").await;
        assert_eq!(sensor_entries.len(), 2);
    }

    #[tokio::test]
    async fn test_get_by_device() {
        let memory = LongTermMemory::new();

        memory
            .add(
                KnowledgeEntry::new("Entry1", "Content", KnowledgeCategory::DeviceManual)
                    .with_devices(vec!["device1".to_string()]),
            )
            .await
            .unwrap();

        memory
            .add(
                KnowledgeEntry::new("Entry2", "Content", KnowledgeCategory::DeviceManual)
                    .with_devices(vec!["device2".to_string()]),
            )
            .await
            .unwrap();

        let device1_entries = memory.get_by_device("device1").await;
        assert_eq!(device1_entries.len(), 1);
    }

    #[tokio::test]
    async fn test_update() {
        let memory = LongTermMemory::new();

        let entry = KnowledgeEntry::new("Test", "Original", KnowledgeCategory::BestPractice);
        memory.add(entry.clone()).await.unwrap();

        memory.update(&entry.id, "Updated".to_string()).await.unwrap();

        let retrieved = memory.get(&entry.id).await.unwrap();
        assert_eq!(retrieved.content, "Updated");
    }

    #[tokio::test]
    async fn test_delete() {
        let memory = LongTermMemory::new();

        let entry = KnowledgeEntry::new("Test", "Content", KnowledgeCategory::BestPractice);
        memory.add(entry.clone()).await.unwrap();

        assert_eq!(memory.len().await, 1);

        memory.delete(&entry.id).await.unwrap();
        assert_eq!(memory.len().await, 0);
    }

    #[tokio::test]
    async fn test_troubleshooting_cases() {
        let memory = LongTermMemory::new();

        let case = TroubleshootingCase::new("Device not working")
            .with_symptom("No power")
            .with_symptom("No response")
            .with_solution(SolutionStep::new(1, "Check power"));

        memory.add_case(case.clone()).await.unwrap();

        let retrieved = memory.get_case(&case.id).await.unwrap();
        assert_eq!(retrieved.problem, "Device not working");
    }

    #[tokio::test]
    async fn test_find_cases() {
        let memory = LongTermMemory::new();

        let case1 = TroubleshootingCase::new("Case 1")
            .with_symptom("No power")
            .with_symptom("LED off");

        let case2 = TroubleshootingCase::new("Case 2")
            .with_symptom("High temperature")
            .with_symptom("Fan not spinning");

        memory.add_case(case1).await.unwrap();
        memory.add_case(case2).await.unwrap();

        let results = memory.find_cases(&["no power".to_string()]).await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].problem, "Case 1");
    }

    #[tokio::test]
    async fn test_most_accessed() {
        let memory = LongTermMemory::new();

        let entry1 = KnowledgeEntry::new("Entry1", "Content1", KnowledgeCategory::DeviceManual);
        let entry2 = KnowledgeEntry::new("Entry2", "Content2", KnowledgeCategory::BestPractice);

        let id1 = entry1.id.clone();
        memory.add(entry1).await.unwrap();
        memory.add(entry2).await.unwrap();

        // Access entry1 multiple times
        for _ in 0..5 {
            memory.get(&id1).await;
        }

        let most_accessed = memory.get_most_accessed(2).await;
        assert_eq!(most_accessed[0].id, id1);
        assert!(most_accessed[0].access_count >= 5);
    }

    #[tokio::test]
    async fn test_clear() {
        let memory = LongTermMemory::new();

        memory
            .add(KnowledgeEntry::new("Test", "Content", KnowledgeCategory::BestPractice))
            .await
            .unwrap();

        memory.clear().await;
        assert!(memory.is_empty().await);
    }
}
