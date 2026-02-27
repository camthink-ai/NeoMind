//! WASM Host Function Bindings
//!
//! This module provides Rust bindings for host functions that can be called
//! from WASM extensions.

/// Host function imports from NeoMind runtime
#[link(wasm_import_module = "neomind")]
extern "C" {
    /// Make an HTTP request
    ///
    /// # Arguments
    /// - `method_ptr`: Pointer to method string (GET, POST, etc.)
    /// - `method_len`: Length of method string
    /// - `url_ptr`: Pointer to URL string
    /// - `url_len`: Length of URL string
    /// - `result_ptr`: Pointer to result buffer
    /// - `result_max_len`: Maximum result buffer size
    ///
    /// # Returns
    /// Length of result on success, -1 on error
    pub fn host_http_request(
        method_ptr: *const u8,
        method_len: i32,
        url_ptr: *const u8,
        url_len: i32,
        result_ptr: *mut u8,
        result_max_len: i32,
    ) -> i32;

    /// Log a message
    ///
    /// # Arguments
    /// - `level_ptr`: Pointer to level string (debug, info, warn, error)
    /// - `level_len`: Length of level string
    /// - `msg_ptr`: Pointer to message string
    /// - `msg_len`: Length of message string
    pub fn host_log(
        level_ptr: *const u8,
        level_len: i32,
        msg_ptr: *const u8,
        msg_len: i32,
    );

    /// Store a metric value
    ///
    /// # Arguments
    /// - `name_ptr`: Pointer to metric name
    /// - `name_len`: Length of metric name
    /// - `value_ptr`: Pointer to JSON-encoded value
    /// - `value_len`: Length of value
    pub fn host_store_metric(
        name_ptr: *const u8,
        name_len: i32,
        value_ptr: *const u8,
        value_len: i32,
    );

    /// Read device data
    ///
    /// # Arguments
    /// - `device_id_ptr`: Pointer to device ID
    /// - `device_id_len`: Length of device ID
    /// - `metric_ptr`: Pointer to metric name
    /// - `metric_len`: Length of metric name
    /// - `result_ptr`: Pointer to result buffer
    /// - `result_max_len`: Maximum result buffer size
    ///
    /// # Returns
    /// Length of result on success, -1 on error
    pub fn host_device_read(
        device_id_ptr: *const u8,
        device_id_len: i32,
        metric_ptr: *const u8,
        metric_len: i32,
        result_ptr: *mut u8,
        result_max_len: i32,
    ) -> i32;

    /// Write to a device
    ///
    /// # Arguments
    /// - `device_id_ptr`: Pointer to device ID
    /// - `device_id_len`: Length of device ID
    /// - `command_ptr`: Pointer to command name
    /// - `command_len`: Length of command name
    /// - `params_ptr`: Pointer to JSON-encoded parameters
    /// - `params_len`: Length of parameters
    /// - `result_ptr`: Pointer to result buffer
    /// - `result_max_len`: Maximum result buffer size
    ///
    /// # Returns
    /// Length of result on success, -1 on error
    pub fn host_device_write(
        device_id_ptr: *const u8,
        device_id_len: i32,
        command_ptr: *const u8,
        command_len: i32,
        params_ptr: *const u8,
        params_len: i32,
        result_ptr: *mut u8,
        result_max_len: i32,
    ) -> i32;
}

/// High-level API for making HTTP requests from WASM
pub fn http_request(method: &str, url: &str) -> Result<serde_json::Value, String> {
    let mut result_buffer = vec![0u8; 65536];

    let result_len = unsafe {
        host_http_request(
            method.as_ptr(),
            method.len() as i32,
            url.as_ptr(),
            url.len() as i32,
            result_buffer.as_mut_ptr(),
            result_buffer.len() as i32,
        )
    };

    if result_len < 0 {
        return Err("HTTP request failed".to_string());
    }

    let end = result_buffer
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(result_len as usize);
    let json_str = String::from_utf8_lossy(&result_buffer[..end]);

    // Try to parse as JSON
    serde_json::from_str(&json_str)
        .map_err(|e| format!("JSON parse error: {}", e))
}

/// Log a message from WASM
pub fn log(level: &str, message: &str) {
    unsafe {
        host_log(
            level.as_ptr(),
            level.len() as i32,
            message.as_ptr(),
            message.len() as i32,
        )
    }
}

/// Store a metric value from WASM
pub fn store_metric(name: &str, value: &serde_json::Value) {
    let value_str = serde_json::to_string(value).unwrap_or_default();
    unsafe {
        host_store_metric(
            name.as_ptr(),
            name.len() as i32,
            value_str.as_ptr(),
            value_str.len() as i32,
        )
    }
}

/// Read a device metric from WASM
pub fn device_read(device_id: &str, metric: &str) -> Result<serde_json::Value, String> {
    let mut result_buffer = vec![0u8; 65536];

    let result_len = unsafe {
        host_device_read(
            device_id.as_ptr(),
            device_id.len() as i32,
            metric.as_ptr(),
            metric.len() as i32,
            result_buffer.as_mut_ptr(),
            result_buffer.len() as i32,
        )
    };

    if result_len < 0 {
        return Err("Device read failed".to_string());
    }

    let end = result_buffer
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(result_len as usize);
    let json_str = String::from_utf8_lossy(&result_buffer[..end]);

    serde_json::from_str(&json_str).map_err(|e| format!("JSON parse error: {}", e))
}

/// Write to a device from WASM
pub fn device_write(
    device_id: &str,
    command: &str,
    params: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let mut result_buffer = vec![0u8; 65536];
    let params_str = serde_json::to_string(params).unwrap_or_default();

    let result_len = unsafe {
        host_device_write(
            device_id.as_ptr(),
            device_id.len() as i32,
            command.as_ptr(),
            command.len() as i32,
            params_str.as_ptr(),
            params_str.len() as i32,
            result_buffer.as_mut_ptr(),
            result_buffer.len() as i32,
        )
    };

    if result_len < 0 {
        return Err("Device write failed".to_string());
    }

    let end = result_buffer
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(result_len as usize);
    let json_str = String::from_utf8_lossy(&result_buffer[..end]);

    serde_json::from_str(&json_str).map_err(|e| format!("JSON parse error: {}", e))
}
