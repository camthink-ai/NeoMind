//! Image Analyzer V2 - Computer vision analysis using usls library.
//!
//! This tool provides object detection capabilities using YOLO models via the usls library.
//! Supports multiple image sources: file paths, URLs, and Base64 encoded images.

use async_trait::async_trait;
use base64::{engine::general_purpose, Engine as _};
use image::DynamicImage;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use usls::models::YOLO;
use usls::{Config, Model};

use crate::error::{Result, ToolError};
use crate::tool::{object_schema, Tool, ToolOutput};

/// Image source type
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ImageSource {
    /// File path on local filesystem
    FilePath { path: String },
    /// URL to download image from
    Url { url: String },
    /// Base64 encoded image data
    Base64 { data: String },
}

/// Detection result for a single object
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Detection {
    /// Object class/label
    pub label: String,
    /// Confidence score (0.0 - 1.0)
    pub confidence: f32,
    /// Bounding box coordinates [x1, y1, x2, y2]
    pub bbox: [f32; 4],
    /// Optional class ID
    pub class_id: Option<usize>,
}

/// Image analysis result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisResult {
    /// Image dimensions [width, height]
    pub image_size: [u32; 2],
    /// Detected objects
    pub detections: Vec<Detection>,
    /// Total number of detections
    pub total_detections: usize,
    /// Model used for inference
    pub model_info: String,
    /// Inference time in milliseconds
    pub inference_time_ms: Option<f64>,
}

/// Image Analyzer V2 Tool
pub struct ImageAnalyzerTool {
    /// YOLO model instance (lazy loaded)
    model: Option<usls::Runtime<YOLO>>,
    /// Model configuration
    model_config: ModelConfig,
}

/// Model configuration
#[derive(Debug, Clone)]
pub struct ModelConfig {
    /// YOLO version (e.g., "v8", "v11")
    pub version: String,
    /// Model scale (e.g., "n", "s", "m", "l", "x")
    pub scale: String,
    /// Confidence threshold
    pub confidence_threshold: f32,
    /// IoU threshold for NMS
    pub iou_threshold: f32,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            version: "v11".to_string(),
            scale: "n".to_string(),
            confidence_threshold: 0.25,
            iou_threshold: 0.45,
        }
    }
}

impl ImageAnalyzerTool {
    /// Create a new Image Analyzer tool with default configuration
    pub fn new() -> Self {
        Self {
            model: None,
            model_config: ModelConfig::default(),
        }
    }

    /// Create with custom model configuration
    pub fn with_config(config: ModelConfig) -> Self {
        Self {
            model: None,
            model_config: config,
        }
    }

    /// Load the YOLO model (lazy initialization)
    fn load_model(&mut self) -> Result<&mut usls::Runtime<YOLO>> {
        if self.model.is_none() {
            tracing::info!(
                "Loading YOLO model: v{} scale={}",
                self.model_config.version,
                self.model_config.scale
            );

            // Build configuration
            // Note: Using default "yolo" name since with_name requires 'static lifetime
            let config = Config::yolo()
                .with_iou(self.model_config.iou_threshold)
                .with_class_confs(&[self.model_config.confidence_threshold])
                .commit()
                .map_err(|e| ToolError::Execution(format!("Failed to build config: {}", e)))?;

            // Create YOLO model
            let model = YOLO::new(config)
                .map_err(|e| ToolError::Execution(format!("Failed to load YOLO model: {}", e)))?;

            self.model = Some(model);
        }

        Ok(self.model.as_mut().unwrap())
    }

    /// Load image from source
    async fn load_image(&self, source: &ImageSource) -> Result<DynamicImage> {
        match source {
            ImageSource::FilePath { path } => {
                tracing::debug!("Loading image from file: {}", path);
                image::open(path)
                    .map_err(|e| ToolError::Execution(format!("Failed to load image from file: {}", e)))
            }

            ImageSource::Url { url } => {
                tracing::debug!("Downloading image from URL: {}", url);
                let response = reqwest::blocking::get(url)
                    .map_err(|e| ToolError::Execution(format!("Failed to download image: {}", e)))?;

                let bytes = response.bytes()
                    .map_err(|e| ToolError::Execution(format!("Failed to read image bytes: {}", e)))?;

                image::load_from_memory(&bytes)
                    .map_err(|e| ToolError::Execution(format!("Failed to decode image: {}", e)))
            }

            ImageSource::Base64 { data } => {
                tracing::debug!("Decoding Base64 image");
                let bytes = general_purpose::STANDARD
                    .decode(data)
                    .map_err(|e| ToolError::Execution(format!("Failed to decode Base64: {}", e)))?;

                image::load_from_memory(&bytes)
                    .map_err(|e| ToolError::Execution(format!("Failed to decode image: {}", e)))
            }
        }
    }

