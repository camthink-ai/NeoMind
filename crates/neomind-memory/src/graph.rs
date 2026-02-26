//! Graph-based Memory with Entity Relationships
//!
//! This module provides a knowledge graph structure for tracking relationships
//! between memory items, entities, and concepts.
//!
//! ## Features
//!
//! - **Entity Graph**: Nodes representing entities (people, places, things)
//! - **Relationships**: Typed edges between entities
//! - **Memory Associations**: Link memories to entities
//! - **Graph Traversal**: Find related memories through graph paths
//! - **Centrality Measures**: Identify important entities
//!
//! ## Example
//!
//! ```rust,no_run
//! use neomind_memory::graph::{
//!     MemoryGraph, Entity, EntityType, RelationType,
//! };
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let graph = MemoryGraph::new();
//!
//! // Add entities
//! let user_id = graph.add_entity(Entity::new("user1", "Alice")
//!     .with_type(EntityType::Person)).await;
//! let device_id = graph.add_entity(Entity::new("device1", "Temperature Sensor")
//!     .with_type(EntityType::Device)).await;
//!
//! // Link entities with a relationship
//! graph.add_relationship(&user_id, &device_id, RelationType::Owns).await?;
//!
//! // Find related entities
//! let related = graph.find_related(&user_id, RelationType::Owns, 1).await;
//! # Ok(())
//! # }
//! ```

use crate::error::{MemoryError, Result};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

/// Unique identifier for an entity in the graph.
pub type EntityId = String;

/// Unique identifier for a relationship in the graph.
pub type RelationId = String;

/// Types of entities that can be stored in the graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EntityType {
    /// A person (user, expert, etc.)
    Person = 1,
    /// A device or sensor
    Device = 2,
    /// A location or place
    Location = 3,
    /// An event or action
    Event = 4,
    /// A concept or topic
    Concept = 5,
    /// An organization or group
    Organization = 6,
    /// A time period
    Time = 7,
    /// Other uncategorized entity
    Other = 99,
}

impl EntityType {
    /// Get the string representation of this entity type.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Person => "person",
            Self::Device => "device",
            Self::Location => "location",
            Self::Event => "event",
            Self::Concept => "concept",
            Self::Organization => "organization",
            Self::Time => "time",
            Self::Other => "other",
        }
    }

    /// Create an entity type from a string.
    pub fn from_string(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "person" => Some(Self::Person),
            "device" => Some(Self::Device),
            "location" => Some(Self::Location),
            "event" => Some(Self::Event),
            "concept" => Some(Self::Concept),
            "organization" => Some(Self::Organization),
            "time" => Some(Self::Time),
            "other" => Some(Self::Other),
            _ => None,
        }
    }
}

/// Types of relationships between entities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RelationType {
    /// Entity A owns entity B
    Owns = 1,
    /// Entity A is located at entity B
    LocatedAt = 2,
    /// Entity A is related to entity B (generic)
    RelatedTo = 3,
    /// Entity A is part of entity B
    PartOf = 4,
    /// Entity A caused entity B
    Caused = 5,
    /// Entity A precedes entity B (temporal)
    Precedes = 6,
    /// Entity A interacts with entity B
    InteractsWith = 7,
    /// Entity A is measured by entity B
    MeasuredBy = 8,
    /// Entity A controls entity B
    Controls = 9,
    /// Entity A depends on entity B
    DependsOn = 10,
    /// Custom relationship type (with string value)
    Custom = 99,
}

