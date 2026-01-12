//! Home Assistant device templates for mapping entities to NeoTalk devices.

use crate::mdl::MetricDataType;
use crate::mdl_format::ParameterDefinition;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A Home Assistant device template.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HassDeviceTemplate {
    /// Template identifier
    pub name: String,

    /// Display name
    pub display_name: String,

    /// Description
    pub description: String,

    /// NeoTalk device type
    pub device_type: String,

    /// HASS domain
    pub domain: String,

    /// HASS device class (optional)
    pub device_class: Option<String>,

    /// Metrics provided by this template
    pub metrics: Vec<TemplateMetric>,

    /// Commands provided by this template
    pub commands: Vec<TemplateCommand>,

    /// Configuration parameters
    pub parameters: Vec<ParameterDefinition>,
}

/// Metric definition in a template.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateMetric {
    /// Metric name
    pub name: String,

    /// Data type
    pub data_type: MetricDataType,

    /// Unit of measurement
    pub unit: Option<String>,

    /// Whether metric is read-only
    pub read_only: bool,

    /// Minimum value (for numeric types)
    pub min: Option<f64>,

    /// Maximum value (for numeric types)
    pub max: Option<f64>,

    /// Description
    pub description: Option<String>,
}

/// Command definition in a template.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateCommand {
    /// Command name
    pub name: String,

    /// HASS service domain
    pub domain: String,

    /// HASS service name
    pub service: String,

    /// Default service data
    pub data: Option<serde_json::Value>,

    /// Command parameters
    pub parameters: Vec<ParameterDefinition>,

    /// Description
    pub description: Option<String>,
}

impl HassDeviceTemplate {
    /// Create a new template.
    pub fn new(
        name: String,
        display_name: String,
        description: String,
        device_type: String,
        domain: String,
    ) -> Self {
        Self {
            name,
            display_name,
            description,
            device_type,
            domain,
            device_class: None,
            metrics: Vec::new(),
            commands: Vec::new(),
            parameters: Vec::new(),
        }
    }

    /// Add a metric to the template.
    pub fn with_metric(mut self, metric: TemplateMetric) -> Self {
        self.metrics.push(metric);
        self
    }

    /// Add a command to the template.
    pub fn with_command(mut self, command: TemplateCommand) -> Self {
        self.commands.push(command);
        self
    }

    /// Set device class.
    pub fn with_device_class(mut self, class: String) -> Self {
        self.device_class = Some(class);
        self
    }
}

