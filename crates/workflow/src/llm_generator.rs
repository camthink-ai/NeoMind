//! LLM-based WASM code generator.
//!
//! This module provides functionality for generating WASM code from natural language
//! descriptions using Large Language Models.

use crate::compiler::{CompilationResult, SourceLanguage};
use crate::error::{Result, WorkflowError};
use serde::{Deserialize, Serialize};

/// Result of LLM code generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedWasmCode {
    /// The generated source code
    pub source_code: String,
    /// Compiled WASM bytes (if compilation succeeded)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wasm_bytes: Option<Vec<u8>>,
    /// Wasm bytes as base64 string
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wasm_base64: Option<String>,
    /// The language used for code generation
    pub language: SourceLanguage,
    /// Explanation of the generated code
    pub explanation: String,
    /// Any warnings from code generation or compilation
    pub warnings: Vec<String>,
    /// Whether compilation was successful
    pub compilation_success: bool,
    /// Compilation error message (if failed)
    pub compilation_error: Option<String>,
}

/// Configuration for the LLM code generator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratorConfig {
    /// Default language for code generation
    pub default_language: SourceLanguage,
    /// Maximum code generation attempts
    pub max_attempts: u32,
    /// Whether to validate generated code
    pub validate_code: bool,
    /// Timeout for LLM requests (in seconds)
    pub timeout_seconds: u64,
}

impl Default for GeneratorConfig {
    fn default() -> Self {
        Self {
            default_language: SourceLanguage::Wat,
            max_attempts: 3,
            validate_code: true,
            timeout_seconds: 30,
        }
    }
}

/// LLM-based WASM code generator.
///
/// This struct uses LLM to generate code from natural language descriptions
/// and compiles it to WASM.
pub struct WasmCodeGenerator {
    config: GeneratorConfig,
    /// Compiler for converting source code to WASM
    compiler: crate::compiler::MultiLanguageCompiler,
}

impl WasmCodeGenerator {
    /// Create a new code generator with default configuration.
    pub fn new() -> Result<Self> {
        Ok(Self {
            config: GeneratorConfig::default(),
            compiler: crate::compiler::MultiLanguageCompiler::new_internal_only()?,
        })
    }

    /// Create a new code generator with custom configuration.
    pub fn with_config(config: GeneratorConfig) -> Result<Self> {
        Ok(Self {
            config,
            compiler: crate::compiler::MultiLanguageCompiler::new_internal_only()?,
        })
    }

    /// Generate WASM code from a natural language description.
    ///
    /// This method creates a prompt for the LLM, sends it, and compiles the
    /// generated code to WASM.
    pub async fn generate_from_description(
        &self,
        description: &str,
        preferred_language: Option<SourceLanguage>,
    ) -> Result<GeneratedWasmCode> {
        let language = preferred_language.unwrap_or(self.config.default_language);
        let prompt = self.build_prompt(description, &language);

        // In a real implementation, this would call an LLM API
        // For now, we return a placeholder that indicates the integration point
        tracing::info!("LLM prompt: {}", prompt);

        // Simulate LLM response with a simple template
        let source_code = self.generate_template_code(description, &language);

        // Try to compile the generated code
        let compilation_result = self.compiler.compile(&source_code, language).await?;

        let warnings = compilation_result.warnings.clone();

        Ok(GeneratedWasmCode {
            source_code,
            wasm_bytes: compilation_result.wasm_bytes,
            wasm_base64: compilation_result.wasm_base64,
            language,
            explanation: format!("Generated {} code based on: {}", format!("{:?}", language).to_lowercase(), description),
            warnings,
            compilation_success: compilation_result.success,
            compilation_error: compilation_result.error,
        })
    }

    /// Generate code from a pre-existing source code string.
    ///
    /// This bypasses the LLM and directly compiles the provided code.
    pub async fn generate_from_source(
        &self,
        source_code: &str,
        language: SourceLanguage,
        description: Option<String>,
    ) -> Result<GeneratedWasmCode> {
        let compilation_result = self.compiler.compile(source_code, language).await?;

        Ok(GeneratedWasmCode {
            source_code: source_code.to_string(),
            wasm_bytes: compilation_result.wasm_bytes,
            wasm_base64: compilation_result.wasm_base64,
            language,
            explanation: description.unwrap_or_else(|| "Compiled from provided source code".to_string()),
            warnings: compilation_result.warnings,
            compilation_success: compilation_result.success,
            compilation_error: compilation_result.error,
        })
    }

