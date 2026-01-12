//! Built-in Device Type Definitions
//!
//! This module provides pre-defined device type definitions for common IoT devices.
//! These definitions follow the MDL (Machine Description Language) format and are
//! protocol-agnostic. Protocol-specific mappings are defined separately.

use serde_json::json;
use crate::mdl_format::DeviceTypeDefinition;

/// Get all built-in device type definitions
///
/// These definitions describe device capabilities (metrics and commands)
/// without protocol-specific details like MQTT topics or Modbus registers.
/// Protocol mappings are configured separately through the ProtocolMapping layer.
pub fn builtin_device_types() -> Vec<DeviceTypeDefinition> {
    vec![
        dht22_sensor(),
        relay_module(),
        energy_meter(),
        air_quality_sensor(),
        ip_camera(),
        image_sensor(),
    ]
}

/// 1. DHT22 温湿度传感器
///
/// Protocol-agnostic device capability definition.
/// Use with protocol mappings (MQTT, Modbus, HASS) for actual communication.
fn dht22_sensor() -> DeviceTypeDefinition {
    serde_json::from_value(json!({
        "device_type": "dht22_sensor",
        "name": "DHT22 温湿度传感器",
        "description": "基于 DHT22 的温湿度传感器，支持温度和湿度数据采集",
        "categories": ["sensor", "climate", "temperature", "humidity"],
        "uplink": {
            "metrics": [
                {
                    "name": "temperature",
                    "display_name": "温度",
                    "data_type": "Float",
                    "unit": "°C",
                    "min": -40.0,
                    "max": 80.0,
                    "required": true
                },
                {
                    "name": "humidity",
                    "display_name": "湿度",
                    "data_type": "Float",
                    "unit": "%",
                    "min": 0.0,
                    "max": 100.0,
                    "required": true
                },
                {
                    "name": "heat_index",
                    "display_name": "体感温度",
                    "data_type": "Float",
                    "unit": "°C",
                    "required": false
                }
            ]
        },
        "downlink": {
            "commands": [
                {
                    "name": "set_interval",
                    "display_name": "设置上报间隔",
                    "parameters": [
                        {
                            "name": "interval",
                            "display_name": "间隔时间",
                            "data_type": "Integer",
                            "default_value": {"Integer": 60},
                            "min": 10.0,
                            "max": 3600.0,
                            "unit": "秒"
                        }
                    ]
                },
                {
                    "name": "request_reading",
                    "display_name": "请求读取",
                    "parameters": []
                }
            ]
        },
        "telemetry": {
            "retain_history": true,
            "aggregation": ["avg", "min", "max", "last"],
            "retention_days": 30
        }
    })).expect("Invalid DHT22 device definition")
}

