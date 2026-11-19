use crate::types::CliResponse;
use crate::ApiClient;
use anyhow::Result;

/// Export full system configuration
pub async fn export_config(client: &ApiClient) -> Result<CliResponse> {
    let data = client.get("/config/export").await?;
    Ok(CliResponse::success(data, "Configuration exported"))
}

/// Import system configuration from JSON
pub async fn import_config(client: &ApiClient, config_json: &str) -> Result<CliResponse> {
    let config = crate::json_arg::parse_json_arg(config_json)
        .map_err(|e| anyhow::anyhow!("Invalid JSON config: {}", e))?;
    let data = client.post("/config/import", &config).await?;
    Ok(CliResponse::success(data, "Configuration imported"))
}

/// Validate configuration without applying
pub async fn validate_config(client: &ApiClient, config_json: &str) -> Result<CliResponse> {
    let config = crate::json_arg::parse_json_arg(config_json)
        .map_err(|e| anyhow::anyhow!("Invalid JSON config: {}", e))?;
    let data = client.post("/config/validate", &config).await?;
    Ok(CliResponse::success(data, "Configuration validated"))
}
