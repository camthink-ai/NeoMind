//! Multimodal data storage (images, audio, video, etc.).
//!
//! Provides storage for binary data like images along with metadata
//! and links to LLM analysis results.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::Error;

// Image metadata table: key = image_id, value = ImageMetadata (serialized)
const IMAGE_METADATA_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("image_metadata");

// Document metadata table: key = doc_id, value = DocumentMetadata (serialized)
const DOCUMENT_METADATA_TABLE: TableDefinition<&str, &[u8]> =
    TableDefinition::new("document_metadata");

/// Metadata for stored images.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageMetadata {
    /// Unique identifier.
    pub id: String,
    /// Path to the stored file.
    pub file_path: String,
    /// MIME type (e.g., "image/png", "image/jpeg").
    pub mime_type: String,
    /// File size in bytes.
    pub size: u64,
    /// Image width in pixels (if available).
    pub width: Option<u32>,
    /// Image height in pixels (if available).
    pub height: Option<u32>,
    /// Creation timestamp.
    pub created_at: i64,
    /// Associated vector embedding ID.
    pub embedding_id: Option<String>,
    /// LLM analysis/description of the image.
    pub llm_analysis: Option<String>,
    /// Additional metadata.
    pub metadata: Option<serde_json::Value>,
}

/// Metadata for stored documents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentMetadata {
    /// Unique identifier.
    pub id: String,
    /// Path to the stored file.
    pub file_path: String,
    /// MIME type (e.g., "application/pdf").
    pub mime_type: String,
    /// File size in bytes.
    pub size: u64,
    /// Document title.
    pub title: Option<String>,
    /// Creation timestamp.
    pub created_at: i64,
    /// Associated vector embedding ID.
    pub embedding_id: Option<String>,
    /// LLM summary of the document.
    pub llm_summary: Option<String>,
    /// Page count (for PDFs, etc.).
    pub page_count: Option<u32>,
    /// Additional metadata.
    pub metadata: Option<serde_json::Value>,
}

/// Multimodal data storage.
///
/// Stores binary data (images, documents) on the filesystem
/// with metadata in redb for fast lookup.
pub struct MultimodalStore {
    db: Arc<Database>,
    storage_path: PathBuf,
    /// Storage path for singleton
    db_path: String,
}

/// Global multimodal store singleton (thread-safe).
static MULTIMODAL_STORE_SINGLETON: StdMutex<Option<Arc<MultimodalStore>>> = StdMutex::new(None);

impl MultimodalStore {
    /// Open or create a multimodal store.
    ///
    /// `db_path` - Path to the redb database file.
    /// `storage_path` - Directory where binary files are stored.
    /// Uses a singleton pattern to prevent multiple opens of the same database.
    pub fn open<P: AsRef<Path>>(db_path: P, storage_path: P) -> Result<Arc<Self>, Error> {
        let db_path_str = db_path.as_ref().to_string_lossy().to_string();

        // Check if we already have a store for this path
        {
            let singleton = MULTIMODAL_STORE_SINGLETON.lock().unwrap();
            if let Some(store) = singleton.as_ref() {
                if store.db_path == db_path_str {
                    return Ok(store.clone());
                }
            }
        }

        // Create new store and save to singleton
        let db_path_ref = db_path.as_ref();
        let db = if db_path_ref.exists() {
            Database::open(db_path_ref)?
        } else {
            Database::create(db_path_ref)?
        };

        let storage_path = PathBuf::from(storage_path.as_ref());
        std::fs::create_dir_all(&storage_path)?;

        let store = Arc::new(MultimodalStore {
            db: Arc::new(db),
            storage_path,
            db_path: db_path_str,
        });

        *MULTIMODAL_STORE_SINGLETON.lock().unwrap() = Some(store.clone());
        Ok(store)
    }

    /// Store an image file.
    pub async fn store_image(
        &self,
        id: &str,
        image_data: &[u8],
        mime_type: &str,
    ) -> Result<ImageMetadata, Error> {
        // Save file to filesystem
        let file_name = format!("{}.bin", id);
        let file_path = self.storage_path.join(&file_name);
        fs::write(&file_path, image_data).await?;

        // Try to get image dimensions from common formats
        let (width, height) = self.read_image_dimensions(image_data, mime_type).await;

        // Create metadata
        let metadata = ImageMetadata {
            id: id.to_string(),
            file_path: file_path.to_string_lossy().to_string(),
            mime_type: mime_type.to_string(),
            size: image_data.len() as u64,
            width,
            height,
            created_at: chrono::Utc::now().timestamp(),
            embedding_id: None,
            llm_analysis: None,
            metadata: None,
        };

        // Save metadata to database
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(IMAGE_METADATA_TABLE)?;
            let serialized = bincode::serialize(&metadata)?;
            table.insert(id, serialized.as_slice())?;
        }
        write_txn.commit()?;

