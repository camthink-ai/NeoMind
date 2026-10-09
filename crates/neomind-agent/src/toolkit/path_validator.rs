//! Shared path validation logic for file_write and file_edit tools.

use std::path::{Path, PathBuf};

use super::error::{Result, ToolError};

/// File names that are always forbidden (security-sensitive).
const FORBIDDEN_NAMES: &[&str] = &[".env"];

/// Binary extensions that are forbidden.
const FORBIDDEN_BINARY_EXTENSIONS: &[&str] = &["so", "dll", "exe", "sys"];

/// Maximum content size for file_write (1 MB).
pub const MAX_CONTENT_SIZE: usize = 1024 * 1024;

/// Maximum file size for file_edit (10 MB).
pub const MAX_FILE_SIZE: usize = 10 * 1024 * 1024;

/// Path validation policy for file tools.
pub struct PathValidator {
    /// Primary data directory.
    data_dir: PathBuf,
    /// Additional allowed directories (from NEOMIND_ALLOWED_WRITE_DIRS).
    extra_dirs: Vec<PathBuf>,
}

impl PathValidator {
    /// Create a new validator with the given data directory.
    /// The primary data directory this validator was built with (tests use
    /// it to construct sibling tools over the same sandbox).
    pub fn data_dir(&self) -> &std::path::Path {
        &self.data_dir
    }

    pub fn new(data_dir: PathBuf) -> Self {
        let extra_dirs = Self::load_extra_dirs();
        Self {
            data_dir,
            extra_dirs,
        }
    }

    /// Load extra allowed directories from the `NEOMIND_ALLOWED_WRITE_DIRS` env var.
    /// Multiple directories separated by `:`.
    fn load_extra_dirs() -> Vec<PathBuf> {
        if let Ok(dirs) = std::env::var("NEOMIND_ALLOWED_WRITE_DIRS") {
            dirs.split(':')
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
                .filter(|p| p.exists())
                .collect()
        } else {
            Vec::new()
        }
    }

    /// All allowed directories (data_dir + extra_dirs).
    fn allowed_dirs(&self) -> Vec<&PathBuf> {
        let mut dirs: Vec<&PathBuf> = vec![&self.data_dir];
        dirs.extend(self.extra_dirs.iter());
        dirs
    }

    /// Resolve and validate a file path — must resolve under one of the allowed directories.
    ///
    /// Security checks:
    /// 1. No `..` path components (prevents traversal)
    /// 2. No binary file extensions (.so, .dll, .exe, .sys)
    /// 3. No forbidden filenames (.env, .env.*)
    /// 4. Path must be under an allowed directory (canonicalized or string prefix for new files)
    pub fn resolve_path(&self, path_str: &str) -> Result<PathBuf> {
        let trimmed = path_str.trim();
        if trimmed.is_empty() {
            return Err(ToolError::InvalidArguments("Path cannot be empty".into()));
        }
        let path = Path::new(trimmed);

        // Check for ".." as a path component (not just substring)
        for component in path.components() {
            if component == std::path::Component::ParentDir {
                return Err(ToolError::PermissionDenied(
                    "Path traversal (..) is not allowed".into(),
                ));
            }
        }

        // If relative, resolve against data_dir
        let resolved = if path.is_relative() {
            self.data_dir.join(path)
        } else {
            path.to_path_buf()
        };

        // Check forbidden binary extensions (case-insensitive)
        if let Some(ext) = resolved.extension().and_then(|e| e.to_str()) {
            let ext_lower = ext.to_lowercase();
            if FORBIDDEN_BINARY_EXTENSIONS.contains(&ext_lower.as_str()) {
                return Err(ToolError::PermissionDenied(format!(
                    "Writing binary .{} files is not allowed",
                    ext
                )));
            }
        }

        // Check forbidden names (.env and .env.* variants)
        if let Some(name) = resolved.file_name().and_then(|n| n.to_str()) {
            if FORBIDDEN_NAMES.contains(&name) || name.starts_with(".env.") {
                return Err(ToolError::PermissionDenied(format!(
                    "Writing '{}' is not allowed",
                    name
                )));
            }
        }

        // Verify path is under one of the allowed directories
        if !self.is_path_allowed(&resolved) {
            let allowed_str: Vec<String> = self
                .allowed_dirs()
                .into_iter()
                .map(|d| d.display().to_string())
                .collect();
            return Err(ToolError::PermissionDenied(format!(
                "Path '{}' is outside allowed directories ({}).",
                path_str,
                allowed_str.join(", ")
            )));
        }

        Ok(resolved)
    }

    /// Check if a resolved path is under any of the allowed directories.
    ///
    /// Uses canonicalization for existing paths. For non-existent paths,
    /// walks up to find the longest existing ancestor, canonicalizes it,
    /// then verifies the remaining non-existent suffix is safe.
    /// Fails closed (denies) if canonicalization fails.
    fn is_path_allowed(&self, resolved: &Path) -> bool {
        for dir in self.allowed_dirs() {
            // Case 1: File exists — canonicalize both and check prefix
            if resolved.exists() {
                if let (Ok(canon_resolved), Ok(canon_dir)) =
                    (resolved.canonicalize(), dir.canonicalize())
                {
                    if canon_resolved.starts_with(&canon_dir) {
                        return true;
                    }
                }
                continue;
            }

            // Case 2: File doesn't exist — walk up to find longest existing ancestor,
            // canonicalize it to resolve symlinks, then verify containment.
            if let Some(ancestor) = Self::find_existing_ancestor(resolved) {
                if let (Ok(canon_ancestor), Ok(canon_dir)) =
                    (ancestor.canonicalize(), dir.canonicalize())
                {
                    if canon_ancestor.starts_with(&canon_dir) {
                        return true;
                    }
                }
            }
            // If no existing ancestor found at all, deny (fail closed)
        }
        false
    }