    /// Perform object detection on image
    async fn detect_objects(&mut self, image: &DynamicImage) -> Result<AnalysisResult> {
        let start_time = std::time::Instant::now();

        // Load model
        let model = self.load_model()?;

        // Convert DynamicImage to usls::Image
        let usls_image = usls::Image::try_from(image.clone())
            .map_err(|e| ToolError::Execution(format!("Failed to convert image: {}", e)))?;

        // Run inference
        let ys = model
            .forward(&[usls_image])
            .map_err(|e| ToolError::Execution(format!("Inference failed: {}", e)))?;

        let inference_time = start_time.elapsed().as_secs_f64() * 1000.0;

        // Extract detections from horizontal bounding boxes
        let detections: Vec<Detection> = ys
            .iter()
            .flat_map(|y| &y.hbbs)
            .map(|hbb| Detection {
                label: hbb.name().unwrap_or("unknown").to_string(),
                confidence: hbb.confidence().unwrap_or(0.0),
                bbox: [hbb.xmin(), hbb.ymin(), hbb.xmax(), hbb.ymax()],
                class_id: hbb.id(),
            })
            .collect();

        let total_detections = detections.len();

        Ok(AnalysisResult {
            image_size: [image.width(), image.height()],
            detections,
            total_detections,
            model_info: format!("YOLO{}{}", self.model_config.version, self.model_config.scale),
            inference_time_ms: Some(inference_time),
        })
    }
}

impl Default for ImageAnalyzerTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for ImageAnalyzerTool {
    fn name(&self) -> &str {
        "image_analyzer_v2"
    }

    fn description(&self) -> &str {
        "Analyze images using computer vision models (YOLO). Detects objects, their locations, and confidence scores. Supports file paths, URLs, and Base64 encoded images."
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "source": {
                    "type": "object",
                    "description": "Image source specification",
                    "oneOf": [
                        {
                            "type": "object",
                            "properties": {
                                "type": { "type": "string", "enum": ["file_path"] },
                                "path": { "type": "string", "description": "Local file path to image" }
                            },
                            "required": ["type", "path"]
                        },
                        {
                            "type": "object",
                            "properties": {
                                "type": { "type": "string", "enum": ["url"] },
                                "url": { "type": "string", "description": "URL to download image from" }
                            },
                            "required": ["type", "url"]
                        },
                        {
                            "type": "object",
                            "properties": {
                                "type": { "type": "string", "enum": ["base64"] },
                                "data": { "type": "string", "description": "Base64 encoded image data" }
                            },
                            "required": ["type", "data"]
                        }
                    ]
                },
                "confidence_threshold": {
                    "type": "number",
                    "description": "Minimum confidence threshold for detections (0.0-1.0)",
                    "default": 0.25
                },
                "max_detections": {
                    "type": "integer",
                    "description": "Maximum number of detections to return",
                    "default": 100
                }
            }),
            vec!["source".to_string()],
        )
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        self.validate_args(&args)?;

        // Parse image source
        let source: ImageSource = serde_json::from_value(args["source"].clone())
            .map_err(|e| ToolError::InvalidArguments(format!("Invalid image source: {}", e)))?;

        // Load image
        let image = self.load_image(&source).await?;

        // Create mutable self for model loading
        let mut analyzer = Self {
            model: None,
            model_config: self.model_config.clone(),
        };

        // Override confidence threshold if provided
        if let Some(threshold) = args.get("confidence_threshold").and_then(|v| v.as_f64()) {
            analyzer.model_config.confidence_threshold = threshold as f32;
        }

        // Perform detection
        let mut result = analyzer.detect_objects(&image).await?;

        // Apply max_detections limit if specified
        if let Some(max_det) = args.get("max_detections").and_then(|v| v.as_u64()) {
            let max_det = max_det as usize;
            if result.detections.len() > max_det {
                result.detections.truncate(max_det);
                result.total_detections = max_det;
            }
        }

        Ok(ToolOutput::success(serde_json::to_value(result)?))
    }

    fn category(&self) -> crate::tool::ToolCategory {
        crate::tool::ToolCategory::Analysis
    }

    fn scenarios(&self) -> Vec<crate::tool::UsageScenario> {
        vec![
            crate::tool::UsageScenario {
                description: "Detect objects in security camera footage".to_string(),
                example_query: "Analyze the image from camera-01 and tell me what objects are detected".to_string(),
                suggested_call: None,
            },
            crate::tool::UsageScenario {
                description: "Count people in a room".to_string(),
                example_query: "How many people are in this image?".to_string(),
                suggested_call: None,
            },
            crate::tool::UsageScenario {
                description: "Identify vehicles in parking lot".to_string(),
                example_query: "What vehicles are visible in the parking lot camera?".to_string(),
                suggested_call: None,
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_config_default() {
        let config = ModelConfig::default();
        assert_eq!(config.version, "v11");
        assert_eq!(config.scale, "n");
        assert_eq!(config.confidence_threshold, 0.25);
    }

    #[test]
    fn test_tool_metadata() {
        let tool = ImageAnalyzerTool::new();
        assert_eq!(tool.name(), "image_analyzer_v2");
        assert!(!tool.description().is_empty());
        assert!(!tool.scenarios().is_empty());
    }

    #[tokio::test]
    async fn test_base64_decode() {
        let tool = ImageAnalyzerTool::new();
        
        // Create a simple 1x1 red pixel PNG in base64
        let base64_data = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg==";
        
        let source = ImageSource::Base64 {
            data: base64_data.to_string(),
        };
        
        let result = tool.load_image(&source).await;
        assert!(result.is_ok());
    }
}