    /// Build a prompt for the LLM based on the description and target language.
    fn build_prompt(&self, description: &str, language: &SourceLanguage) -> String {
        match language {
            SourceLanguage::Wat => self.build_wat_prompt(description),
            SourceLanguage::Rust => self.build_rust_prompt(description),
            SourceLanguage::JavaScript => self.build_javascript_prompt(description),
            SourceLanguage::TypeScript => self.build_typescript_prompt(description),
            SourceLanguage::Python => self.build_python_prompt(description),
        }
    }

    /// Build a Wat prompt.
    fn build_wat_prompt(&self, description: &str) -> String {
        format!(
            r#"You are a WebAssembly expert. Generate WebAssembly Text Format (Wat) code based on the user's request.

Available Host APIs (for NeoTalk platform):
- neotalk.log(message: string) - Log a message
- neotalk.get_metric(device_id: string, metric: string) -> f64 - Get a device metric value
- neotalk.send_command(device_id: string, command: string, params: object) - Send a command to a device
- neotalk.send_alert(severity: string, title: string, message: string) - Send an alert

Example Wat code:
```wat
(module
  (import "neotalk" "log" (func $log (param i32 i32)))
  (memory (export "memory") 1)
  (func (export "main")
    ;; Your code here
  )
)
```

User request: {}

Please generate complete Wat code that fulfills the user's request. The code should be valid WebAssembly Text Format."#,
            description
        )
    }

    /// Build a Rust prompt.
    fn build_rust_prompt(&self, description: &str) -> String {
        format!(
            r#"You are a Rust and WebAssembly expert. Generate Rust code that will be compiled to WASM.

Available Host APIs (via wasm-bindgen):
- neotalk::log(message: &str)
- neotalk::get_metric(device_id: &str, metric: &str) -> f64
- neotalk::send_command(device_id: &str, command: &str, params: &JsValue)
- neotalk::send_alert(severity: &str, title: &str, message: &str)

Example Rust code:
```rust
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn main() {{
    let temp = neotalk::get_metric("sensor_1", "temperature");
    neotalk::log(&format!("Temperature: {{}}", temp));
}}
```

User request: {}

Please generate complete Rust code with necessary imports and #[wasm_bindgen] attributes."#,
            description
        )
    }

    /// Build a JavaScript prompt.
    fn build_javascript_prompt(&self, description: &str) -> String {
        format!(
            r#"You are a JavaScript and WebAssembly expert. Generate AssemblyScript code that will be compiled to WASM.

Available Host APIs:
- neotalk.log(message: string): void
- neotalk.getMetric(deviceId: string, metric: string): f64
- neotalk.sendCommand(deviceId: string, command: string, params: object): void
- neotalk.sendAlert(severity: string, title: string, message: string): void

Example AssemblyScript code:
```assemblyscript
export function main(): void {{
    let temp = neotalk.getMetric("sensor_1", "temperature");
    neotalk.log("Temperature: " + temp.toString());
}}
```

User request: {}

Please generate complete AssemblyScript code."#,
            description
        )
    }

    /// Build a TypeScript prompt.
    fn build_typescript_prompt(&self, description: &str) -> String {
        // TypeScript uses AssemblyScript for WASM compilation
        self.build_javascript_prompt(description)
    }

    /// Build a Python prompt.
    fn build_python_prompt(&self, description: &str) -> String {
        format!(
            r#"You are a Python expert. Generate Python code conceptually (note: Python → WASM has limited support).

User request: {}

Please generate Python code. Note that full Python to WASM compilation is experimental; this is primarily for documentation and future integration purposes."#,
            description
        )
    }

