//! MQTT Mock Device for Testing
//!
//! This module provides a mock MQTT device implementation for testing the
//! device management system. Supports various device types including image
//! capture devices.

use rumqttc::{AsyncClient, MqttOptions, QoS};
use serde_json::json;
use std::time::Duration;
use tokio::time::sleep;

/// Mock MQTT device simulator
pub struct MqttMockDevice {
    client: AsyncClient,
    device_id: String,
    device_type: String,
    name: Option<String>,
}

impl MqttMockDevice {
    /// Create a new mock MQTT device
    pub async fn new(
        device_id: impl Into<String>,
        device_type: impl Into<String>,
        broker_addr: impl Into<String>,
        port: u16,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let device_id = device_id.into();
        let device_type = device_type.into();

        let mut mqttoptions = MqttOptions::new(
            format!("mock_device_{}", device_id),
            broker_addr.into(),
            port,
        );
        mqttoptions.set_keep_alive(Duration::from_secs(60));

        let (client, mut eventloop) = AsyncClient::new(mqttoptions, 10);

        // Start the event loop in the background
        let device_id_clone = device_id.clone();
        tokio::spawn(async move {
            let mut eventloop = eventloop;
            loop {
                match eventloop.poll().await {
                    Ok(_) => {}
                    Err(e) => {
                        eprintln!("MQTT mock device {} error: {}", device_id_clone, e);
                        sleep(Duration::from_secs(1)).await;
                    }
                }
            }
        });

        // Wait a bit for connection
        sleep(Duration::from_millis(100)).await;

        Ok(Self {
            client,
            device_id,
            device_type,
            name: None,
        })
    }

    /// Set device name
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Announce device presence (discovery)
    pub async fn announce(&self) -> Result<(), Box<dyn std::error::Error>> {
        let announcement = json!({
            "device_type": self.device_type,
            "device_id": self.device_id,
            "name": self.name.as_ref().unwrap_or(&self.device_id),
            "timestamp": chrono::Utc::now().timestamp()
        });

        let topic = "neomind/discovery/announce";
        let payload: Vec<u8> = serde_json::to_vec(&announcement)?;
        self.client
            .publish(topic, QoS::AtLeastOnce, false, payload)
            .await?;

        println!("Mock device {} announced", self.device_id);
        Ok(())
    }

    /// Publish a metric value
    pub async fn publish_metric(
        &self,
        metric_name: &str,
        value: serde_json::Value,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let topic = self.build_metric_topic(metric_name);
        let payload = value.to_string().into_bytes();
        self.client
            .publish(&topic, QoS::AtLeastOnce, false, payload)
            .await?;
        Ok(())
    }

    /// Publish numeric metric
    pub async fn publish_float(
        &self,
        metric_name: &str,
        value: f64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.publish_metric(metric_name, json!(value)).await
    }

    /// Publish integer metric
    pub async fn publish_int(
        &self,
        metric_name: &str,
        value: i64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.publish_metric(metric_name, json!(value)).await
    }

    /// Publish boolean metric
    pub async fn publish_bool(
        &self,
        metric_name: &str,
        value: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.publish_metric(metric_name, json!(value)).await
    }

    /// Publish string metric
    pub async fn publish_string(
        &self,
        metric_name: &str,
        value: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.publish_metric(metric_name, json!(value)).await
    }

    /// Publish image data (Base64 encoded)
    pub async fn publish_image(
        &self,
        metric_name: &str,
        image_data: &[u8],
        mime_type: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let topic = self.build_metric_topic(metric_name);

        // Encode image data as Base64
        let base64_data = base64::encode(image_data);
        let payload_json = json!({
            "data": base64_data,
            "mime_type": mime_type,
            "timestamp": chrono::Utc::now().timestamp(),
            "size": image_data.len()
        });

        let payload: Vec<u8> = serde_json::to_vec(&payload_json)?;
        self.client
            .publish(&topic, QoS::AtLeastOnce, false, payload)
            .await?;

        println!(
            "Mock device {} published image: {} bytes",
            self.device_id,
            image_data.len()
        );
        Ok(())
    }

    /// Publish image metadata
    pub async fn publish_image_metadata(
        &self,
        width: u32,
        height: u32,
        format: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let topic = self.build_metric_topic("metadata");
        let payload_json = json!({
            "width": width,
            "height": height,
            "format": format,
            "timestamp": chrono::Utc::now().timestamp()
        });

        let payload: Vec<u8> = serde_json::to_vec(&payload_json)?;
        self.client
            .publish(&topic, QoS::AtLeastOnce, false, payload)
            .await?;

        Ok(())
    }

    /// Build topic for a metric
    fn build_metric_topic(&self, metric_name: &str) -> String {
        // Based on device type, build appropriate topic
        match self.device_type.as_str() {
            "dht22_sensor" => format!("sensor/{}/{}", self.device_id, metric_name),
            "relay_module" => format!("relay/{}/{}", self.device_id, metric_name),
            "energy_meter" => format!("meter/{}/{}", self.device_id, metric_name),
            "air_quality_sensor" => format!("air/{}/{}", self.device_id, metric_name),
            "ip_camera" => format!("camera/{}/{}", self.device_id, metric_name),
            "image_sensor" => format!("sensor/{}/{}", self.device_id, metric_name),
            _ => format!("device/{}/{}", self.device_id, metric_name),
        }
    }

