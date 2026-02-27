# NeoMind 扩展包 (.nep) 标准

## 概述

.nep (NeoMind Extension Package) 是一个标准的 ZIP 压缩包格式，用于分发和安装 NeoMind 扩展。

## 包结构

```
{extension-id}-{version}.nep
├── manifest.json              # 扩展元数据（必需）
├── binaries/                  # 平台相关二进制文件
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
└── frontend/                  # 前端组件（可选）
    ├── *-components.umd.cjs
    └── frontend.json
```

## manifest.json 格式

```json
{
  "format": "neomind-extension-package",
  "format_version": "2.0",
  "abi_version": 3,
  "id": "weather-forecast-v2",
  "name": "Weather Forecast",
  "version": "2.0.0",
  "sdk_version": "2.0.0",
  "description": "使用统一 SDK 的天气预报扩展",
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

## 安装方式

### 方式 1：本地文件安装

```bash
# 1. 构建 .nep 包（在 NeoMind-Extension 仓库中）
cd NeoMind-Extension
./build.sh --yes

# 2. 通过 API 安装
curl -X POST http://localhost:9375/api/extensions/upload/file \
  -H "Content-Type: application/octet-stream" \
  --data-binary @dist/weather-forecast-v2-2.0.0.nep
```

### 方式 2：Web UI 安装

1. 打开 NeoMind Web UI
2. 导航到扩展 → 添加扩展 → 文件模式
3. 拖放 .nep 文件

## 卸载

```bash
curl -X DELETE http://localhost:9375/api/extensions/{extension-id}
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

## ABI 版本

当前 ABI 版本：**3**

扩展必须通过 `neomind_extension_abi_version()` FFI 函数返回此版本号。

## 可用的 V2 扩展

| 扩展 ID | 类型 | 描述 |
|---------|------|------|
| `weather-forecast-v2` | Native | 天气预报扩展 |
| `image-analyzer-v2` | Native | 图像分析 (YOLOv8) |
| `yolo-video-v2` | Native | 视频处理 (YOLOv11) |