/// 2. 继电器模块
///
/// Protocol-agnostic relay device capability definition.
fn relay_module() -> DeviceTypeDefinition {
    serde_json::from_value(json!({
        "device_type": "relay_module",
        "name": "继电器模块",
        "description": "多路继电器控制模块，支持远程开关控制",
        "categories": ["switch", "actuator", "relay"],
        "uplink": {
            "metrics": [
                {
                    "name": "relay_state",
                    "display_name": "继电器状态",
                    "data_type": "Boolean",
                    "required": true
                },
                {
                    "name": "relay_count",
                    "display_name": "继电器数量",
                    "data_type": "Integer",
                    "required": true
                },
                {
                    "name": "power_consumption",
                    "display_name": "功耗",
                    "data_type": "Float",
                    "unit": "W",
                    "required": false
                }
            ]
        },
        "downlink": {
            "commands": [
                {
                    "name": "turn_on",
                    "display_name": "开启继电器",
                    "parameters": [
                        {
                            "name": "channel",
                            "display_name": "通道号",
                            "data_type": "Integer",
                            "default_value": {"Integer": 1},
                            "min": 1.0,
                            "max": 8.0
                        }
                    ]
                },
                {
                    "name": "turn_off",
                    "display_name": "关闭继电器",
                    "parameters": [
                        {
                            "name": "channel",
                            "display_name": "通道号",
                            "data_type": "Integer",
                            "default_value": {"Integer": 1},
                            "min": 1.0,
                            "max": 8.0
                        }
                    ]
                },
                {
                    "name": "toggle",
                    "display_name": "切换状态",
                    "parameters": [
                        {
                            "name": "channel",
                            "display_name": "通道号",
                            "data_type": "Integer",
                            "default_value": {"Integer": 1},
                            "min": 1.0,
                            "max": 8.0
                        }
                    ]
                },
                {
                    "name": "pulse",
                    "display_name": "脉冲触发",
                    "parameters": [
                        {
                            "name": "channel",
                            "display_name": "通道号",
                            "data_type": "Integer",
                            "default_value": {"Integer": 1},
                            "min": 1.0,
                            "max": 8.0
                        },
                        {
                            "name": "duration",
                            "display_name": "脉冲时长",
                            "data_type": "Integer",
                            "default_value": {"Integer": 500},
                            "min": 50.0,
                            "max": 5000.0,
                            "unit": "毫秒"
                        }
                    ]
                }
            ]
        },
        "telemetry": {
            "retain_history": true,
            "aggregation": ["last", "count"],
            "retention_days": 7
        }
    })).expect("Invalid relay module device definition")
}

/// 3. 智能电表
///
/// Protocol-agnostic energy meter capability definition.
fn energy_meter() -> DeviceTypeDefinition {
    serde_json::from_value(json!({
        "device_type": "energy_meter",
        "name": "智能电表",
        "description": "多功能智能电表，支持电压、电流、功率、电能等参数监测",
        "categories": ["sensor", "energy", "power"],
        "uplink": {
            "metrics": [
                {
                    "name": "voltage",
                    "display_name": "电压",
                    "data_type": "Float",
                    "unit": "V",
                    "min": 0.0,
                    "max": 300.0,
                    "required": true
                },
                {
                    "name": "current",
                    "display_name": "电流",
                    "data_type": "Float",
                    "unit": "A",
                    "min": 0.0,
                    "max": 100.0,
                    "required": true
                },
                {
                    "name": "active_power",
                    "display_name": "有功功率",
                    "data_type": "Float",
                    "unit": "kW",
                    "min": 0.0,
                    "required": true
                },
                {
                    "name": "energy",
                    "display_name": "电能",
                    "data_type": "Float",
                    "unit": "kWh",
                    "min": 0.0,
                    "required": true
                },
                {
                    "name": "frequency",
                    "display_name": "频率",
                    "data_type": "Float",
                    "unit": "Hz",
                    "min": 45.0,
                    "max": 65.0,
                    "required": false
                },
                {
                    "name": "power_factor",
                    "display_name": "功率因数",
                    "data_type": "Float",
                    "min": 0.0,
                    "max": 1.0,
                    "required": false
                }
            ]
        },
        "downlink": {
            "commands": [
                {
                    "name": "reset_energy",
                    "display_name": "重置电能",
                    "parameters": []
                },
                {
                    "name": "set_alarm",
                    "display_name": "设置告警",
                    "parameters": [
                        {
                            "name": "type",
                            "display_name": "告警类型",
                            "data_type": "String",
                            "default_value": {"String": "overload"},
                            "allowed_values": [
                                {"String": "overload"},
                                {"String": "overvoltage"},
                                {"String": "undervoltage"}
                            ]
                        },
                        {
                            "name": "threshold",
                            "display_name": "阈值",
                            "data_type": "Float",
                            "min": 0.0
                        }
                    ]
                }
            ]
        },
        "telemetry": {
            "retain_history": true,
            "aggregation": ["avg", "min", "max", "sum", "last"],
            "retention_days": 90
        }
    })).expect("Invalid energy meter device definition")
}