/// Get built-in HASS device templates.
pub fn builtin_templates() -> HashMap<String, HassDeviceTemplate> {
    let mut templates = HashMap::new();

    // ==================== SENSOR TEMPLATES ====================

    // Temperature sensor
    templates.insert(
        "sensor_temperature".to_string(),
        HassDeviceTemplate::new(
            "sensor_temperature".to_string(),
            "Temperature Sensor".to_string(),
            "A temperature sensor providing temperature readings".to_string(),
            "sensor".to_string(),
            "sensor".to_string(),
        )
        .with_device_class("temperature".to_string())
        .with_metric(TemplateMetric {
            name: "temperature".to_string(),
            data_type: MetricDataType::Float,
            unit: Some("°C".to_string()),
            read_only: true,
            min: Some(-50.0),
            max: Some(150.0),
            description: Some("Current temperature".to_string()),
        }),
    );

    // Humidity sensor
    templates.insert(
        "sensor_humidity".to_string(),
        HassDeviceTemplate::new(
            "sensor_humidity".to_string(),
            "Humidity Sensor".to_string(),
            "A humidity sensor providing relative humidity readings".to_string(),
            "sensor".to_string(),
            "sensor".to_string(),
        )
        .with_device_class("humidity".to_string())
        .with_metric(TemplateMetric {
            name: "humidity".to_string(),
            data_type: MetricDataType::Float,
            unit: Some("%".to_string()),
            read_only: true,
            min: Some(0.0),
            max: Some(100.0),
            description: Some("Current relative humidity".to_string()),
        }),
    );

    // Pressure sensor
    templates.insert(
        "sensor_pressure".to_string(),
        HassDeviceTemplate::new(
            "sensor_pressure".to_string(),
            "Pressure Sensor".to_string(),
            "A barometric pressure sensor".to_string(),
            "sensor".to_string(),
            "sensor".to_string(),
        )
        .with_device_class("pressure".to_string())
        .with_metric(TemplateMetric {
            name: "pressure".to_string(),
            data_type: MetricDataType::Float,
            unit: Some("hPa".to_string()),
            read_only: true,
            min: Some(800.0),
            max: Some(1200.0),
            description: Some("Barometric pressure".to_string()),
        }),
    );

    // Battery sensor
    templates.insert(
        "sensor_battery".to_string(),
        HassDeviceTemplate::new(
            "sensor_battery".to_string(),
            "Battery Sensor".to_string(),
            "A battery level sensor".to_string(),
            "sensor".to_string(),
            "sensor".to_string(),
        )
        .with_device_class("battery".to_string())
        .with_metric(TemplateMetric {
            name: "battery_level".to_string(),
            data_type: MetricDataType::Float,
            unit: Some("%".to_string()),
            read_only: true,
            min: Some(0.0),
            max: Some(100.0),
            description: Some("Battery level".to_string()),
        }),
    );

    // Illuminance sensor
    templates.insert(
        "sensor_illuminance".to_string(),
        HassDeviceTemplate::new(
            "sensor_illuminance".to_string(),
            "Illuminance Sensor".to_string(),
            "A light/illuminance sensor".to_string(),
            "sensor".to_string(),
            "sensor".to_string(),
        )
        .with_device_class("illuminance".to_string())
        .with_metric(TemplateMetric {
            name: "illuminance".to_string(),
            data_type: MetricDataType::Float,
            unit: Some("lx".to_string()),
            read_only: true,
            min: Some(0.0),
            max: Some(100000.0),
            description: Some("Light intensity".to_string()),
        }),
    );

    // Power sensor
    templates.insert(
        "sensor_power".to_string(),
        HassDeviceTemplate::new(
            "sensor_power".to_string(),
            "Power Sensor".to_string(),
            "A power consumption sensor".to_string(),
            "sensor".to_string(),
            "sensor".to_string(),
        )
        .with_device_class("power".to_string())
        .with_metric(TemplateMetric {
            name: "power".to_string(),
            data_type: MetricDataType::Float,
            unit: Some("W".to_string()),
            read_only: true,
            min: Some(0.0),
            max: None,
            description: Some("Current power consumption".to_string()),
        }),
    );

    // Energy sensor
    templates.insert(
        "sensor_energy".to_string(),
        HassDeviceTemplate::new(
            "sensor_energy".to_string(),
            "Energy Sensor".to_string(),
            "An energy consumption sensor".to_string(),
            "sensor".to_string(),
            "sensor".to_string(),
        )
        .with_device_class("energy".to_string())
        .with_metric(TemplateMetric {
            name: "energy".to_string(),
            data_type: MetricDataType::Float,
            unit: Some("kWh".to_string()),
            read_only: true,
            min: Some(0.0),
            max: None,
            description: Some("Total energy consumption".to_string()),
        }),
    );

    // Generic sensor
    templates.insert(
        "sensor_generic".to_string(),
        HassDeviceTemplate::new(
            "sensor_generic".to_string(),
            "Generic Sensor".to_string(),
            "A generic sensor for any type of measurement".to_string(),
            "sensor".to_string(),
            "sensor".to_string(),
        )
        .with_metric(TemplateMetric {
            name: "value".to_string(),
            data_type: MetricDataType::String,
            unit: None,
            read_only: true,
            min: None,
            max: None,
            description: Some("Sensor value".to_string()),
        }),
    );

    // ==================== BINARY SENSOR TEMPLATES ====================

    templates.insert(
        "binary_sensor".to_string(),
        HassDeviceTemplate::new(
            "binary_sensor".to_string(),
            "Binary Sensor".to_string(),
            "A binary (on/off) sensor".to_string(),
            "binary_sensor".to_string(),
            "binary_sensor".to_string(),
        )
        .with_metric(TemplateMetric {
            name: "state".to_string(),
            data_type: MetricDataType::Boolean,
            unit: None,
            read_only: true,
            min: None,
            max: None,
            description: Some("Sensor state (on/off)".to_string()),
        }),
    );

    // ==================== SWITCH TEMPLATES ====================

    templates.insert(
        "switch".to_string(),
        HassDeviceTemplate::new(
            "switch".to_string(),
            "Switch".to_string(),
            "A controllable switch".to_string(),
            "switch".to_string(),
            "switch".to_string(),
        )
        .with_metric(TemplateMetric {
            name: "state".to_string(),
            data_type: MetricDataType::Boolean,
            unit: None,
            read_only: false,
            min: None,
            max: None,
            description: Some("Switch state".to_string()),
        })
        .with_command(TemplateCommand {
            name: "turn_on".to_string(),
            domain: "switch".to_string(),
            service: "turn_on".to_string(),
            data: None,
            parameters: vec![],
            description: Some("Turn the switch on".to_string()),
        })
        .with_command(TemplateCommand {
            name: "turn_off".to_string(),
            domain: "switch".to_string(),
            service: "turn_off".to_string(),
            data: None,
            parameters: vec![],
            description: Some("Turn the switch off".to_string()),
        })
        .with_command(TemplateCommand {
            name: "toggle".to_string(),
            domain: "switch".to_string(),
            service: "toggle".to_string(),
            data: None,
            parameters: vec![],
            description: Some("Toggle the switch".to_string()),
        }),
    );

    // ==================== LIGHT TEMPLATES ====================

    templates.insert(
        "light".to_string(),
        HassDeviceTemplate::new(
            "light".to_string(),
            "Light".to_string(),
            "A controllable light".to_string(),
            "light".to_string(),
            "light".to_string(),
        )
        .with_metric(TemplateMetric {
            name: "state".to_string(),
            data_type: MetricDataType::Boolean,
            unit: None,
            read_only: false,
            min: None,
            max: None,
            description: Some("Light state".to_string()),
        })
        .with_metric(TemplateMetric {
            name: "brightness".to_string(),
            data_type: MetricDataType::Integer,
            unit: Some("%".to_string()),
            read_only: false,
            min: Some(0.0),
            max: Some(100.0),
            description: Some("Brightness level".to_string()),
        })
        .with_metric(TemplateMetric {
            name: "color_temp".to_string(),
            data_type: MetricDataType::Integer,
            unit: Some("mired".to_string()),
            read_only: false,
            min: Some(153.0),
            max: Some(500.0),
            description: Some("Color temperature".to_string()),
        })
        .with_metric(TemplateMetric {
            name: "rgb_color".to_string(),
            data_type: MetricDataType::String,
            unit: None,
            read_only: false,
            min: None,
            max: None,
            description: Some("RGB color".to_string()),
        })
        .with_command(TemplateCommand {
            name: "turn_on".to_string(),
            domain: "light".to_string(),
            service: "turn_on".to_string(),
            data: None,
            parameters: vec![],
            description: Some("Turn the light on".to_string()),
        })
        .with_command(TemplateCommand {
            name: "turn_off".to_string(),
            domain: "light".to_string(),
            service: "turn_off".to_string(),
            data: None,
            parameters: vec![],
            description: Some("Turn the light off".to_string()),
        })
        .with_command(TemplateCommand {
            name: "toggle".to_string(),
            domain: "light".to_string(),
            service: "toggle".to_string(),
            data: None,
            parameters: vec![],
            description: Some("Toggle the light".to_string()),
        }),
    );

    // ==================== COVER TEMPLATES ====================

    templates.insert(
        "cover".to_string(),
        HassDeviceTemplate::new(
            "cover".to_string(),
            "Cover".to_string(),
            "A window cover, blind, or shade".to_string(),
            "cover".to_string(),
            "cover".to_string(),
        )
        .with_metric(TemplateMetric {
            name: "state".to_string(),
            data_type: MetricDataType::String,
            unit: None,
            read_only: false,
            min: None,
            max: None,
            description: Some("Cover state (open/closed)".to_string()),
        })
        .with_metric(TemplateMetric {
            name: "position".to_string(),
            data_type: MetricDataType::Integer,
            unit: Some("%".to_string()),
            read_only: false,
            min: Some(0.0),
            max: Some(100.0),
            description: Some("Cover position".to_string()),
        })
        .with_command(TemplateCommand {
            name: "open".to_string(),
            domain: "cover".to_string(),
            service: "open_cover".to_string(),
            data: None,
            parameters: vec![],
            description: Some("Open the cover".to_string()),
        })
        .with_command(TemplateCommand {
            name: "close".to_string(),
            domain: "cover".to_string(),
            service: "close_cover".to_string(),
            data: None,
            parameters: vec![],
            description: Some("Close the cover".to_string()),
        })
        .with_command(TemplateCommand {
            name: "stop".to_string(),
            domain: "cover".to_string(),
            service: "stop_cover".to_string(),
            data: None,
            parameters: vec![],
            description: Some("Stop the cover".to_string()),
        })
        .with_command(TemplateCommand {
            name: "set_position".to_string(),
            domain: "cover".to_string(),
            service: "set_cover_position".to_string(),
            data: Some(serde_json::json!({"position": 50})),
            parameters: vec![ParameterDefinition {
                name: "position".to_string(),
                display_name: "Position".to_string(),
                data_type: MetricDataType::Integer,
                default_value: None,
                min: Some(0.0),
                max: Some(100.0),
                unit: String::new(),
                allowed_values: vec![],
            }],
            description: Some("Set cover position".to_string()),
        }),
    );

    // ==================== CLIMATE TEMPLATES ====================

    templates.insert(
        "climate".to_string(),
        HassDeviceTemplate::new(
            "climate".to_string(),
            "Thermostat".to_string(),
            "A thermostat or climate control device".to_string(),
            "climate".to_string(),
            "climate".to_string(),
        )
        .with_metric(TemplateMetric {
            name: "temperature".to_string(),
            data_type: MetricDataType::Float,
            unit: Some("°C".to_string()),
            read_only: false,
            min: Some(5.0),
            max: Some(40.0),
            description: Some("Current temperature".to_string()),
        })
        .with_metric(TemplateMetric {
            name: "target_temperature".to_string(),
            data_type: MetricDataType::Float,
            unit: Some("°C".to_string()),
            read_only: false,
            min: Some(5.0),
            max: Some(40.0),
            description: Some("Target temperature".to_string()),
        })
        .with_metric(TemplateMetric {
            name: "hvac_mode".to_string(),
            data_type: MetricDataType::String,
            unit: None,
            read_only: false,
            min: None,
            max: None,
            description: Some("HVAC mode".to_string()),
        })
        .with_command(TemplateCommand {
            name: "set_temperature".to_string(),
            domain: "climate".to_string(),
            service: "set_temperature".to_string(),
            data: Some(serde_json::json!({"temperature": 20})),
            parameters: vec![ParameterDefinition {
                name: "temperature".to_string(),
                display_name: "Temperature".to_string(),
                data_type: MetricDataType::Float,
                default_value: None,
                min: Some(5.0),
                max: Some(40.0),
                unit: "°C".to_string(),
                allowed_values: vec![],
            }],
            description: Some("Set target temperature".to_string()),
        })
        .with_command(TemplateCommand {
            name: "set_hvac_mode".to_string(),
            domain: "climate".to_string(),
            service: "set_hvac_mode".to_string(),
            data: Some(serde_json::json!({"hvac_mode": "heat"})),
            parameters: vec![ParameterDefinition {
                name: "hvac_mode".to_string(),
                display_name: "HVAC Mode".to_string(),
                data_type: MetricDataType::String,
                default_value: None,
                min: None,
                max: None,
                unit: String::new(),
                allowed_values: vec![
                    crate::mdl::MetricValue::String("heat".to_string()),
                    crate::mdl::MetricValue::String("cool".to_string()),
                    crate::mdl::MetricValue::String("auto".to_string()),
                    crate::mdl::MetricValue::String("off".to_string()),
                ],
            }],
            description: Some("Set HVAC mode".to_string()),
        }),
    );

    // ==================== CAMERA TEMPLATES ====================

    templates.insert(
        "camera".to_string(),
        HassDeviceTemplate::new(
            "camera".to_string(),
            "Camera".to_string(),
            "A camera device".to_string(),
            "camera".to_string(),
            "camera".to_string(),
        )
        .with_metric(TemplateMetric {
            name: "stream_url".to_string(),
            data_type: MetricDataType::String,
            unit: None,
            read_only: true,
            min: None,
            max: None,
            description: Some("Stream URL".to_string()),
        }),
    );

    // ==================== FAN TEMPLATES ====================

    templates.insert(
        "fan".to_string(),
        HassDeviceTemplate::new(
            "fan".to_string(),
            "Fan".to_string(),
            "A controllable fan".to_string(),
            "fan".to_string(),
            "fan".to_string(),
        )
        .with_metric(TemplateMetric {
            name: "state".to_string(),
            data_type: MetricDataType::Boolean,
            unit: None,
            read_only: false,
            min: None,
            max: None,
            description: Some("Fan state".to_string()),
        })
        .with_metric(TemplateMetric {
            name: "speed".to_string(),
            data_type: MetricDataType::String,
            unit: None,
            read_only: false,
            min: None,
            max: None,
            description: Some("Fan speed".to_string()),
        })
        .with_command(TemplateCommand {
            name: "turn_on".to_string(),
            domain: "fan".to_string(),
            service: "turn_on".to_string(),
            data: None,
            parameters: vec![],
            description: Some("Turn the fan on".to_string()),
        })
        .with_command(TemplateCommand {
            name: "turn_off".to_string(),
            domain: "fan".to_string(),
            service: "turn_off".to_string(),
            data: None,
            parameters: vec![],
            description: Some("Turn the fan off".to_string()),
        })
        .with_command(TemplateCommand {
            name: "toggle".to_string(),
            domain: "fan".to_string(),
            service: "toggle".to_string(),
            data: None,
            parameters: vec![],
            description: Some("Toggle the fan".to_string()),
        })
        .with_command(TemplateCommand {
            name: "set_speed".to_string(),
            domain: "fan".to_string(),
            service: "set_speed".to_string(),
            data: Some(serde_json::json!({"speed": "low"})),
            parameters: vec![ParameterDefinition {
                name: "speed".to_string(),
                display_name: "Fan Speed".to_string(),
                data_type: MetricDataType::String,
                default_value: None,
                min: None,
                max: None,
                unit: String::new(),
                allowed_values: vec![
                    crate::mdl::MetricValue::String("low".to_string()),
                    crate::mdl::MetricValue::String("medium".to_string()),
                    crate::mdl::MetricValue::String("high".to_string()),
                ],
            }],
            description: Some("Set fan speed".to_string()),
        }),
    );

    // ==================== LOCK TEMPLATES ====================

    templates.insert(
        "lock".to_string(),
        HassDeviceTemplate::new(
            "lock".to_string(),
            "Lock".to_string(),
            "A smart lock".to_string(),
            "lock".to_string(),
            "lock".to_string(),
        )
        .with_metric(TemplateMetric {
            name: "state".to_string(),
            data_type: MetricDataType::Boolean,
            unit: None,
            read_only: false,
            min: None,
            max: None,
            description: Some("Lock state (locked/unlocked)".to_string()),
        })
        .with_command(TemplateCommand {
            name: "lock".to_string(),
            domain: "lock".to_string(),
            service: "lock".to_string(),
            data: None,
            parameters: vec![],
            description: Some("Lock the lock".to_string()),
        })
        .with_command(TemplateCommand {
            name: "unlock".to_string(),
            domain: "lock".to_string(),
            service: "unlock".to_string(),
            data: None,
            parameters: vec![],
            description: Some("Unlock the lock".to_string()),
        }),
    );

    // ==================== MEDIA PLAYER TEMPLATES ====================

    templates.insert(
        "media_player".to_string(),
        HassDeviceTemplate::new(
            "media_player".to_string(),
            "Media Player".to_string(),
            "A media player device".to_string(),
            "media_player".to_string(),
            "media_player".to_string(),
        )
        .with_metric(TemplateMetric {
            name: "state".to_string(),
            data_type: MetricDataType::String,
            unit: None,
            read_only: false,
            min: None,
            max: None,
            description: Some("Player state".to_string()),
        })
        .with_metric(TemplateMetric {
            name: "volume_level".to_string(),
            data_type: MetricDataType::Float,
            unit: None,
            read_only: false,
            min: Some(0.0),
            max: Some(1.0),
            description: Some("Volume level".to_string()),
        })
        .with_metric(TemplateMetric {
            name: "media_title".to_string(),
            data_type: MetricDataType::String,
            unit: None,
            read_only: true,
            min: None,
            max: None,
            description: Some("Current media title".to_string()),
        })
        .with_command(TemplateCommand {
            name: "turn_on".to_string(),
            domain: "media_player".to_string(),
            service: "turn_on".to_string(),
            data: None,
            parameters: vec![],
            description: Some("Turn on the media player".to_string()),
        })
        .with_command(TemplateCommand {
            name: "turn_off".to_string(),
            domain: "media_player".to_string(),
            service: "turn_off".to_string(),
            data: None,
            parameters: vec![],
            description: Some("Turn off the media player".to_string()),
        })
        .with_command(TemplateCommand {
            name: "play".to_string(),
            domain: "media_player".to_string(),
            service: "media_play".to_string(),
            data: None,
            parameters: vec![],
            description: Some("Play media".to_string()),
        })
        .with_command(TemplateCommand {
            name: "pause".to_string(),
            domain: "media_player".to_string(),
            service: "media_pause".to_string(),
            data: None,
            parameters: vec![],
            description: Some("Pause media".to_string()),
        }),
    );

    // ==================== GENERIC TEMPLATE ====================

    templates.insert(
        "generic".to_string(),
        HassDeviceTemplate::new(
            "generic".to_string(),
            "Generic Device".to_string(),
            "A generic device template".to_string(),
            "generic".to_string(),
            "unknown".to_string(),
        )
        .with_metric(TemplateMetric {
            name: "state".to_string(),
            data_type: MetricDataType::String,
            unit: None,
            read_only: true,
            min: None,
            max: None,
            description: Some("Device state".to_string()),
        }),
    );

    templates
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_templates_exist() {
        let templates = builtin_templates();

        // Check some key templates exist
        assert!(templates.contains_key("sensor_temperature"));
        assert!(templates.contains_key("sensor_humidity"));
        assert!(templates.contains_key("switch"));
        assert!(templates.contains_key("light"));
        assert!(templates.contains_key("cover"));
        assert!(templates.contains_key("climate"));
        assert!(templates.contains_key("camera"));
    }

    #[test]
    fn test_sensor_template_metrics() {
        let templates = builtin_templates();
        let temp_sensor = templates.get("sensor_temperature").unwrap();

        assert_eq!(temp_sensor.metrics.len(), 1);
        assert_eq!(temp_sensor.metrics[0].name, "temperature");
        assert_eq!(temp_sensor.metrics[0].unit, Some("°C".to_string()));
    }

    #[test]
    fn test_light_template_metrics_and_commands() {
        let templates = builtin_templates();
        let light = templates.get("light").unwrap();

        assert!(light.metrics.len() >= 3);
        assert_eq!(light.metrics[0].name, "state");
        assert_eq!(light.metrics[1].name, "brightness");

        assert_eq!(light.commands.len(), 3);
        assert_eq!(light.commands[0].name, "turn_on");
        assert_eq!(light.commands[1].name, "turn_off");
        assert_eq!(light.commands[2].name, "toggle");
    }
}
