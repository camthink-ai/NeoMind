//! WASM-specific type definitions
//!
//! These types are used when compiling for WASM target and don't have
//! access to the full neomind-core crate.

use serde::{Deserialize, Serialize};

// Re-export types from extension.rs
pub use crate::extension::*;

/// ABI version for WASM extensions
pub const ABI_VERSION: u32 = 3;
