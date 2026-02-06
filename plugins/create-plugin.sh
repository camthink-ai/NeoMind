#!/bin/bash
# NeoMind Plugin Scaffold Generator
#
# Usage: ./create-plugin.sh <plugin-name> <plugin-type>
#
# Example: ./create-plugin.sh my-greeting-tool tool

set -e

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# 默认值
PLUGIN_NAME=""
PLUGIN_TYPE="tool"
PLUGIN_AUTHOR=""
PLUGIN_DESCRIPTION=""

# 打印帮助信息
show_help() {
    echo "NeoMind Plugin Scaffold Generator"
    echo ""
    echo "Usage: $0 <plugin-name> [plugin-type] [options]"
    echo ""
    echo "Arguments:"
    echo "  plugin-name    Plugin ID (lowercase, hyphens only)"
    echo "  plugin-type     Plugin type (default: tool)"
    echo "                  Types: tool, llm_backend, storage_backend,"
    echo "                         device_adapter, integration, alert_channel,"
    echo "                         rule_engine, workflow_engine"
    echo ""
    echo "Options:"
    echo "  --author NAME    Author name"
    echo "  --desc TEXT      Plugin description"
    echo "  --help           Show this help message"
    echo ""
    echo "Examples:"
    echo "  $0 my-greeting-tool"
    echo "  $0 my-storage-plugin storage_backend --author 'Your Name'"
    echo "  $0 mqtt-bridge device_adapter --desc 'MQTT to NeoMind bridge'"
}

# 解析参数
while [[ $# -gt 0 ]]; do
    case $1 in
        --help|-h)
            show_help
            exit 0
            ;;
        --author)
            PLUGIN_AUTHOR="$2"
            shift 2
            ;;
        --desc)
            PLUGIN_DESCRIPTION="$2"
            shift 2
            ;;
        -*)
            echo -e "${RED}Unknown option: $1${NC}"
            show_help
            exit 1
            ;;
        *)
            if [[ -z "$PLUGIN_NAME" ]]; then
                PLUGIN_NAME="$1"
            elif [[ -z "$PLUGIN_TYPE" ]]; then
                PLUGIN_TYPE="$1"
            else
                echo -e "${RED}Too many arguments${NC}"
                show_help
                exit 1
            fi
            shift
            ;;
    esac
done

# 验证插件名称
if [[ -z "$PLUGIN_NAME" ]]; then
    echo -e "${RED}Error: Plugin name is required${NC}"
    show_help
    exit 1
fi

# 验证插件名称格式（只允许小写字母、数字、连字符）
if [[ ! "$PLUGIN_NAME" =~ ^[a-z0-9-]+$ ]]; then
    echo -e "${RED}Error: Plugin name must contain only lowercase letters, numbers, and hyphens${NC}"
    exit 1
fi

# 验证插件类型
VALID_TYPES=("tool" "llm_backend" "storage_backend" "device_adapter" "integration" "alert_channel" "rule_engine" "workflow_engine")
if [[ ! " ${VALID_TYPES[@]} " =~ " ${PLUGIN_TYPE} " ]]; then
    echo -e "${RED}Error: Invalid plugin type '$PLUGIN_TYPE'${NC}"
    echo "Valid types: ${VALID_TYPES[*]}"
    exit 1
fi

# 设置默认值
PLUGIN_STRUCT_NAME="$(echo "$PLUGIN_NAME" | sed -r 's/(^|-)(\w)/\U\2/g')"
PLUGIN_DISPLAY_NAME="$(echo "$PLUGIN_NAME" | sed 's/-/ /g' | sed -E 's/\b(.)/\u\1/g')"
if [[ -z "$PLUGIN_AUTHOR" ]]; then
    PLUGIN_AUTHOR="$(git config user.name 2>/dev/null || echo "NeoMind Developer")"
fi
if [[ -z "$PLUGIN_DESCRIPTION" ]]; then
    PLUGIN_DESCRIPTION="A $PLUGIN_TYPE plugin for NeoMind"
fi

# 创建插件目录
PLUGIN_DIR="$(dirname "$0")/$PLUGIN_NAME"
if [[ -d "$PLUGIN_DIR" ]]; then
    echo -e "${YELLOW}Warning: Directory '$PLUGIN_DIR' already exists${NC}"
    read -p "Continue and overwrite? [y/N] " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        echo "Aborted"
        exit 0
    fi
    rm -rf "$PLUGIN_DIR"
fi

echo -e "${GREEN}Creating NeoMind plugin: $PLUGIN_NAME${NC}"
echo -e "  Type: $PLUGIN_TYPE"
echo -e "  Struct: $PLUGIN_STRUCT_NAME"
echo -e "  Author: $PLUGIN_AUTHOR"
echo ""

mkdir -p "$PLUGIN_DIR/src"

# 生成 Cargo.toml
cat > "$PLUGIN_DIR/Cargo.toml" << EOF
[package]
name = "$PLUGIN_NAME"
version = "0.1.0"
edition = "2021"
authors = ["$PLUGIN_AUTHOR"]
description = "$PLUGIN_DESCRIPTION"
license = "MIT OR Apache-2.0"

[lib]
crate-type = ["cdylib"]

[dependencies]
edge-ai-core = { path = "../../crates/core" }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

[dev-dependencies]
tokio = { version = "1", features = ["full"] }

[features]
default = []
test-utils = []
EOF

# 生成 build.rs
cat > "$PLUGIN_DIR/build.rs" << EOF
fn main() {
    println!("cargo:rerun-if-changed=src/");
    println!("cargo:rerun-if-changed=Cargo.toml");
}
EOF