impl RelationType {
    /// Get the string representation of this relation type.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Owns => "owns",
            Self::LocatedAt => "located_at",
            Self::RelatedTo => "related_to",
            Self::PartOf => "part_of",
            Self::Caused => "caused",
            Self::Precedes => "precedes",
            Self::InteractsWith => "interacts_with",
            Self::MeasuredBy => "measured_by",
            Self::Controls => "controls",
            Self::DependsOn => "depends_on",
            Self::Custom => "custom",
        }
    }

    /// Create a relation type from a string.
    pub fn from_string(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "owns" => Some(Self::Owns),
            "located_at" | "locatedat" => Some(Self::LocatedAt),
            "related_to" | "relatedto" => Some(Self::RelatedTo),
            "part_of" | "partof" => Some(Self::PartOf),
            "caused" => Some(Self::Caused),
            "precedes" => Some(Self::Precedes),
            "interacts_with" | "interactswith" => Some(Self::InteractsWith),
            "measured_by" | "measureby" | "measuredby" => Some(Self::MeasuredBy),
            "controls" => Some(Self::Controls),
            "depends_on" | "dependson" => Some(Self::DependsOn),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }
}

/// An entity in the knowledge graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    /// Unique identifier
    pub id: EntityId,
    /// Human-readable name
    pub name: String,
    /// Type of the entity
    pub entity_type: EntityType,
    /// Optional description
    pub description: Option<String>,
    /// Associated memory IDs
    pub memory_ids: Vec<String>,
    /// Additional attributes
    pub attributes: HashMap<String, serde_json::Value>,
    /// Creation timestamp
    pub created_at: i64,
    /// Last modified timestamp
    pub modified_at: i64,
}

impl Entity {
    /// Create a new entity.
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        let now = chrono::Utc::now().timestamp();
        Self {
            id: id.into(),
            name: name.into(),
            entity_type: EntityType::Other,
            description: None,
            memory_ids: Vec::new(),
            attributes: HashMap::new(),
            created_at: now,
            modified_at: now,
        }
    }

    /// Set the entity type.
    pub fn with_type(mut self, entity_type: EntityType) -> Self {
        self.entity_type = entity_type;
        self
    }

    /// Set the description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Add an attribute.
    pub fn with_attribute(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.attributes.insert(key.into(), value);
        self
    }

    /// Associate a memory ID.
    pub fn with_memory(mut self, memory_id: impl Into<String>) -> Self {
        self.memory_ids.push(memory_id.into());
        self
    }

    /// Add a memory association.
    pub fn add_memory(&mut self, memory_id: impl Into<String>) {
        let id = memory_id.into();
        if !self.memory_ids.contains(&id) {
            self.memory_ids.push(id);
        }
        self.modified_at = chrono::Utc::now().timestamp();
    }

    /// Set an attribute value.
    pub fn set_attribute(&mut self, key: impl Into<String>, value: serde_json::Value) {
        self.attributes.insert(key.into(), value);
        self.modified_at = chrono::Utc::now().timestamp();
    }

    /// Get an attribute value.
    pub fn get_attribute(&self, key: &str) -> Option<&serde_json::Value> {
        self.attributes.get(key)
    }
}

/// A relationship between two entities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relationship {
    /// Unique identifier
    pub id: RelationId,
    /// Source entity ID
    pub from: EntityId,
    /// Target entity ID
    pub to: EntityId,
    /// Type of relationship
    pub relation_type: RelationType,
    /// Optional custom relation type string (when relation_type is Custom)
    pub custom_type: Option<String>,
    /// Weight/strength of the relationship (0.0 - 1.0)
    pub weight: f64,
    /// Optional description
    pub description: Option<String>,
    /// Creation timestamp
    pub created_at: i64,
}

impl Relationship {
    /// Create a new relationship.
    pub fn new(
        id: impl Into<String>,
        from: impl Into<String>,
        to: impl Into<String>,
        relation_type: RelationType,
    ) -> Self {
        Self {
            id: id.into(),
            from: from.into(),
            to: to.into(),
            relation_type,
            custom_type: None,
            weight: 0.5,
            description: None,
            created_at: chrono::Utc::now().timestamp(),
        }
    }

    /// Set the weight.
    pub fn with_weight(mut self, weight: f64) -> Self {
        self.weight = weight.clamp(0.0, 1.0);
        self
    }