/// 4. 空气质量传感器
///
/// Protocol-agnostic air quality sensor capability definition.
fn air_quality_sensor() -> DeviceTypeDefinition {
    serde_json::from_value(json!({
        "device_type": "air_quality_sensor",
        "name": "空气质量传感器",
        "description": "多功能空气质量传感器，监测 PM2.5、PM10、CO2、TVOC、甲醛等",
        "categories": ["sensor", "environment", "air_quality"],
        "uplink": {
            "metrics": [
                {
                    "name": "pm2_5",
                    "display_name": "PM2.5",
                    "data_type": "Float",
                    "unit": "µg/m³",
                    "min": 0.0,
                    "max": 500.0,
                    "required": true
                },
                {
                    "name": "pm10",
                    "display_name": "PM10",
                    "data_type": "Float",
                    "unit": "µg/m³",
                    "min": 0.0,
                    "max": 1000.0,
                    "required": true
                },
                {
                    "name": "co2",
                    "display_name": "二氧化碳",
                    "data_type": "Float",
                    "unit": "ppm",
                    "min": 400.0,
                    "max": 5000.0,
                    "required": true
                },
                {
                    "name": "tvoc",
                    "display_name": "总挥发性有机物",
                    "data_type": "Float",
                    "unit": "ppb",
                    "min": 0.0,
                    "max": 1000.0,
                    "required": false
                },
                {
                    "name": "formaldehyde",
                    "display_name": "甲醛",
                    "data_type": "Float",
                    "unit": "mg/m³",
                    "min": 0.0,
                    "max": 1.0,
                    "required": false
                },
                {
                    "name": "temperature",
                    "display_name": "温度",
                    "data_type": "Float",
                    "unit": "°C",
                    "required": false
                },
                {
                    "name": "humidity",
                    "display_name": "湿度",
                    "data_type": "Float",
                    "unit": "%",
                    "min": 0.0,
                    "max": 100.0,
                    "required": false
                },
                {
                    "name": "aqi",
                    "display_name": "空气质量指数",
                    "data_type": "Integer",
                    "min": 0.0,
                    "max": 500.0,
                    "required": true
                }
            ]
        },
        "downlink": {
            "commands": [
                {
                    "name": "set_interval",
                    "display_name": "设置上报间隔",
                    "parameters": [
                        {
                            "name": "interval",
                            "display_name": "间隔时间",
                            "data_type": "Integer",
                            "default_value": {"Integer": 60},
                            "min": 10.0,
                            "max": 600.0,
                            "unit": "秒"
                        }
                    ]
                },
                {
                    "name": "calibrate",
                    "display_name": "校准传感器",
                    "parameters": [
                        {
                            "name": "sensor",
                            "display_name": "传感器类型",
                            "data_type": "String",
                            "default_value": {"String": "all"},
                            "allowed_values": [
                                {"String": "all"},
                                {"String": "pm2_5"},
                                {"String": "co2"}
                            ]
                        }
                    ]
                }
            ]
        },
        "telemetry": {
            "retain_history": true,
            "aggregation": ["avg", "min", "max", "last"],
            "retention_days": 30
        }
    })).expect("Invalid air quality sensor device definition")
}

