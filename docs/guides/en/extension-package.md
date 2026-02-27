# NeoMind Extension Package (.nep) Standard

## Overview

.nep (NeoMind Extension Package) is a standard ZIP archive format for distributing and installing NeoMind extensions.

## Package Structure

```
{extension-id}-{version}.nep
├── manifest.json              # Extension metadata (required)
├── binaries/                  # Platform-specific binaries
│   ├── darwin_aarch64/
│   │   └── libneomind_extension_*.dylib
│   ├── darwin_x86_64/
│   │   └── libneomind_extension_*.dylib
│   ├── linux_amd64/
│   │   └── libneomind_extension_*.so
│   ├── windows_amd64/
│   │   └── neomind_extension_*.dll
│   └── wasm/
│       ├── extension.wasm
│       └── extension.json
└── frontend/                  # Frontend components (optional)
    ├── *-components.umd.cjs
    └── frontend.json
```

## manifest.json Format

```json
{
  "format": "neomind-extension-package",
  "format_version": "2.0",
  "abi_version": 3,
  "id": "weather-forecast-v2",
  "name": "Weather Forecast",
  "version": "2.0.0",
  "sdk_version": "2.0.0",
  "description": "Weather data extension using unified SDK",
  "author": "NeoMind Team",
  "license": "Apache-2.0",
  "type": "native",
  "binaries": {
    "darwin_aarch64": "binaries/darwin_aarch64/libneomind_extension_weather_forecast_v2.dylib"
  },
  "frontend": "frontend/",
  "permissions": [],
  "config_parameters": [],
  "metrics": [],
  "commands": []
}
```

## Installation

### Method 1: Local File Installation

```bash
# 1. Build .nep package (in NeoMind-Extension repository)
cd NeoMind-Extension
./build.sh --yes

# 2. Install via API
curl -X POST http://localhost:9375/api/extensions/upload/file \
  -H "Content-Type: application/octet-stream" \
  --data-binary @dist/weather-forecast-v2-2.0.0.nep
```

### Method 2: Web UI Installation

1. Open NeoMind Web UI
2. Navigate to Extensions → Add Extension → File Mode
3. Drag and drop the .nep file

## Uninstall

```bash
curl -X DELETE http://localhost:9375/api/extensions/{extension-id}
```

## Extension Types

| Type | Description | Binary Format |
|------|-------------|---------------|
| `native` | Native extension | .dylib/.so/.dll |
| `wasm` | WASM extension | .wasm |
| `frontend-only` | Frontend-only extension | No binary |

## Platform Identifiers

| Platform | Identifier |
|----------|------------|
| Apple Silicon (macOS) | `darwin_aarch64` |
| Intel (macOS) | `darwin_x86_64` |
| Linux (x86_64) | `linux_amd64` |
| Linux (ARM64) | `linux_arm64` |
| Windows (x64) | `windows_amd64` |
| WASM | `wasm` |

## ABI Version

Current ABI version: **3**

Extensions must return this version via `neomind_extension_abi_version()` FFI function.

## Available V2 Extensions

| Extension ID | Type | Description |
|--------------|------|-------------|
| `weather-forecast-v2` | Native | Weather forecast extension |
| `image-analyzer-v2` | Native | Image analysis with YOLOv8 |
| `yolo-video-v2` | Native | Video processing with YOLOv11 |