    /// Get device ID
    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    /// Get device type
    pub fn device_type(&self) -> &str {
        &self.device_type
    }
}

/// DHT22 sensor simulator
pub struct Dht22MockDevice {
    device: MqttMockDevice,
}

impl Dht22MockDevice {
    pub async fn new(
        device_id: impl Into<String>,
        broker: &str,
        port: u16,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let device = MqttMockDevice::new(device_id, "dht22_sensor", broker, port)
            .await?
            .with_name("DHT22 温湿度传感器");
        Ok(Self { device })
    }

    pub async fn announce(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.device.announce().await
    }

    pub async fn publish_reading(
        &self,
        temperature: f64,
        humidity: f64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.device
            .publish_float("temperature", temperature)
            .await?;
        self.device.publish_float("humidity", humidity).await?;
        Ok(())
    }

    pub fn device_id(&self) -> &str {
        self.device.device_id()
    }
}

/// IP Camera simulator
pub struct IpCameraMockDevice {
    device: MqttMockDevice,
    image_counter: usize,
}

impl IpCameraMockDevice {
    pub async fn new(
        device_id: impl Into<String>,
        broker: &str,
        port: u16,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let device = MqttMockDevice::new(device_id, "ip_camera", broker, port)
            .await?
            .with_name("IP 摄像头");
        Ok(Self {
            device,
            image_counter: 0,
        })
    }

    pub async fn announce(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.device.announce().await
    }

    /// Capture and publish a mock image
    pub async fn capture_image(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Generate a small mock JPEG image (just random bytes for testing)
        let image_size = 1024 + (self.image_counter % 10) * 512;
        let mock_image_data = vec![0xFFu8; image_size]; // JPEG SOI marker + padding

        self.device
            .publish_image("image", &mock_image_data, "image/jpeg")
            .await?;
        self.device
            .publish_image_metadata(1920, 1080, "jpeg")
            .await?;
        self.device
            .publish_string("resolution", "1920x1080")
            .await?;
        self.device.publish_float("fps", 30.0).await?;

        Ok(())
    }

    /// Publish motion detection event
    pub async fn publish_motion(&self, detected: bool) -> Result<(), Box<dyn std::error::Error>> {
        self.device.publish_bool("motion_detected", detected).await
    }

    pub fn device_id(&self) -> &str {
        self.device.device_id()
    }
}

/// Image sensor simulator
pub struct ImageSensorMockDevice {
    device: MqttMockDevice,
    image_counter: usize,
}

impl ImageSensorMockDevice {
    pub async fn new(
        device_id: impl Into<String>,
        broker: &str,
        port: u16,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let device = MqttMockDevice::new(device_id, "image_sensor", broker, port)
            .await?
            .with_name("图像传感器");
        Ok(Self {
            device,
            image_counter: 0,
        })
    }

    pub async fn announce(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.device.announce().await
    }

    /// Trigger image capture
    pub async fn trigger_capture(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Generate a small mock PNG image
        let image_size = 2048 + (self.image_counter % 5) * 1024;
        let mock_image_data = vec![0x89u8, 0x50, 0x4E, 0x47]; // PNG signature + padding
        let mut full_image = mock_image_data;
        full_image.resize(image_size, 0);

        self.device
            .publish_image("image_data", &full_image, "image/png")
            .await?;
        self.device
            .publish_int("image_timestamp", chrono::Utc::now().timestamp())
            .await?;
        self.device.publish_int("image_width", 640).await?;
        self.device.publish_int("image_height", 480).await?;
        self.device.publish_string("image_format", "png").await?;
        self.device
            .publish_int("image_size", image_size as i64)
            .await?;

        Ok(())
    }

    pub fn device_id(&self) -> &str {
        self.device.device_id()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires MQTT broker
    async fn test_mock_device_creation() {
        let device = MqttMockDevice::new("test_001", "dht22_sensor", "localhost", 1883).await;
        assert!(device.is_ok());
    }

    #[tokio::test]
    #[ignore]
    async fn test_dht22_mock_device() {
        let sensor = Dht22MockDevice::new("dht22_001", "localhost", 1883)
            .await
            .unwrap();
        sensor.announce().await.unwrap();
        sensor.publish_reading(25.5, 60.0).await.unwrap();
    }

    #[tokio::test]
    #[ignore]
    async fn test_ip_camera_mock_device() {
        let camera = IpCameraMockDevice::new("cam_001", "localhost", 1883)
            .await
            .unwrap();
        camera.announce().await.unwrap();
        camera.capture_image().await.unwrap();
        camera.publish_motion(true).await.unwrap();
    }

    #[tokio::test]
    #[ignore]
    async fn test_image_sensor_mock_device() {
        let sensor = ImageSensorMockDevice::new("img_sensor_001", "localhost", 1883)
            .await
            .unwrap();
        sensor.announce().await.unwrap();
        sensor.trigger_capture().await.unwrap();
    }
}