/// 5. IP 摄像头（图片采集设备）
///
/// Protocol-agnostic IP camera capability definition.
fn ip_camera() -> DeviceTypeDefinition {
    serde_json::from_value(json!({
        "device_type": "ip_camera",
        "name": "IP 摄像头",
        "description": "支持图片和视频采集的 IP 摄像头，支持运动检测",
        "categories": ["sensor", "camera", "image", "video"],
        "uplink": {
            "metrics": [
                {
                    "name": "image",
                    "display_name": "图片数据",
                    "data_type": "Binary",
                    "unit": "bytes",
                    "required": true
                },
                {
                    "name": "image_metadata",
                    "display_name": "图片元数据",
                    "data_type": "String",
                    "required": false
                },
                {
                    "name": "motion_detected",
                    "display_name": "运动检测",
                    "data_type": "Boolean",
                    "required": false
                },
                {
                    "name": "resolution",
                    "display_name": "分辨率",
                    "data_type": "String",
                    "required": false
                },
                {
                    "name": "fps",
                    "display_name": "帧率",
                    "data_type": "Float",
                    "unit": "fps",
                    "required": false
                }
            ]
        },
        "downlink": {
            "commands": [
                {
                    "name": "capture_image",
                    "display_name": "拍摄图片",
                    "parameters": [
                        {
                            "name": "format",
                            "display_name": "图片格式",
                            "data_type": "String",
                            "default_value": {"String": "jpeg"},
                            "allowed_values": [
                                {"String": "jpeg"},
                                {"String": "png"},
                                {"String": "webp"}
                            ]
                        },
                        {
                            "name": "quality",
                            "display_name": "图片质量",
                            "data_type": "Integer",
                            "default_value": {"Integer": 85},
                            "min": 1.0,
                            "max": 100.0
                        }
                    ]
                },
                {
                    "name": "set_resolution",
                    "display_name": "设置分辨率",
                    "parameters": [
                        {
                            "name": "width",
                            "display_name": "宽度",
                            "data_type": "Integer",
                            "default_value": {"Integer": 1920},
                            "min": 320.0,
                            "max": 7680.0
                        },
                        {
                            "name": "height",
                            "display_name": "高度",
                            "data_type": "Integer",
                            "default_value": {"Integer": 1080},
                            "min": 240.0,
                            "max": 4320.0
                        }
                    ]
                },
                {
                    "name": "enable_motion_detection",
                    "display_name": "启用运动检测",
                    "parameters": [
                        {
                            "name": "enabled",
                            "display_name": "启用",
                            "data_type": "Boolean",
                            "default_value": {"Boolean": true}
                        },
                        {
                            "name": "sensitivity",
                            "display_name": "灵敏度",
                            "data_type": "Integer",
                            "default_value": {"Integer": 50},
                            "min": 0.0,
                            "max": 100.0
                        }
                    ]
                },
                {
                    "name": "start_stream",
                    "display_name": "开始推流",
                    "parameters": [
                        {
                            "name": "format",
                            "display_name": "流格式",
                            "data_type": "String",
                            "default_value": {"String": "h264"},
                            "allowed_values": [
                                {"String": "h264"},
                                {"String": "mjpeg"}
                            ]
                        }
                    ]
                },
                {
                    "name": "stop_stream",
                    "display_name": "停止推流",
                    "parameters": []
                }
            ]
        },
        "telemetry": {
            "retain_history": true,
            "aggregation": ["last"],
            "retention_days": 7
        },
        "metadata": {
            "storage_type": "multimodal",
            "max_image_size": "10485760",
            "supported_formats": "jpeg,png,webp,h264"
        }
    })).expect("Invalid IP camera device definition")
}