    /// Generate template code based on description (placeholder for LLM).
    fn generate_template_code(&self, description: &str, language: &SourceLanguage) -> String {
        match language {
            SourceLanguage::Wat => {
                // Generate a simple Wat module template
                format!(
                    r#"(module
  ;; Auto-generated from: {}
  (import "neotalk" "log" (func $log (param i32 i32)))
  (memory (export "memory") 1)
  (func (export "main")
    ;; TODO: Implement logic for: {}
  )
)
"#,
                    description, description
                )
            }
            SourceLanguage::Rust => {
                format!(
                    r#"// Auto-generated from: {}
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn main() {{
    // TODO: Implement logic for: {}
}}
"#,
                    description, description
                )
            }
            SourceLanguage::JavaScript | SourceLanguage::TypeScript => {
                format!(
                    r#"// Auto-generated from: {}
// {}
export function main(): void {{
    // TODO: Implement logic for: {}
}}
"#,
                    description,
                    if *language == SourceLanguage::TypeScript {
                        "AssemblyScript"
                    } else {
                        "AssemblyScript (JavaScript)"
                    },
                    description
                )
            }
            SourceLanguage::Python => {
                format!(
                    r#"# Auto-generated from: {}
# TODO: Implement logic for: {}
def main():
    pass
"#,
                    description, description
                )
            }
        }
    }

    /// Extract code from LLM response.
    ///
    /// This method looks for code blocks in the LLM response and extracts the code.
    pub fn extract_code(&self, response: &str, language: &SourceLanguage) -> Result<String> {
        let marker = match language {
            SourceLanguage::Wat => "wat",
            SourceLanguage::Rust => "rust",
            SourceLanguage::JavaScript => "javascript",
            SourceLanguage::TypeScript => "typescript",
            SourceLanguage::Python => "python",
        };

        // Look for code blocks like ```wat ... ```
        if let Some(start) = response.find(&format!("```{}", marker)) {
            let code_start = start + marker.len() + 3;
            if let Some(end) = response[code_start..].find("```") {
                return Ok(response[code_start..code_start + end].trim().to_string());
            }
        }

        // Look for generic code blocks
        if let Some(start) = response.find("```") {
            let code_start = start + 3;
            if let Some(end) = response[code_start..].find("```") {
                return Ok(response[code_start..code_start + end].trim().to_string());
            }
        }

        // If no code blocks found, return the whole response
        Ok(response.trim().to_string())
    }
}

impl Default for WasmCodeGenerator {
    fn default() -> Self {
        Self::new().unwrap_or_else(|_| {
            let config = GeneratorConfig::default();
            Self {
                config,
                compiler: crate::compiler::MultiLanguageCompiler::default(),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_generator_creation() {
        let generator = WasmCodeGenerator::new();
        assert!(generator.is_ok());
    }

    #[tokio::test]
    async fn test_generate_from_description_wat() {
        let generator = WasmCodeGenerator::new().unwrap();

        let result = generator
            .generate_from_description("Create a simple add function", Some(SourceLanguage::Wat))
            .await
            .unwrap();

        assert_eq!(result.language, SourceLanguage::Wat);
        assert!(!result.source_code.is_empty());
        assert!(!result.explanation.is_empty());
    }

    #[tokio::test]
    async fn test_generate_from_source_wat() {
        let generator = WasmCodeGenerator::new().unwrap();

        let wat_code = r#"(module
  (func $add (param $a i32) (param $b i32) (result i32)
    local.get $a
    local.get $b
    i32.add)
  (export "add" (func $add))
)"#;

        let result = generator
            .generate_from_source(wat_code, SourceLanguage::Wat, Some("Simple add function".to_string()))
            .await
            .unwrap();

        assert_eq!(result.language, SourceLanguage::Wat);
        assert!(result.compilation_success, "Wat code should compile successfully");
        assert!(result.wasm_bytes.is_some(), "Should have WASM bytes");
    }

    #[test]
    fn test_extract_code_with_code_blocks() {
        let generator = WasmCodeGenerator::new().unwrap();

        let response = r#"Here's the code:

```wat
(module
  (func (export "main")
    nop
  )
)
```

That should work."#;

        let code = generator.extract_code(response, &SourceLanguage::Wat).unwrap();
        assert!(code.contains("(module"));
        assert!(code.contains("(export \"main\""));
    }

    #[test]
    fn test_extract_code_without_code_blocks() {
        let generator = WasmCodeGenerator::new().unwrap();

        let response = "(module (func (export \"main\") nop))";
        let code = generator.extract_code(response, &SourceLanguage::Wat).unwrap();
        assert_eq!(code, response);
    }

    #[test]
    fn test_prompt_building() {
        let generator = WasmCodeGenerator::new().unwrap();

        let rust_prompt = generator.build_rust_prompt("Create a temperature monitor");
        assert!(rust_prompt.contains("temperature monitor"));
        assert!(rust_prompt.contains("wasm-bindgen"));

        let wat_prompt = generator.build_wat_prompt("Create a counter");
        assert!(wat_prompt.contains("counter"));
        assert!(wat_prompt.contains("WebAssembly Text Format"));
    }
}
