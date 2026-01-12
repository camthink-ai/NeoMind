//! Multi-language compiler for compiling source code to WASM.
//!
//! This module provides a unified interface for compiling code written in
//! various languages (Rust, JavaScript, TypeScript, Wat) to WebAssembly.

use crate::error::{Result, WorkflowError};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Source programming languages that can be compiled to WASM.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceLanguage {
    /// Rust programming language
    Rust,
    /// JavaScript (via AssemblyScript)
    JavaScript,
    /// TypeScript (via AssemblyScript)
    TypeScript,
    /// WebAssembly Text Format
    Wat,
    /// Python (via Pyodide - limited support)
    Python,
}

impl SourceLanguage {
    /// Get the file extension for this language.
    pub fn extension(&self) -> &str {
        match self {
            SourceLanguage::Rust => "rs",
            SourceLanguage::JavaScript => "js",
            SourceLanguage::TypeScript => "ts",
            SourceLanguage::Wat => "wat",
            SourceLanguage::Python => "py",
        }
    }

    /// Parse from string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "rust" | "rs" => Some(SourceLanguage::Rust),
            "javascript" | "js" => Some(SourceLanguage::JavaScript),
            "typescript" | "ts" => Some(SourceLanguage::TypeScript),
            "wat" | "wast" => Some(SourceLanguage::Wat),
            "python" | "py" => Some(SourceLanguage::Python),
            _ => None,
        }
    }
}

/// Result of a compilation operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompilationResult {
    /// The compiled WASM bytes (as base64 for serialization)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wasm_bytes: Option<Vec<u8>>,
    /// Wasm bytes as base64 string for JSON serialization
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wasm_base64: Option<String>,
    /// Compilation warnings
    pub warnings: Vec<String>,
    /// Whether compilation was successful
    pub success: bool,
    /// Error message if compilation failed
    pub error: Option<String>,
    /// The source language
    pub language: SourceLanguage,
    /// Compilation time in milliseconds
    pub compilation_time_ms: u64,
}

/// Multi-language compiler for WASM.
pub struct MultiLanguageCompiler {
    /// Temporary directory for compilation
    temp_dir: PathBuf,
    /// Whether to use external compilers (wasm-pack, asc, etc.)
    use_external_compilers: bool,
}

impl MultiLanguageCompiler {
    /// Create a new compiler instance.
    pub fn new() -> Result<Self> {
        let temp_dir = std::env::temp_dir()
            .join("neotalk_compile")
            .join(uuid::Uuid::new_v4().to_string());

        std::fs::create_dir_all(&temp_dir)
            .map_err(|e| WorkflowError::CompilationError(format!("Failed to create temp dir: {}", e)))?;

        Ok(Self {
            temp_dir,
            use_external_compilers: true,
        })
    }

    /// Create a compiler that doesn't use external compilers.
    ///
    /// This mode only supports Wat compilation (using the `wat` crate).
    pub fn new_internal_only() -> Result<Self> {
        let temp_dir = std::env::temp_dir()
            .join("neotalk_compile")
            .join(uuid::Uuid::new_v4().to_string());

        std::fs::create_dir_all(&temp_dir)
            .map_err(|e| WorkflowError::CompilationError(format!("Failed to create temp dir: {}", e)))?;

        Ok(Self {
            temp_dir,
            use_external_compilers: false,
        })
    }

    /// Set whether to use external compilers.
    pub fn with_external_compilers(mut self, use_external: bool) -> Self {
        self.use_external_compilers = use_external;
        self
    }