    /// Set the description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set custom type string.
    pub fn with_custom_type(mut self, custom_type: impl Into<String>) -> Self {
        self.custom_type = Some(custom_type.into());
        self
    }
}

/// Path in the graph from one entity to another.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphPath {
    /// Sequence of entity IDs in the path
    pub entities: Vec<EntityId>,
    /// Relationships between entities
    pub relationships: Vec<RelationType>,
    /// Total path weight (inverse of sum of edge weights)
    pub total_weight: f64,
}

/// Result of a graph traversal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraversalResult {
    /// Starting entity ID
    pub from: EntityId,
    /// Paths found
    pub paths: Vec<GraphPath>,
    /// All unique entities reached
    pub reached: Vec<EntityId>,
}

/// Centrality metrics for an entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CentralityMetrics {
    /// Degree centrality (number of direct connections)
    pub degree: usize,
    /// Number of incoming connections
    pub in_degree: usize,
    /// Number of outgoing connections
    pub out_degree: usize,
    /// Weighted centrality score
    pub weighted_score: f64,
}

/// Configuration for the memory graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphConfig {
    /// Maximum depth for graph traversals
    pub max_traversal_depth: usize,
    /// Maximum number of paths to return
    pub max_paths: usize,
    /// Minimum weight threshold for considering relationships
    pub min_weight: f64,
}

impl Default for GraphConfig {
    fn default() -> Self {
        Self {
            max_traversal_depth: 5,
            max_paths: 100,
            min_weight: 0.1,
        }
    }
}

/// Memory graph for tracking entity relationships.
#[derive(Clone)]
pub struct MemoryGraph {
    /// All entities in the graph - using DashMap for concurrent access
    entities: DashMap<EntityId, Entity>,
    /// All relationships in the graph - using DashMap for concurrent access
    relationships: DashMap<RelationId, Relationship>,
    /// Adjacency list: entity_id -> [(related_id, relation_id)] - using DashMap
    adj_out: DashMap<EntityId, Vec<(EntityId, RelationId)>>,
    /// Reverse adjacency list: entity_id -> [(source_id, relation_id)] - using DashMap
    adj_in: DashMap<EntityId, Vec<(EntityId, RelationId)>>,
    /// Configuration
    config: GraphConfig,
}

impl MemoryGraph {
    /// Create a new memory graph.
    pub fn new() -> Self {
        Self::with_config(GraphConfig::default())
    }

    /// Create with custom configuration.
    pub fn with_config(config: GraphConfig) -> Self {
        Self {
            entities: DashMap::with_capacity(64),  // Pre-allocate for typical graphs
            relationships: DashMap::with_capacity(64),
            adj_out: DashMap::with_capacity(64),
            adj_in: DashMap::with_capacity(64),
            config,
        }
    }

    /// Add an entity to the graph.
    pub fn add_entity(&self, entity: Entity) -> EntityId {
        let id = entity.id.clone();
        self.entities.insert(id.clone(), entity);
        id
    }

    /// Create and add a new entity.
    pub fn create_entity(
        &self,
        id: impl Into<String>,
        name: impl Into<String>,
        entity_type: EntityType,
    ) -> EntityId {
        let entity = Entity::new(id, name).with_type(entity_type);
        self.add_entity(entity)
    }

    /// Get an entity by ID.
    pub fn get_entity(&self, id: &str) -> Option<Entity> {
        self.entities.get(id).map(|item| item.value().clone())
    }

    /// Check if an entity exists.
    pub fn has_entity(&self, id: &str) -> bool {
        self.entities.contains_key(id)
    }

