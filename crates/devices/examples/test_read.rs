use edge_ai_devices::telemetry::{TimeSeriesStorage};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let storage = TimeSeriesStorage::open("data/telemetry_persistence_test.redb")?;
    
    let now = chrono::Utc::now().timestamp();
    
    // Try to read all metric types
    println!("Testing data persistence after restart...");
    
    let metrics = vec!["metric.int", "metric.float", "metric.string", "metric.bool"];
    
    for metric in metrics {
        let results = storage.query("device1", metric, now - 1000, now + 1000).await?;
        println!("{}: {} data points", metric, results.len());
        for point in results {
            println!("  Value: {:?}", point.value);
        }
    }
    
    Ok(())
}