    /// Compile source code to WASM.
    pub async fn compile(
        &self,
        source_code: &str,
        language: SourceLanguage,
    ) -> Result<CompilationResult> {
        let start = std::time::Instant::now();

        let result = match language {
            SourceLanguage::Wat => self.compile_wat(source_code).await,
            SourceLanguage::Rust if self.use_external_compilers => {
                self.compile_rust(source_code).await
            }
            SourceLanguage::JavaScript | SourceLanguage::TypeScript if self.use_external_compilers => {
                self.compile_assemblyscript(source_code, language).await
            }
            SourceLanguage::Python if self.use_external_compilers => {
                self.compile_python(source_code).await
            }
            _ => {
                // Language not supported without external compilers
                Ok(CompilationResult {
                    wasm_bytes: None,
                    wasm_base64: None,
                    warnings: vec![],
                    success: false,
                    error: Some(format!(
                        "Language {:?} requires external compilers (enable with use_external_compilers)",
                        language
                    )),
                    language,
                    compilation_time_ms: start.elapsed().as_millis() as u64,
                })
            }
        };

        // Add compilation time
        match result {
            Ok(mut r) => {
                r.compilation_time_ms = start.elapsed().as_millis() as u64;
                Ok(r)
            }
            Err(e) => Ok(CompilationResult {
                wasm_bytes: None,
                wasm_base64: None,
                warnings: vec![],
                success: false,
                error: Some(e.to_string()),
                language,
                compilation_time_ms: start.elapsed().as_millis() as u64,
            }),
        }
    }

    /// Compile Wat (WebAssembly Text Format) to WASM.
    async fn compile_wat(&self, source: &str) -> Result<CompilationResult> {
        #[cfg(feature = "wat")]
        {
            let mut warnings = vec![];

            // Parse and compile Wat using wat::parse_str
            let result = wat::parse_str(source)
                .map_err(|e| WorkflowError::CompilationError(format!("Wat compilation error: {}", e)));

            match result {
                Ok(wasm_bytes) => {
                    use base64::Engine;
                    let base64 = base64::engine::general_purpose::STANDARD.encode(&wasm_bytes);
                    Ok(CompilationResult {
                        wasm_bytes: Some(wasm_bytes.clone()),
                        wasm_base64: Some(base64),
                        warnings,
                        success: true,
                        error: None,
                        language: SourceLanguage::Wat,
                        compilation_time_ms: 0, // Will be set by caller
                    })
                }
                Err(e) => {
                    let error_msg = e.to_string();
                    Ok(CompilationResult {
                        wasm_bytes: None,
                        wasm_base64: None,
                        warnings,
                        success: false,
                        error: Some(error_msg),
                        language: SourceLanguage::Wat,
                        compilation_time_ms: 0,
                    })
                }
            }
        }

        #[cfg(not(feature = "wat"))]
        {
            Ok(CompilationResult {
                wasm_bytes: None,
                wasm_base64: None,
                warnings: vec!["Wat support requires the 'wat' feature".to_string()],
                success: false,
                error: Some("Wat support not enabled".to_string()),
                language: SourceLanguage::Wat,
                compilation_time_ms: 0,
            })
        }
    }

    /// Compile Rust code to WASM using wasm-pack.
    async fn compile_rust(&self, _source: &str) -> Result<CompilationResult> {
        // For now, return a placeholder - full Rust compilation requires:
        // 1. Creating a temporary Cargo project
        // 2. Writing the source code to src/lib.rs
        // 3. Setting up Cargo.toml with wasm-bindgen dependencies
        // 4. Running wasm-pack build
        // 5. Reading the generated .wasm file

        Ok(CompilationResult {
            wasm_bytes: None,
            wasm_base64: None,
            warnings: vec!["Rust compilation requires wasm-pack to be installed".to_string()],
            success: false,
            error: Some("Rust → WASM compilation not yet implemented. Please use pre-compiled WASM modules.".to_string()),
            language: SourceLanguage::Rust,
            compilation_time_ms: 0,
        })
    }

    /// Compile AssemblyScript (JavaScript/TypeScript) to WASM.
    async fn compile_assemblyscript(&self, _source: &str, language: SourceLanguage) -> Result<CompilationResult> {
        // For now, return a placeholder - full AssemblyScript compilation requires:
        // 1. Installing asc (AssemblyScript compiler)
        // 2. Writing source to .ts file
        // 3. Running asc to compile
        // 4. Reading the generated .wasm file

        Ok(CompilationResult {
            wasm_bytes: None,
            wasm_base64: None,
            warnings: vec![format!("{:?} compilation requires AssemblyScript compiler (asc)", language)],
            success: false,
            error: Some("AssemblyScript → WASM compilation not yet implemented. Please use pre-compiled WASM modules.".to_string()),
            language,
            compilation_time_ms: 0,
        })
    }

