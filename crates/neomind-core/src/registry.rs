//! Common registry interface.
//!
//! This module defines a unified interface that all registries should implement.
//! It provides common operations for managing collections of items.

use async_trait::async_trait;
use std::fmt::Debug;

/// Common operations for all registries.
///
/// This trait defines a minimal set of operations that all registries should support.
/// Individual registries may extend this with additional domain-specific methods.
///
/// # Example
///
/// ```rust
/// use neomind_core::registry::{Registry, RegistryError};
///
/// #[async_trait]
/// impl Registry for MyRegistry {
///     type Item = MyItem;
///     type Id = String;
///
///     async fn get(&self, id: &Self::Id) -> Option<Self::Item> {
///         self.items.get(id).cloned()
///     }
///
///     async fn list(&self) -> Vec<Self::Item> {
///         self.items.values().cloned().collect()
///     }
///
///     async fn count(&self) -> usize {
///         self.items.len()
///     }
///
///     async fn contains(&self, id: &Self::Id) -> bool {
///         self.items.contains_key(id)
///     }
///
///     async fn add(&mut self, item: Self::Item) -> Result<(), RegistryError> {
///         let id = item.id();
///         if self.items.contains_key(&id) {
///             return Err(RegistryError::AlreadyExists(id));
///         }
///         self.items.insert(id, item);
///         Ok(())
///     }
///
///     async fn remove(&mut self, id: &Self::Id) -> Result<Option<Self::Item>, RegistryError> {
///         Ok(self.items.remove(id))
///     }
///
///     async fn clear(&mut self) {
///         self.items.clear();
///     }
/// }
/// ```
#[async_trait]
pub trait Registry: Send + Sync {
    /// The item type stored in this registry.
    type Item: Clone + Send + Sync + Debug;

    /// The identifier type used to look up items.
    type Id: Clone + Send + Sync + Debug + PartialEq + Eq + std::hash::Hash;

    /// Get an item by ID.
    ///
    /// Returns `None` if the item doesn't exist.
    async fn get(&self, id: &Self::Id) -> Option<Self::Item>;

    /// List all items in the registry.
    async fn list(&self) -> Vec<Self::Item>;

    /// Get the count of items in the registry.
    async fn count(&self) -> usize;

    /// Check if an item exists in the registry.
    async fn contains(&self, id: &Self::Id) -> bool;

    /// Add an item to the registry.
    ///
    /// Returns an error if an item with the same ID already exists.
    async fn add(&mut self, item: Self::Item) -> Result<(), RegistryError>;

    /// Remove an item from the registry.
    ///
    /// Returns the removed item, or `None` if it didn't exist.
    async fn remove(&mut self, id: &Self::Id) -> Result<Option<Self::Item>, RegistryError>;

    /// Clear all items from the registry.
    async fn clear(&mut self);
}

/// Registry-specific errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryError {
    /// An item with the given ID already exists.
    AlreadyExists(String),

    /// An item with the given ID was not found.
    NotFound(String),

    /// The registry is in an invalid state for the operation.
    InvalidState(String),

    /// An operation was attempted that is not supported.
    NotSupported(String),

    /// A generic error message.
    Other(String),
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegistryError::AlreadyExists(id) => write!(f, "Item already exists: {}", id),
            RegistryError::NotFound(id) => write!(f, "Item not found: {}", id),
            RegistryError::InvalidState(msg) => write!(f, "Invalid state: {}", msg),
            RegistryError::NotSupported(msg) => write!(f, "Operation not supported: {}", msg),
            RegistryError::Other(msg) => write!(f, "Registry error: {}", msg),
        }
    }
}

impl std::error::Error for RegistryError {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;
    use std::collections::HashMap;

    #[derive(Debug, Clone, Serialize)]
    struct TestItem {
        id: String,
        name: String,
    }

    struct TestRegistry {
        items: HashMap<String, TestItem>,
    }

    #[async_trait]
    impl Registry for TestRegistry {
        type Item = TestItem;
        type Id = String;

        async fn get(&self, id: &Self::Id) -> Option<Self::Item> {
            self.items.get(id).cloned()
        }

        async fn list(&self) -> Vec<Self::Item> {
            self.items.values().cloned().collect()
        }

        async fn count(&self) -> usize {
            self.items.len()
        }

        async fn contains(&self, id: &Self::Id) -> bool {
            self.items.contains_key(id)
        }

        async fn add(&mut self, item: Self::Item) -> Result<(), RegistryError> {
            let id = item.id.clone();
            if self.items.contains_key(&id) {
                return Err(RegistryError::AlreadyExists(id));
            }
            self.items.insert(id, item);
            Ok(())
        }

        async fn remove(&mut self, id: &Self::Id) -> Result<Option<Self::Item>, RegistryError> {
            Ok(self.items.remove(id))
        }

        async fn clear(&mut self) {
            self.items.clear();
        }
    }

    #[tokio::test]
    async fn test_registry_add_and_get() {
        let mut registry = TestRegistry {
            items: HashMap::new(),
        };

        let item = TestItem {
            id: "test-1".to_string(),
            name: "Test Item 1".to_string(),
        };

        registry.add(item.clone()).await.unwrap();
        assert_eq!(registry.count().await, 1);
        assert!(registry.contains(&"test-1".to_string()).await);

        let retrieved = registry.get(&"test-1".to_string()).await;
        assert_eq!(retrieved.unwrap().id, "test-1");
    }

    #[tokio::test]
    async fn test_registry_duplicate() {
        let mut registry = TestRegistry {
            items: HashMap::new(),
        };

        let item = TestItem {
            id: "test-1".to_string(),
            name: "Test Item 1".to_string(),
        };

        registry.add(item).await.unwrap();

        let result = registry.add(item).await;
        assert!(matches!(result, Err(RegistryError::AlreadyExists(_))));
    }

    #[tokio::test]
    async fn test_registry_remove() {
        let mut registry = TestRegistry {
            items: HashMap::new(),
        };

        let item = TestItem {
            id: "test-1".to_string(),
            name: "Test Item 1".to_string(),
        };

        registry.add(item).await.unwrap();
        let removed = registry.remove(&"test-1".to_string()).await;
        assert!(removed.is_some());
        assert_eq!(registry.count().await, 0);
    }

    #[tokio::test]
    async fn test_registry_clear() {
        let mut registry = TestRegistry {
            items: HashMap::new(),
        };

        for i in 0..5 {
            registry.add(TestItem {
                id: format!("test-{}", i),
                name: format!("Test Item {}", i),
            })
            .await
            .unwrap();
        }

        registry.clear().await;
        assert_eq!(registry.count().await, 0);
    }
}