    /// Remove an entity and all its relationships.
    pub fn remove_entity(&self, id: &str) -> bool {
        // First, remove all relationships involving this entity
        let outgoing: Vec<_> = self
            .adj_out
            .get(id)
            .map(|item| item.value().clone())
            .unwrap_or_default();
        let incoming: Vec<_> = self
            .adj_in
            .get(id)
            .map(|item| item.value().clone())
            .unwrap_or_default();

        for (_, rel_id) in &outgoing {
            self.remove_relationship(rel_id);
        }
        for (_, rel_id) in &incoming {
            self.remove_relationship(rel_id);
        }

        // Remove the entity
        let removed = self.entities.remove(id).is_some();

        // Clean up adjacency lists
        self.adj_out.remove(id);
        self.adj_in.remove(id);

        removed
    }

    /// Add a relationship between two entities.
    pub async fn add_relationship(
        &self,
        from: impl Into<String>,
        to: impl Into<String>,
        relation_type: RelationType,
    ) -> Result<RelationId> {
        let from_id = from.into();
        let to_id = to.into();

        // Check if entities exist - DashMap is lock-free
        if !self.entities.contains_key(&from_id) {
            return Err(MemoryError::NotFound(format!("Source entity: {}", from_id)));
        }
        if !self.entities.contains_key(&to_id) {
            return Err(MemoryError::NotFound(format!("Target entity: {}", to_id)));
        }

        let rel_id = format!("{}_{}_{:?}", from_id, to_id, relation_type);
        let relationship = Relationship::new(
            rel_id.clone(),
            from_id.clone(),
            to_id.clone(),
            relation_type,
        );

        // Add relationship - DashMap is lock-free
        self.relationships.insert(rel_id.clone(), relationship);

        // Update adjacency lists - DashMap is lock-free
        self.adj_out
            .entry(from_id.clone())
            .or_default()
            .push((to_id.clone(), rel_id.clone()));

        self.adj_in
            .entry(to_id)
            .or_default()
            .push((from_id, rel_id.clone()));

        Ok(rel_id)
    }

    /// Add a weighted relationship.
    pub async fn add_weighted_relationship(
        &self,
        from: impl Into<String>,
        to: impl Into<String>,
        relation_type: RelationType,
        weight: f64,
    ) -> Result<RelationId> {
        let from_id = from.into();
        let to_id = to.into();
        let rel_id = format!("{}_{}_{:?}", from_id, to_id, relation_type);
        let relationship = Relationship::new(
            rel_id.clone(),
            from_id.clone(),
            to_id.clone(),
            relation_type,
        )
        .with_weight(weight);

        // Add relationship - DashMap is lock-free
        self.relationships.insert(rel_id.clone(), relationship);

        // Update adjacency lists - DashMap is lock-free
        self.adj_out
            .entry(from_id.clone())
            .or_default()
            .push((to_id.clone(), rel_id.clone()));

        self.adj_in
            .entry(to_id)
            .or_default()
            .push((from_id, rel_id.clone()));

        Ok(rel_id)
    }

    /// Get a relationship by ID.
    pub fn get_relationship(&self, id: &str) -> Option<Relationship> {
        self.relationships.get(id).map(|item| item.value().clone())
    }

    /// Remove a relationship.
    pub fn remove_relationship(&self, id: &str) -> bool {
        let relationship = self.relationships.remove(id).map(|(_, v)| v);

        if let Some(rel) = relationship {
            // Update adjacency lists - DashMap is lock-free
            if let Some(mut neighbors) = self.adj_out.get_mut(&rel.from) {
                neighbors.retain(|(to_id, rel_id)| to_id != &rel.to || rel_id != id);
            }
            if let Some(mut sources) = self.adj_in.get_mut(&rel.to) {
                sources.retain(|(from_id, rel_id)| from_id != &rel.from || rel_id != id);
            }
            true
        } else {
            false
        }
    }

    /// Find entities directly related to the given entity.
    pub fn find_neighbors(&self, entity_id: &str) -> Vec<(EntityId, RelationType)> {
        let mut result = Vec::new();

        if let Some(neighbors) = self.adj_out.get(entity_id) {
            for (to_id, rel_id) in neighbors.iter() {
                if let Some(rel) = self.relationships.get(rel_id) {
                    result.push((to_id.clone(), rel.relation_type));
                }
            }
        }

        result
    }