    /// Compile Python to WASM using Pyodide.
    async fn compile_python(&self, _source: &str) -> Result<CompilationResult> {
        // Python to WASM is complex - Pyodide provides a Python runtime in WASM,
        // but compiling arbitrary Python code to WASM is not straightforward.

        Ok(CompilationResult {
            wasm_bytes: None,
            wasm_base64: None,
            warnings: vec!["Python → WASM has limited support".to_string()],
            success: false,
            error: Some("Python → WASM compilation not yet implemented. Consider using the Python runtime in WASM via Pyodide instead.".to_string()),
            language: SourceLanguage::Python,
            compilation_time_ms: 0,
        })
    }

    /// Clean up temporary files.
    pub fn cleanup(&self) -> Result<()> {
        if self.temp_dir.exists() {
            std::fs::remove_dir_all(&self.temp_dir)
                .map_err(|e| WorkflowError::CompilationError(format!("Failed to cleanup: {}", e)))?;
        }
        Ok(())
    }
}

impl Default for MultiLanguageCompiler {
    fn default() -> Self {
        Self::new().unwrap_or_else(|_| {
            let temp_dir = std::env::temp_dir().join("neotalk_compile_fallback");
            std::fs::create_dir_all(&temp_dir).unwrap_or(());
            Self {
                temp_dir,
                use_external_compilers: false,
            }
        })
    }
}

impl Drop for MultiLanguageCompiler {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_source_language_extensions() {
        assert_eq!(SourceLanguage::Rust.extension(), "rs");
        assert_eq!(SourceLanguage::JavaScript.extension(), "js");
        assert_eq!(SourceLanguage::TypeScript.extension(), "ts");
        assert_eq!(SourceLanguage::Wat.extension(), "wat");
        assert_eq!(SourceLanguage::Python.extension(), "py");
    }

    #[test]
    fn test_source_language_from_str() {
        assert_eq!(SourceLanguage::from_str("rust"), Some(SourceLanguage::Rust));
        assert_eq!(SourceLanguage::from_str("js"), Some(SourceLanguage::JavaScript));
        assert_eq!(SourceLanguage::from_str("wat"), Some(SourceLanguage::Wat));
        assert_eq!(SourceLanguage::from_str("unknown"), None);
    }

    #[tokio::test]
    async fn test_compiler_creation() {
        let compiler = MultiLanguageCompiler::new();
        assert!(compiler.is_ok());
    }

    #[tokio::test]
    async fn test_compile_wat() {
        let compiler = MultiLanguageCompiler::new_internal_only().unwrap();

        // Simple Wat module
        let wat_code = r#"(module
  (func $add (param $a i32) (param $b i32) (result i32)
    local.get $a
    local.get $b
    i32.add)
  (export "add" (func $add))
)"#;

        let result = compiler.compile(wat_code, SourceLanguage::Wat).await.unwrap();

        #[cfg(feature = "wat")]
        {
            assert!(result.success, "Wat compilation should succeed");
            assert!(result.wasm_bytes.is_some(), "Should have wasm bytes");
            assert!(result.wasm_base64.is_some(), "Should have base64 encoded wasm");
        }

        #[cfg(not(feature = "wat"))]
        {
            assert!(!result.success, "Wat compilation should fail without feature");
        }
    }

    #[tokio::test]
    async fn test_compile_wat_invalid() {
        let compiler = MultiLanguageCompiler::new_internal_only().unwrap();

        let invalid_wat = "(module invalid syntax here";

        let result = compiler.compile(invalid_wat, SourceLanguage::Wat).await.unwrap();

        assert!(!result.success, "Invalid Wat should fail to compile");
        assert!(result.error.is_some(), "Should have error message");
    }

    #[tokio::test]
    async fn test_compile_unsupported_language_without_external() {
        let compiler = MultiLanguageCompiler::new_internal_only().unwrap();

        let rust_code = r#"fn main() { println!("Hello"); }"#;

        let result = compiler.compile(rust_code, SourceLanguage::Rust).await.unwrap();

        assert!(!result.success, "Should fail without external compilers");
        assert!(result.error.is_some(), "Should have error message");
    }
}