    /// Walk up the path to find the longest existing ancestor.
    /// Returns None if no component of the path exists on disk.
    fn find_existing_ancestor(path: &Path) -> Option<PathBuf> {
        let mut current = path;
        loop {
            if current.exists() {
                return Some(current.to_path_buf());
            }
            current = current.parent()?;
        }
    }
}

/// Perform an atomic file write: write to temp file, then rename.
/// On POSIX, rename is atomic on the same filesystem.
/// Preserves existing file permissions on overwrite.
pub fn atomic_write(path: &Path, content: &str) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "path has no parent directory",
        )
    })?;

    // Generate temp file name
    let file_name = path
        .file_name()
        .ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no file name")
        })?
        .to_string_lossy();
    let tmp_name = format!(".{}.tmp", file_name);
    let tmp_path = parent.join(&tmp_name);

    // Write to temp file
    std::fs::write(&tmp_path, content)?;

    // Preserve original file permissions if overwriting
    if let Ok(meta) = std::fs::metadata(path) {
        let _ = std::fs::set_permissions(&tmp_path, meta.permissions());
    }

    // Atomic rename
    match std::fs::rename(&tmp_path, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            // Clean up temp file on rename failure
            let _ = std::fs::remove_file(&tmp_path);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn validator() -> PathValidator {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.keep();
        PathValidator::new(path)
    }

    #[test]
    fn test_rejects_parent_dir_component() {
        let v = validator();
        assert!(v.resolve_path("../../etc/passwd").is_err());
        assert!(v.resolve_path("foo/../bar").is_err());
    }

    #[test]
    fn test_allows_double_dot_in_filename() {
        let v = validator();
        // "file..backup.txt" contains ".." but not as a path component
        // Actually this should be rejected by the component check since
        // ".." inside a filename is a Normal component, not ParentDir
        let result = v.resolve_path("file..backup.txt");
        assert!(result.is_ok(), "Double-dot in filename should be allowed");
    }

    #[test]
    fn test_rejects_binary_extensions() {
        let v = validator();
        for ext in &["so", "dll", "exe", "sys"] {
            assert!(
                v.resolve_path(&format!("test.{}", ext)).is_err(),
                "Should reject .{}",
                ext
            );
        }
    }

    #[test]
    fn test_rejects_env_files() {
        let v = validator();
        assert!(v.resolve_path(".env").is_err());
        assert!(v.resolve_path(".env.local").is_err());
        assert!(v.resolve_path(".env.production").is_err());
    }

    #[test]
    fn test_accepts_rs_file() {
        let v = validator();
        assert!(v.resolve_path("src/lib.rs").is_ok());
    }

    #[test]
    fn test_accepts_toml_file() {
        let v = validator();
        assert!(v.resolve_path("Cargo.toml").is_ok());
    }

    #[test]
    fn test_atomic_write() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.keep().join("test.txt");
        atomic_write(&path, "hello world").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello world");
    }

    #[test]
    fn test_atomic_write_overwrite() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.keep().join("test.txt");
        std::fs::write(&path, "old content").unwrap();
        atomic_write(&path, "new content").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new content");
    }

    #[test]
    fn test_atomic_write_preserves_permissions() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.keep().join("secret.txt");

        // Create file with restrictive permissions
        std::fs::write(&path, "secret").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }

        // Overwrite via atomic_write
        atomic_write(&path, "new secret").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new secret");

        // Verify permissions preserved
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            // Mask off file type bits, keep permission bits (0o777)
            assert_eq!(
                mode & 0o777,
                0o600,
                "Permissions should be preserved as 0600"
            );
        }
    }

    #[test]
    fn test_symlink_escape_blocked() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data_dir = dir.path().join("data");
        std::fs::create_dir_all(&data_dir).unwrap();

        // Create a symlink: data/link -> /tmp (outside allowed dir)
        let link = data_dir.join("link");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/tmp", &link).unwrap();
        }

        let v = PathValidator::new(data_dir.clone());

        // Writing to data/link/escape.txt should be blocked
        // because canonicalizing the symlink resolves to /tmp which is outside data_dir
        let path = data_dir.join("link").join("escape.txt");
        if cfg!(unix) {
            // On Unix, the symlink exists, so canonicalize resolves it to /tmp
            // and /tmp/escape.txt does NOT start with /tmp/../data (the allowed dir)
            assert!(
                !v.is_path_allowed(&path),
                "Symlink escape should be blocked"
            );
        }
    }

    #[test]
    fn test_find_existing_ancestor() {
        let dir = tempfile::tempdir().expect("tempdir");
        let existing = dir.path().join("a");
        std::fs::create_dir_all(&existing).unwrap();

        let non_existent = existing.join("b").join("c").join("file.txt");
        let ancestor = PathValidator::find_existing_ancestor(&non_existent);
        assert_eq!(ancestor, Some(existing));
    }
}