    /// Find entities related through a specific relationship type.
    pub async fn find_related(
        &self,
        entity_id: &str,
        relation_type: RelationType,
        max_depth: usize,
    ) -> Vec<EntityId> {
        let mut visited = HashSet::new();
        let mut result = Vec::new();
        let mut queue = VecDeque::new();
        queue.push_back((entity_id.to_string(), 0));

        while let Some((current_id, depth)) = queue.pop_front() {
            if depth >= max_depth {
                continue;
            }

            // DashMap is lock-free
            if let Some(neighbors) = self.adj_out.get(&current_id) {
                for (to_id, rel_id) in neighbors.iter() {
                    if visited.contains(to_id) {
                        continue;
                    }

                    if let Some(rel) = self.relationships.get(rel_id) {
                        if rel.relation_type == relation_type {
                            visited.insert(to_id.clone());
                            result.push(to_id.clone());
                            queue.push_back((to_id.clone(), depth + 1));
                        }
                    }
                }
            }
        }

        result
    }

    /// Find all paths between two entities using BFS.
    pub async fn find_paths(
        &self,
        from: impl Into<String>,
        to: impl Into<String>,
        max_depth: usize,
    ) -> Vec<GraphPath> {
        let from_id = from.into();
        let to_id = to.into();
        let mut paths = Vec::new();

        // BFS to find all paths
        let mut queue: VecDeque<(EntityId, Vec<EntityId>, Vec<RelationType>, f64)> =
            VecDeque::new();
        queue.push_back((from_id.clone(), vec![from_id.clone()], Vec::new(), 0.0));

        while let Some((current, path, rels, weight)) = queue.pop_front() {
            if path.len() > max_depth + 1 {
                continue;
            }

            if current == to_id && !path.is_empty() {
                paths.push(GraphPath {
                    entities: path.clone(),
                    relationships: rels.clone(),
                    total_weight: weight,
                });
                continue;
            }

            // Get neighbors - now sync
            let neighbors = self.find_neighbors(&current);

            for (next_id, rel_type) in neighbors {
                // Avoid cycles
                if path.contains(&next_id) {
                    continue;
                }

                // Get relationship weight - DashMap is lock-free
                let edge_weight = self
                    .adj_out
                    .get(&current)
                    .and_then(|neighbors| {
                        neighbors
                            .iter()
                            .find(|(id, _)| id == &next_id)
                            .and_then(|(_, rel_id)| self.relationships.get(rel_id))
                            .map(|r| r.weight)
                    })
                    .unwrap_or(0.5);

                let mut new_path = path.clone();
                new_path.push(next_id.clone());
                let mut new_rels = rels.clone();
                new_rels.push(rel_type);

                queue.push_back((next_id, new_path, new_rels, weight + edge_weight));
            }

            if paths.len() >= self.config.max_paths {
                break;
            }
        }

        paths
    }

    /// Calculate centrality metrics for an entity.
    pub fn centrality(&self, entity_id: &str) -> Option<CentralityMetrics> {
        let out_degree = self.adj_out.get(entity_id).map(|v| v.len()).unwrap_or(0);
        let in_degree = self.adj_in.get(entity_id).map(|v| v.len()).unwrap_or(0);
        let total_degree = out_degree + in_degree;

        // Calculate weighted score - DashMap is lock-free
        let mut weighted_score = 0.0;
        if let Some(neighbors) = self.adj_out.get(entity_id) {
            for (_, rel_id) in neighbors.iter() {
                if let Some(rel) = self.relationships.get(rel_id) {
                    weighted_score += rel.weight;
                }
            }
        }
        if let Some(sources) = self.adj_in.get(entity_id) {
            for (_, rel_id) in sources.iter() {
                if let Some(rel) = self.relationships.get(rel_id) {
                    weighted_score += rel.weight;
                }
            }
        }

        Some(CentralityMetrics {
            degree: total_degree,
            out_degree,
            in_degree,
            weighted_score,
        })
    }

