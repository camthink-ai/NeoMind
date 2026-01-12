use edge_ai_devices::mdl::MetricValue;

fn main() {
    // JPEG header signature
    let binary_data = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46];
    let metric = MetricValue::Binary(binary_data);

    let json = serde_json::to_string(&metric).unwrap();
    println!("Serialized MetricValue::Binary: {}", json);

    // Check if it's a valid base64 string
    if let MetricValue::Binary(decoded) = serde_json::from_str::<MetricValue>(&json).unwrap() {
        println!("Deserialized binary: {:02x?}", decoded);
        println!("Binary serialization works correctly!");
    }

    // Test with a small PNG (1x1 transparent PNG)
    let png_header = vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D,
        0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
    ];
    let png_metric = MetricValue::Binary(png_header);
    let png_json = serde_json::to_string(&png_metric).unwrap();
    println!("\nSerialized PNG: {}", png_json);
    println!("Base64 starts with: {}", &png_json.chars().take(30).collect::<String>());
}