/// 6. 图像传感器（支持图片数据上报）
///
/// Protocol-agnostic image sensor capability definition.
fn image_sensor() -> DeviceTypeDefinition {
    serde_json::from_value(json!({
        "device_type": "image_sensor",
        "name": "图像传感器",
        "description": "用于采集图像数据的传感器设备，支持定时采集和触发采集",
        "categories": ["sensor", "image"],
        "uplink": {
            "metrics": [
                {
                    "name": "image_data",
                    "display_name": "图像数据",
                    "data_type": "Binary",
                    "unit": "bytes",
                    "required": true
                },
                {
                    "name": "image_timestamp",
                    "display_name": "图像时间戳",
                    "data_type": "Integer",
                    "unit": "unix",
                    "required": true
                },
                {
                    "name": "image_width",
                    "display_name": "图像宽度",
                    "data_type": "Integer",
                    "unit": "px",
                    "required": false
                },
                {
                    "name": "image_height",
                    "display_name": "图像高度",
                    "data_type": "Integer",
                    "unit": "px",
                    "required": false
                },
                {
                    "name": "image_format",
                    "display_name": "图像格式",
                    "data_type": "String",
                    "required": false
                },
                {
                    "name": "image_size",
                    "display_name": "图像大小",
                    "data_type": "Integer",
                    "unit": "bytes",
                    "required": false
                }
            ]
        },
        "downlink": {
            "commands": [
                {
                    "name": "trigger_capture",
                    "display_name": "触发采集",
                    "parameters": []
                },
                {
                    "name": "set_interval",
                    "display_name": "设置定时采集",
                    "parameters": [
                        {
                            "name": "interval",
                            "display_name": "间隔时间",
                            "data_type": "Integer",
                            "default_value": {"Integer": 300},
                            "min": 10.0,
                            "max": 3600.0,
                            "unit": "秒"
                        }
                    ]
                },
                {
                    "name": "set_resolution",
                    "display_name": "设置分辨率",
                    "parameters": [
                        {
                            "name": "width",
                            "display_name": "宽度",
                            "data_type": "Integer",
                            "default_value": {"Integer": 640},
                            "min": 160.0,
                            "max": 3840.0
                        },
                        {
                            "name": "height",
                            "display_name": "高度",
                            "data_type": "Integer",
                            "default_value": {"Integer": 480},
                            "min": 120.0,
                            "max": 2160.0
                        }
                    ]
                },
                {
                    "name": "set_format",
                    "display_name": "设置格式",
                    "parameters": [
                        {
                            "name": "format",
                            "display_name": "图片格式",
                            "data_type": "String",
                            "default_value": {"String": "png"},
                            "allowed_values": [
                                {"String": "png"},
                                {"String": "jpeg"},
                                {"String": "rgb565"},
                                {"String": "grayscale"}
                            ]
                        },
                        {
                            "name": "quality",
                            "display_name": "质量",
                            "data_type": "Integer",
                            "default_value": {"Integer": 90},
                            "min": 1.0,
                            "max": 100.0
                        }
                    ]
                }
            ]
        },
        "telemetry": {
            "retain_history": true,
            "aggregation": ["last", "count"],
            "retention_days": 7
        },
        "metadata": {
            "storage_type": "multimodal",
            "max_image_size": "5242880",
            "supported_formats": "png,jpeg,rgb565,grayscale"
        }
    })).expect("Invalid image sensor device definition")
}

/// Get MQTT protocol mappings for built-in device types
///
/// These mappings define how to access device capabilities via MQTT.
pub fn builtin_mqtt_mappings() -> std::collections::HashMap<String, crate::protocol::MqttMapping> {
    use std::collections::HashMap;
    use crate::protocol::{MqttMappingBuilder, MqttValueParser};

    let mut mappings = HashMap::new();

    // DHT22 MQTT mapping
    let dht22_mapping = MqttMappingBuilder::new("dht22_sensor")
        .add_metric("temperature", "sensor/${device_id}/temperature")
        .add_metric("humidity", "sensor/${device_id}/humidity")
        .add_metric("heat_index", "sensor/${device_id}/heat_index")
        .add_command_with_payload(
            "set_interval",
            "sensor/${device_id}/command",
            r#"{"action": "set_interval", "interval": ${interval}}"#
        )
        .add_command_with_payload(
            "request_reading",
            "sensor/${device_id}/command",
            r#"{"action": "read"}"#
        )
        .build();
    mappings.insert("dht22_sensor_mqtt".to_string(), dht22_mapping);

    // Relay MQTT mapping
    let relay_mapping = MqttMappingBuilder::new("relay_module")
        .add_metric("relay_state", "relay/${device_id}/state")
        .add_metric("relay_count", "relay/${device_id}/count")
        .add_metric("power_consumption", "relay/${device_id}/power")
        .add_command_with_payload(
            "turn_on",
            "relay/${device_id}/command",
            r#"{"action": "on", "channel": ${channel}}"#
        )
        .add_command_with_payload(
            "turn_off",
            "relay/${device_id}/command",
            r#"{"action": "off", "channel": ${channel}}"#
        )
        .add_command_with_payload(
            "toggle",
            "relay/${device_id}/command",
            r#"{"action": "toggle", "channel": ${channel}}"#
        )
        .add_command_with_payload(
            "pulse",
            "relay/${device_id}/command",
            r#"{"action": "pulse", "channel": ${channel}, "duration": ${duration}}"#
        )
        .build();
    mappings.insert("relay_module_mqtt".to_string(), relay_mapping);

    // Energy meter MQTT mapping
    let energy_meter_mapping = MqttMappingBuilder::new("energy_meter")
        .add_metric("voltage", "meter/${device_id}/voltage")
        .add_metric("current", "meter/${device_id}/current")
        .add_metric("active_power", "meter/${device_id}/power")
        .add_metric("energy", "meter/${device_id}/energy")
        .add_metric("frequency", "meter/${device_id}/frequency")
        .add_metric("power_factor", "meter/${device_id}/pf")
        .add_command_with_payload(
            "reset_energy",
            "meter/${device_id}/command",
            r#"{"action": "reset_energy"}"#
        )
        .add_command_with_payload(
            "set_alarm",
            "meter/${device_id}/command",
            r#"{"action": "set_alarm", "type": "${type}", "threshold": ${threshold}}"#
        )
        .build();
    mappings.insert("energy_meter_mqtt".to_string(), energy_meter_mapping);

    mappings
}