    /// Get all entities of a specific type.
    pub fn get_entities_by_type(&self, entity_type: EntityType) -> Vec<Entity> {
        self.entities
            .iter()
            .filter(|item| item.value().entity_type == entity_type)
            .map(|item| item.value().clone())
            .collect()
    }

    /// Get all entities associated with a memory ID.
    pub fn get_entities_for_memory(&self, memory_id: &str) -> Vec<Entity> {
        self.entities
            .iter()
            .filter(|item| item.value().memory_ids.contains(&memory_id.to_string()))
            .map(|item| item.value().clone())
            .collect()
    }

    /// Associate a memory with an entity.
    pub fn associate_memory(
        &self,
        entity_id: &str,
        memory_id: impl Into<String>,
    ) -> Result<()> {
        let mem_id = memory_id.into();
        
        // DashMap entry API is lock-free
        self.entities
            .entry(entity_id.to_string())
            .and_modify(|entity| {
                if !entity.memory_ids.contains(&mem_id) {
                    entity.memory_ids.push(mem_id.clone());
                }
            })
            .or_insert_with(|| {
                Entity::new(entity_id, entity_id).with_memory(mem_id.clone())
            });

        Ok(())
    }

    /// Get the total number of entities.
    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }

    /// Get the total number of relationships.
    pub fn relationship_count(&self) -> usize {
        self.relationships.len()
    }

    /// Clear all entities and relationships.
    pub fn clear(&self) {
        self.entities.clear();
        self.relationships.clear();
        self.adj_out.clear();
        self.adj_in.clear();
    }
}

impl Default for MemoryGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_entity_creation() {
        let entity = Entity::new("test", "Test Entity")
            .with_type(EntityType::Device)
            .with_description("A test device");

