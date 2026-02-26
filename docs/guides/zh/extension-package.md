# NeoMind 扩展包 (.nep) 标准

## 概述

.nep (NeoMind Extension Package) 是一个标准的 ZIP 压缩包格式，用于分发和安装 NeoMind 扩展。

## 包结构

```
{extension-id}-{version}.nep
├── manifest.json              # 扩展元数据（必需）
├── binaries/                  # 平台相关二进制文件
│   ├── darwin_aarch64/
│   │   └── extension.dylib
│   ├── darwin_x86_64/
│   │   └── extension.dylib
│   ├── linux_amd64/
│   │   └── extension.so
│   ├── windows_amd64/
│   │   └── extension.dll
│   └── wasm/
│       ├── extension.wasm
│       └── extension.json
└── frontend/                  # 前端组件（可选）
    ├── dist/
    │   ├── bundle.js
    │   └── bundle.css
    └── assets/
        └── icons/
```

## manifest.json 格式

```json
{
  "format": "neomind-extension-package",
  "format_version": "1.0",
  "id": "neomind.weather.forecast",
  "name": "天气预报",
  "version": "1.0.0",
  "description": "天气数据扩展",
  "author": "NeoMind 团队",
  "license": "MIT",
  "homepage": "https://github.com/camthink-ai/NeoMind-Extensions",
  "type": "wasm",
  "neomind": {
    "min_version": "0.5.0"
  },
  "binaries": {
    "wasm": "binaries/wasm/extension.wasm"
  },
  "frontend": {
    "components": [
      {
        "type": "weather-card",
        "name": "天气卡片",
        "description": "显示天气数据",
        "category": "visualization",
        "bundle_path": "frontend/dist/bundle.js",
        "export_name": "WeatherCard"
      }
    ]
  },
  "permissions": ["network"]
}
```

## 安装方式

### 方式 1：本地文件安装

```bash
# 1. 构建 .nep 包
cd ~/NeoMind-Extension/extensions/your-extension
../scripts/package.sh

# 2. 安装到 NeoMind
curl -X POST http://localhost:9375/api/extensions/upload \
  -H "Content-Type: application/json" \
  -d '{"file_path": "/path/to/extension.nep"}'
```

### 方式 2：从扩展商店安装

```bash
curl -X POST http://localhost:9375/api/extensions/market/install \
  -H "Content-Type: application/json" \
  -d '{"id": "neomind.weather.forecast"}'
```

## 卸载

```bash
curl -X DELETE http://localhost:9375/api/extensions/{extension-id}/uninstall
```

## 扩展类型

| 类型 | 说明 | 二进制格式 |
|------|------|-----------|
| `native` | 原生扩展 | .dylib/.so/.dll |
| `wasm` | WASM 扩展 | .wasm |
| `frontend-only` | 纯前端扩展 | 无二进制 |

## 平台标识

| 平台 | 标识 |
|------|------|
| Apple Silicon (macOS) | `darwin_aarch64` |
| Intel (macOS) | `darwin_x86_64` |
| Linux (x86_64) | `linux_amd64` |
| Linux (ARM64) | `linux_arm64` |
| Windows (x64) | `windows_amd64` |
| WASM | `wasm` |