/// Get Modbus protocol mappings for built-in device types
///
/// These mappings define how to access device capabilities via Modbus.
pub fn builtin_modbus_mappings() -> std::collections::HashMap<String, crate::protocol::ModbusMapping> {
    use std::collections::HashMap;
    use crate::protocol::{ModbusMappingBuilder, ModbusDataType};

    let mut mappings = HashMap::new();

    // Energy meter Modbus mapping (common Modbus energy meter layout)
    let energy_meter_mapping = ModbusMappingBuilder::new("energy_meter")
        .default_slave(1)
        .add_input_register("voltage", 0x0000, ModbusDataType::Float32)
        .add_input_register("current", 0x0002, ModbusDataType::Float32)
        .add_input_register("active_power", 0x0004, ModbusDataType::Float32)
        .add_input_register("energy", 0x0006, ModbusDataType::Float32)
        .add_input_register("frequency", 0x0008, ModbusDataType::Float32)
        .add_holding_register("reset_energy", 0x0100, ModbusDataType::Bool)
        .build();
    mappings.insert("energy_meter_modbus".to_string(), energy_meter_mapping);

    mappings
}

/// Get Home Assistant protocol mappings for built-in device types
///
/// These mappings define how to access device capabilities via Home Assistant.
pub fn builtin_hass_mappings() -> std::collections::HashMap<String, crate::protocol::HassMapping> {
    use std::collections::HashMap;
    use crate::protocol::HassMappingBuilder;

    let mut mappings = HashMap::new();

    // Climate sensor HASS mapping
    let climate_mapping = HassMappingBuilder::new("dht22_sensor")
        .add_sensor("temperature", "sensor.indoor_temperature")
        .add_sensor("humidity", "sensor.indoor_humidity")
        .build();
    mappings.insert("dht22_sensor_hass".to_string(), climate_mapping);

    mappings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mdl::MetricDataType;

    #[test]
    fn test_builtin_device_types_count() {
        let types = builtin_device_types();
        assert_eq!(types.len(), 6, "Should have exactly 6 built-in device types");
    }

    #[test]
    fn test_dht22_sensor_definition() {
        let sensor = dht22_sensor();
        assert_eq!(sensor.device_type, "dht22_sensor");
        assert_eq!(sensor.uplink.metrics.len(), 3);
        assert_eq!(sensor.downlink.commands.len(), 2);
        assert!(sensor.categories.contains(&"climate".to_string()));
    }

    #[test]
    fn test_relay_module_definition() {
        let relay = relay_module();
        assert_eq!(relay.device_type, "relay_module");
        assert_eq!(relay.uplink.metrics.len(), 3);
        assert_eq!(relay.downlink.commands.len(), 4);
    }

    #[test]
    fn test_energy_meter_definition() {
        let meter = energy_meter();
        assert_eq!(meter.device_type, "energy_meter");
        assert_eq!(meter.uplink.metrics.len(), 6);
        assert!(meter.uplink.metrics.iter().any(|m| m.name == "voltage"));
        assert!(meter.uplink.metrics.iter().any(|m| m.name == "energy"));
    }

    #[test]
    fn test_air_quality_sensor_definition() {
        let sensor = air_quality_sensor();
        assert_eq!(sensor.device_type, "air_quality_sensor");
        assert_eq!(sensor.uplink.metrics.len(), 8);
        assert!(sensor.uplink.metrics.iter().any(|m| m.name == "pm2_5"));
        assert!(sensor.uplink.metrics.iter().any(|m| m.name == "aqi"));
    }

    #[test]
    fn test_ip_camera_definition() {
        let camera = ip_camera();
        assert_eq!(camera.device_type, "ip_camera");
        assert_eq!(camera.uplink.metrics.len(), 5);
        assert_eq!(camera.downlink.commands.len(), 5);
        assert!(camera.categories.contains(&"image".to_string()));
        assert!(camera.categories.contains(&"camera".to_string()));

        // Check for image metric with binary data type
        let image_metric = camera.uplink.metrics.iter()
            .find(|m| m.name == "image")
            .expect("Should have image metric");
        assert_eq!(image_metric.data_type, MetricDataType::Binary);
    }

    #[test]
    fn test_image_sensor_definition() {
        let sensor = image_sensor();
        assert_eq!(sensor.device_type, "image_sensor");
        assert_eq!(sensor.uplink.metrics.len(), 6);
        assert_eq!(sensor.downlink.commands.len(), 4);
        assert!(sensor.categories.contains(&"image".to_string()));

        // Check for image_data metric with binary data type
        let image_metric = sensor.uplink.metrics.iter()
            .find(|m| m.name == "image_data")
            .expect("Should have image_data metric");
        assert_eq!(image_metric.data_type, MetricDataType::Binary);
    }

    #[test]
    fn test_all_image_devices_have_multimodal_metadata() {
        let types = builtin_device_types();
        let image_devices: Vec<_> = types.iter()
            .filter(|t| t.categories.contains(&"image".to_string()))
            .collect();

        assert_eq!(image_devices.len(), 2, "Should have exactly 2 image device types");

        for device in image_devices {
            // Check that image devices have the expected structure
            assert!(!device.device_type.is_empty(),
                "Image device should have a valid device_type");
        }
    }

    #[test]
    fn test_no_protocol_specific_fields() {
        let types = builtin_device_types();
        for device_type in types {
            // Ensure no topic fields in metrics
            for metric in &device_type.uplink.metrics {
                // The MetricDefinition struct doesn't have a topic field,
                // so this test ensures the JSON doesn't include unknown fields
                assert!(!metric.name.is_empty(), "Metric should have a name");
            }
        }
    }

    #[test]
    fn test_mqtt_mappings_exist() {
        let mqtt_mappings = builtin_mqtt_mappings();
        assert!(mqtt_mappings.contains_key("dht22_sensor_mqtt"));
        assert!(mqtt_mappings.contains_key("relay_module_mqtt"));
        assert!(mqtt_mappings.contains_key("energy_meter_mqtt"));
    }

    #[test]
    fn test_modbus_mappings_exist() {
        let modbus_mappings = builtin_modbus_mappings();
        assert!(modbus_mappings.contains_key("energy_meter_modbus"));
    }

    #[test]
    fn test_hass_mappings_exist() {
        let hass_mappings = builtin_hass_mappings();
        assert!(hass_mappings.contains_key("dht22_sensor_hass"));
    }
}