# 生成源代码
cat > "$PLUGIN_DIR/src/lib.rs" << EOF
//! $PLUGIN_DISPLAY_NAME for NeoMind
//!
//! $PLUGIN_DESCRIPTION

use neomind-core::plugin::native::{NEOMIND_PLUGIN_API_VERSION, NativePluginDescriptor};
use neomind-core::plugin::{Plugin, PluginMetadata, PluginError};
use serde_json::Value;

/// $PLUGIN_DISPLAY_NAME plugin
pub struct $PLUGIN_STRUCT_NAME {
    initialized: bool,
    config: Value,
}

impl $PLUGIN_STRUCT_NAME {
    pub fn new() -> Self {
        Self {
            initialized: false,
            config: json!({}),
        }
    }

    pub fn initialize(&mut self, config: &Value) -> Result<(), PluginError> {
        self.config = config.clone();
        self.initialized = true;
        Ok(())
    }

    pub fn handle_command(&self, command: &str, args: &Value) -> Result<Value, PluginError> {
        match command {
            "status" => Ok(json!({
                "initialized": self.initialized,
                "config": self.config,
            })),
            _ => Err(PluginError::ExecutionFailed(
                format!("Unknown command: {}", command)
            )),
        }
    }
}

impl Default for $PLUGIN_STRUCT_NAME {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Plugin Trait (optional)
// ============================================================================

impl Plugin for $PLUGIN_STRUCT_NAME {
    fn metadata(&self) -> &PluginMetadata {
        static METADATA: PluginMetadata = {
            let mut meta = PluginMetadata::new(
                "$PLUGIN_NAME",
                "$PLUGIN_DISPLAY_NAME",
                "0.1.0",
                ">=0.1.0",
            );
            meta.description = "$PLUGIN_DESCRIPTION".to_string();
            meta.author = Some("$PLUGIN_AUTHOR".to_string());
            meta.types = vec!["$PLUGIN_TYPE".to_string()];
            meta
        };
        &METADATA
    }

    fn initialize(&mut self, config: &Value) -> neomind-core::plugin::Result<()> {
        Self::initialize(self, config)
    }

    fn is_initialized(&self) -> bool {
        self.initialized
    }
}

// ============================================================================
// FFI Exports
// ============================================================================

#[no_mangle]
pub extern "C" fn neomind_plugin_create() -> *mut $PLUGIN_STRUCT_NAME {
    Box::into_raw(Box::new($PLUGIN_STRUCT_NAME::new()))
}

#[no_mangle]
pub extern "C" fn neomind_plugin_destroy(plugin: *mut $PLUGIN_STRUCT_NAME) {
    if !plugin.is_null() {
        unsafe { let _ = Box::from_raw(plugin); }
    }
}

// ============================================================================
// Plugin Descriptor
// ============================================================================

const PLUGIN_ID: &str = "$PLUGIN_NAME\0";
const PLUGIN_NAME: &str = "$PLUGIN_DISPLAY_NAME\0";
const PLUGIN_VERSION: &str = "0.1.0\0";
const PLUGIN_DESC: &str = "$PLUGIN_DESCRIPTION\0";
const REQUIRED_VERSION: &str = ">=0.1.0\0";

#[no_mangle]
pub static neomind_plugin_descriptor: NativePluginDescriptor = NativePluginDescriptor {
    api_version: NEOMIND_PLUGIN_API_VERSION,
    id: PLUGIN_ID.as_ptr(),
    id_len: PLUGIN_ID.len() - 1,
    name: PLUGIN_NAME.as_ptr(),
    name_len: PLUGIN_NAME.len() - 1,
    version: PLUGIN_VERSION.as_ptr(),
    version_len: PLUGIN_VERSION.len() - 1,
    description: PLUGIN_DESC.as_ptr(),
    description_len: PLUGIN_DESC.len() - 1,
    required_version: REQUIRED_VERSION.as_ptr(),
    required_version_len: REQUIRED_VERSION.len() - 1,
    create: neomind_plugin_create as *const (),
    destroy: neomind_plugin_destroy as *const () -> *const (),
};

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_creation() {
        let plugin = $PLUGIN_STRUCT_NAME::new();
        assert!(!plugin.initialized);
    }

    #[test]
    fn test_initialization() {
        let mut plugin = $PLUGIN_STRUCT_NAME::new();
        assert!(plugin.initialize(&json!({})).is_ok());
        assert!(plugin.initialized);
    }
}
EOF

# 生成 README.md
cat > "$PLUGIN_DIR/README.md" << EOF
# $PLUGIN_DISPLAY_NAME

$PLUGIN_DESCRIPTION

## Building

\`\`\`bash
cargo build --release
\`\`\`

## Testing

\`\`\`bash
cargo test
\`\`\`

## Usage

Load the plugin via NeoMind API:

\`\`\`bash
curl -X POST http://localhost:3000/api/plugins \\\\
  -H "Content-Type: application/json" \\\\
  -d '{
    "path": "/path/to/$PLUGIN_NAME/target/release/lib$(echo $PLUGIN_NAME | sed 's/-/_/g').so",
    "plugin_type": "$PLUGIN_TYPE"
  }'
\`\`\`

## Configuration

The plugin accepts the following configuration:

\`\`\`json
{
  "option1": "value1",
  "option2": 42
}
\`\`\`

## Commands

- \`status\` - Get plugin status

## License

MIT OR Apache-2.0
EOF

# 使脚本可执行
chmod +x "$PLUGIN_DIR/../create-plugin.sh"

echo -e "${GREEN}Plugin scaffold created successfully!${NC}"
echo ""
echo "Next steps:"
echo "  1. cd $PLUGIN_DIR"
echo "  2. Review and modify src/lib.rs"
echo "  3. cargo build"
echo "  4. cargo test"
echo ""