        Ok(metadata)
    }

    /// Link LLM analysis to an image.
    pub fn link_llm_analysis(
        &self,
        image_id: &str,
        analysis: &str,
        embedding_id: Option<&str>,
    ) -> Result<(), Error> {
        // First, read the existing metadata
        let existing_metadata: Option<ImageMetadata> = {
            let read_txn = self.db.begin_read()?;
            let table = read_txn.open_table(IMAGE_METADATA_TABLE)?;
            if let Some(data) = table.get(image_id)? {
                Some(bincode::deserialize(data.value())?)
            } else {
                None
            }
        };

        let metadata = existing_metadata
            .ok_or_else(|| Error::NotFound(format!("Image not found: {}", image_id)))?;

        // Now update with the new metadata
        let mut updated_metadata = metadata;
        updated_metadata.llm_analysis = Some(analysis.to_string());
        if let Some(eid) = embedding_id {
            updated_metadata.embedding_id = Some(eid.to_string());
        }

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(IMAGE_METADATA_TABLE)?;
            let serialized = bincode::serialize(&updated_metadata)?;
            table.insert(image_id, serialized.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Get image metadata.
    pub fn get_metadata(&self, image_id: &str) -> Result<Option<ImageMetadata>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(IMAGE_METADATA_TABLE)?;

        if let Some(data) = table.get(image_id)? {
            let metadata: ImageMetadata = bincode::deserialize(data.value())?;
            Ok(Some(metadata))
        } else {
            Ok(None)
        }
    }

    /// Load image file.
    pub async fn load_image(&self, image_id: &str) -> Result<Vec<u8>, Error> {
        let metadata = self
            .get_metadata(image_id)?
            .ok_or_else(|| Error::NotFound(format!("Image not found: {}", image_id)))?;

        let data = fs::read(&metadata.file_path).await?;
        Ok(data)
    }

    /// Store a document file.
    pub async fn store_document(
        &self,
        id: &str,
        document_data: &[u8],
        mime_type: &str,
    ) -> Result<DocumentMetadata, Error> {
        // Save file to filesystem
        let file_name = format!("{}.bin", id);
        let file_path = self.storage_path.join(&file_name);
        fs::write(&file_path, document_data).await?;

        // Create metadata
        let metadata = DocumentMetadata {
            id: id.to_string(),
            file_path: file_path.to_string_lossy().to_string(),
            mime_type: mime_type.to_string(),
            size: document_data.len() as u64,
            title: None,
            created_at: chrono::Utc::now().timestamp(),
            embedding_id: None,
            llm_summary: None,
            page_count: None,
            metadata: None,
        };

        // Save metadata to database
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(DOCUMENT_METADATA_TABLE)?;
            let serialized = bincode::serialize(&metadata)?;
            table.insert(id, serialized.as_slice())?;
        }
        write_txn.commit()?;

        Ok(metadata)
    }

    /// Link LLM summary to a document.
    pub fn link_llm_summary(
        &self,
        doc_id: &str,
        summary: &str,
        embedding_id: Option<&str>,
    ) -> Result<(), Error> {
        // First, read the existing metadata
        let existing_metadata: Option<DocumentMetadata> = {
            let read_txn = self.db.begin_read()?;
            let table = read_txn.open_table(DOCUMENT_METADATA_TABLE)?;
            if let Some(data) = table.get(doc_id)? {
                Some(bincode::deserialize(data.value())?)
            } else {
                None
            }
        };

        let metadata = existing_metadata
            .ok_or_else(|| Error::NotFound(format!("Document not found: {}", doc_id)))?;

        // Now update with the new metadata
        let mut updated_metadata = metadata;
        updated_metadata.llm_summary = Some(summary.to_string());
        if let Some(eid) = embedding_id {
            updated_metadata.embedding_id = Some(eid.to_string());
        }

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(DOCUMENT_METADATA_TABLE)?;
            let serialized = bincode::serialize(&updated_metadata)?;
            table.insert(doc_id, serialized.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Get document metadata.
    pub fn get_document_metadata(&self, doc_id: &str) -> Result<Option<DocumentMetadata>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(DOCUMENT_METADATA_TABLE)?;

        if let Some(data) = table.get(doc_id)? {
            let metadata: DocumentMetadata = bincode::deserialize(data.value())?;
            Ok(Some(metadata))
        } else {
            Ok(None)
        }
    }

    /// Load document file.
    pub async fn load_document(&self, doc_id: &str) -> Result<Vec<u8>, Error> {
        let metadata = self
            .get_document_metadata(doc_id)?
            .ok_or_else(|| Error::NotFound(format!("Document not found: {}", doc_id)))?;

        let data = fs::read(&metadata.file_path).await?;
        Ok(data)
    }

    /// Delete an image.
    pub fn delete_image(&self, image_id: &str) -> Result<bool, Error> {
        // First, get the file path if the image exists
        let file_path: Option<String> = {
            let read_txn = self.db.begin_read()?;
            let table = read_txn.open_table(IMAGE_METADATA_TABLE)?;
            if let Some(data) = table.get(image_id)? {
                let metadata: ImageMetadata = bincode::deserialize(data.value())?;
                Some(metadata.file_path)
            } else {
                None
            }
        };

        if file_path.is_none() {
            return Ok(false);
        }

        // Delete from database
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(IMAGE_METADATA_TABLE)?;
            table.remove(image_id)?;
        }
        write_txn.commit()?;

        // Delete the file
        if let Some(path) = file_path {
            let _ = std::fs::remove_file(path);
        }

        Ok(true)
    }

    /// Delete a document.
    pub fn delete_document(&self, doc_id: &str) -> Result<bool, Error> {
        // First, get the file path if the document exists
        let file_path: Option<String> = {
            let read_txn = self.db.begin_read()?;
            let table = read_txn.open_table(DOCUMENT_METADATA_TABLE)?;
            if let Some(data) = table.get(doc_id)? {
                let metadata: DocumentMetadata = bincode::deserialize(data.value())?;
                Some(metadata.file_path)
            } else {
                None
            }
        };

        if file_path.is_none() {
            return Ok(false);
        }

        // Delete from database
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(DOCUMENT_METADATA_TABLE)?;
            table.remove(doc_id)?;
        }
        write_txn.commit()?;

        // Delete the file
        if let Some(path) = file_path {
            let _ = std::fs::remove_file(path);
        }

        Ok(true)
    }

    /// List all image IDs.
    pub fn list_images(&self) -> Result<Vec<String>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(IMAGE_METADATA_TABLE)?;

        let mut ids = Vec::new();
        for result in table.iter()? {
            let key = result?.0;
            ids.push(key.value().to_string());
        }

        Ok(ids)
    }

    /// List all document IDs.
    pub fn list_documents(&self) -> Result<Vec<String>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(DOCUMENT_METADATA_TABLE)?;

        let mut ids = Vec::new();
        for result in table.iter()? {
            let key = result?.0;
            ids.push(key.value().to_string());
        }

        Ok(ids)
    }

    /// Read image dimensions from common formats.
    async fn read_image_dimensions(
        &self,
        data: &[u8],
        mime_type: &str,
    ) -> (Option<u32>, Option<u32>) {
        match mime_type {
            "image/png" => {
                if data.len() > 24 {
                    // PNG width/height are at bytes 16-23
                    let width = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
                    let height = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
                    (Some(width), Some(height))
                } else {
                    (None, None)
                }
            }
            "image/jpeg" => {
                // JPEG parsing is complex, return None for now
                (None, None)
            }
            _ => (None, None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_multimodal_store() {
        let temp_dir =
            std::env::temp_dir().join(format!("multimodal_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let store =
            MultimodalStore::open(temp_dir.join("db.redb"), temp_dir.join("files")).unwrap();

        // Store an image
        let image_data = vec![0u8; 100];
        let metadata = store
            .store_image("img1", &image_data, "image/png")
            .await
            .unwrap();

        assert_eq!(metadata.id, "img1");
        assert_eq!(metadata.size, 100);
        // PNG parsing extracts width/height from bytes 16-23 (test data has 0s)

        // Get metadata
        let retrieved = store.get_metadata("img1").unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().id, "img1");

        // Load image
        let loaded = store.load_image("img1").await.unwrap();
        assert_eq!(loaded.len(), 100);

        // List images
        let ids = store.list_images().unwrap();
        assert!(ids.contains(&"img1".to_string()));

        // Link analysis
        store
            .link_llm_analysis("img1", "A test image", Some("emb1"))
            .unwrap();

        let updated = store.get_metadata("img1").unwrap();
        assert_eq!(
            updated.unwrap().llm_analysis,
            Some("A test image".to_string())
        );
    }

    #[tokio::test]
    async fn test_document_storage() {
        let temp_dir = std::env::temp_dir().join(format!("doc_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let store =
            MultimodalStore::open(temp_dir.join("db.redb"), temp_dir.join("files")).unwrap();

        // Store a document
        let doc_data = b"Test document content".to_vec();
        let metadata = store
            .store_document("doc1", &doc_data, "text/plain")
            .await
            .unwrap();

        assert_eq!(metadata.id, "doc1");
        assert_eq!(metadata.mime_type, "text/plain");

        // Get metadata
        let retrieved = store.get_document_metadata("doc1").unwrap();
        assert!(retrieved.is_some());

        // Load document
        let loaded = store.load_document("doc1").await.unwrap();
        assert_eq!(loaded, b"Test document content");

        // Link summary
        store
            .link_llm_summary("doc1", "Test summary", Some("emb1"))
            .unwrap();

        let updated = store.get_document_metadata("doc1").unwrap();
        assert_eq!(
            updated.unwrap().llm_summary,
            Some("Test summary".to_string())
        );
    }
}
