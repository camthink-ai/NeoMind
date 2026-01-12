// Test image storage and retrieval
use edge_ai_devices::telemetry::{TimeSeriesStorage, DataPoint};
use edge_ai_devices::mdl::MetricValue;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let storage = TimeSeriesStorage::open("data/test_images.redb")?;

    let now = chrono::Utc::now().timestamp();

    // Create a small PNG (1x1 red pixel)
    // This is a valid PNG file in base64
    let png_base64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
    let png_bytes = base64::decode(png_base64)?;

    // Store as binary metric
    storage.write(
        "camera_device",
        "image",
        DataPoint::new(now, MetricValue::Binary(png_bytes))
    ).await?;

    println!("Image data stored successfully!");

    // Query it back
    let results = storage.query("camera_device", "image", now - 10, now + 10).await?;
    println!("Retrieved {} data points", results.len());

    for point in results {
        match &point.value {
            MetricValue::Binary(data) => {
                let encoded = base64::encode(data);
                println!("Binary data (base64): {}...", &encoded[..50]);

                // Check if it starts with PNG signature
                if encoded.starts_with("iVBORw0KGgo") {
                    println!("This is a PNG image!");
                }
            }
            _ => println!("Not binary data: {:?}", point.value),
        }
    }

    Ok(())
}