        assert_eq!(entity.id, "test");
        assert_eq!(entity.name, "Test Entity");
        assert_eq!(entity.entity_type, EntityType::Device);
        assert_eq!(entity.description, Some("A test device".to_string()));
    }

    #[tokio::test]
    async fn test_memory_graph_creation() {
        let graph = MemoryGraph::new();
        assert_eq!(graph.entity_count().await, 0);
        assert_eq!(graph.relationship_count().await, 0);
    }

    #[tokio::test]
    async fn test_add_and_get_entity() {
        let graph = MemoryGraph::new();
        let entity = Entity::new("device1", "Sensor").with_type(EntityType::Device);

        graph.add_entity(entity).await;
        assert!(graph.has_entity("device1").await);

        let retrieved = graph.get_entity("device1").await;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name, "Sensor");
    }

    #[tokio::test]
    async fn test_remove_entity() {
        let graph = MemoryGraph::new();
        graph
            .create_entity("test", "Test", EntityType::Device)
            .await;
        assert!(graph.has_entity("test").await);

        graph.remove_entity("test").await;
        assert!(!graph.has_entity("test").await);
    }

    #[tokio::test]
    async fn test_add_relationship() {
        let graph = MemoryGraph::new();
        graph
            .create_entity("user1", "Alice", EntityType::Person)
            .await;
        graph
            .create_entity("device1", "Sensor", EntityType::Device)
            .await;

        let rel_id = graph
            .add_relationship("user1", "device1", RelationType::Owns)
            .await;

        assert!(rel_id.is_ok());
        assert_eq!(graph.relationship_count().await, 1);
    }

    #[tokio::test]
    async fn test_find_neighbors() {
        let graph = MemoryGraph::new();
        graph.create_entity("a", "A", EntityType::Person).await;
        graph.create_entity("b", "B", EntityType::Device).await;
        graph.create_entity("c", "C", EntityType::Device).await;

        graph
            .add_relationship("a", "b", RelationType::Owns)
            .await
            .unwrap();
        graph
            .add_relationship("a", "c", RelationType::Controls)
            .await
            .unwrap();

        let neighbors = graph.find_neighbors("a").await;
        assert_eq!(neighbors.len(), 2);
    }

    #[tokio::test]
    async fn test_find_related() {
        let graph = MemoryGraph::new();
        graph
            .create_entity("user", "User", EntityType::Person)
            .await;
        graph
            .create_entity("device1", "Device 1", EntityType::Device)
            .await;
        graph
            .create_entity("device2", "Device 2", EntityType::Device)
            .await;
        graph
            .create_entity("sub1", "Sub 1", EntityType::Device)
            .await;

        graph
            .add_relationship("user", "device1", RelationType::Owns)
            .await
            .unwrap();
        graph
            .add_relationship("device1", "sub1", RelationType::PartOf)
            .await
            .unwrap();

        // Find entities connected through Owns relationship
        let related = graph.find_related("user", RelationType::Owns, 2).await;
        assert_eq!(related.len(), 1);
    }

    #[tokio::test]
    async fn test_centrality() {
        let graph = MemoryGraph::new();
        graph.create_entity("hub", "Hub", EntityType::Person).await;
        graph.create_entity("a", "A", EntityType::Device).await;
        graph.create_entity("b", "B", EntityType::Device).await;

        graph
            .add_relationship("hub", "a", RelationType::Controls)
            .await
            .unwrap();
        graph
            .add_relationship("hub", "b", RelationType::Controls)
            .await
            .unwrap();

        let metrics = graph.centrality("hub").await;
        assert!(metrics.is_some());
        let m = metrics.unwrap();
        assert_eq!(m.out_degree, 2);
        assert_eq!(m.in_degree, 0);
        assert_eq!(m.degree, 2);
    }

    #[tokio::test]
    async fn test_get_entities_by_type() {
        let graph = MemoryGraph::new();
        graph
            .create_entity("d1", "Device 1", EntityType::Device)
            .await;
        graph
            .create_entity("d2", "Device 2", EntityType::Device)
            .await;
        graph
            .create_entity("u1", "User 1", EntityType::Person)
            .await;

        let devices = graph.get_entities_by_type(EntityType::Device).await;
        assert_eq!(devices.len(), 2);

        let users = graph.get_entities_by_type(EntityType::Person).await;
        assert_eq!(users.len(), 1);
    }

    #[tokio::test]
    async fn test_associate_memory() {
        let graph = MemoryGraph::new();
        graph
            .create_entity("entity1", "Entity", EntityType::Device)
            .await;

        graph
            .associate_memory("entity1", "memory123")
            .await
            .unwrap();

        let entities = graph.get_entities_for_memory("memory123").await;
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].id, "entity1");
    }

    #[tokio::test]
    async fn test_entity_type_from_str() {
        assert_eq!(EntityType::from_string("person"), Some(EntityType::Person));
        assert_eq!(EntityType::from_string("device"), Some(EntityType::Device));
        assert_eq!(EntityType::from_string("unknown"), None);
    }

    #[tokio::test]
    async fn test_relation_type_from_str() {
        assert_eq!(RelationType::from_string("owns"), Some(RelationType::Owns));
        assert_eq!(
            RelationType::from_string("controls"),
            Some(RelationType::Controls)
        );
        assert_eq!(RelationType::from_string("unknown"), None);
    }

    #[tokio::test]
    async fn test_clear_graph() {
        let graph = MemoryGraph::new();
        graph.create_entity("e1", "E1", EntityType::Device).await;
        graph.create_entity("e2", "E2", EntityType::Device).await;
        graph
            .add_relationship("e1", "e2", RelationType::Controls)
            .await
            .unwrap();

        assert_eq!(graph.entity_count().await, 2);
        assert_eq!(graph.relationship_count().await, 1);

        graph.clear().await;

        assert_eq!(graph.entity_count().await, 0);
        assert_eq!(graph.relationship_count().await, 0);
    }
}
