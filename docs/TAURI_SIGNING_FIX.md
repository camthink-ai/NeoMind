# Tauri v2 签名密钥配置修复

## 🚨 问题

CI/CD 构建失败，错误信息：
```
A public key has been found, but no private key.
Make sure to set `TAURI_SIGNING_PRIVATE_KEY` environment variable.
```

---

## 🔍 原因

**Tauri v2 环境变量名称变更：**

| Tauri v1 | Tauri v2 | 说明 |
|----------|----------|------|
| `TAURI_PRIVATE_KEY` | `TAURI_SIGNING_PRIVATE_KEY` | 私钥 |
| `TAURI_KEY_PASSWORD` | `TAURI_SIGNING_KEY_PASSWORD` | 私钥密码 |

---

## ✅ 解决方案

### 方案 1: 更新 GitHub Secrets（推荐）

#### 步骤 1: 确认现有 Secrets

访问：https://github.com/mingmingshen/NeoMind/settings/secrets/actions

检查是否有：
- `TAURI_PRIVATE_KEY` ❌（旧名称）
- `TAURI_KEY_PASSWORD` ❌（旧名称）

#### 步骤 2: 创建新的 Secrets

在 GitHub Secrets 中添加：

**`TAURI_SIGNING_PRIVATE_KEY`**
```
私钥内容（从您的密钥对中获取）
```

**`TAURI_SIGNING_KEY_PASSWORD`**（如果设置了密码）
```
私钥密码
```

#### 步骤 3: （可选）删除旧 Secrets

删除旧的 secrets：
- `TAURI_PRIVATE_KEY`
- `TAURI_KEY_PASSWORD`

---

### 方案 2: 暂时禁用签名（快速测试）

如果暂时不需要签名，可以禁用 updater：

#### 选项 A: 修改 tauri.conf.json

```json
{
  "bundle": {
    "updater": {
      "active": false,  // 暂时禁用
      "pubkey": "..."
    }
  }
}
```

#### 选项 B: 移除 pubkey

```json
{
  "bundle": {
    "updater": {
      "active": true,
      // 移除 pubkey，这样就不会签名
    }
  }
}
```

**注意：** 禁用签名后，自动更新功能将无法验证更新包的安全性。

---

### 方案 3: 生成新的密钥对

如果还没有密钥对，可以生成新的：

```bash
# 安装 Tauri CLI
cargo install tauri-cli --version "^2.0.0"

# 生成密钥对
tauri signer generate

# 输出：
# Public key: dW50cnVzdGVk...
# Private key: dW50cnVzdGVk...
# Key password: (可选)

# 将私钥保存到 GitHub Secrets: TAURI_SIGNING_PRIVATE_KEY
# 将公钥更新到 tauri.conf.json: updater.pubkey
```

---

## 📝 已完成的修复

### ✅ 更新了 build.yml

```yaml
# 之前（Tauri v1）
env:
  TAURI_PRIVATE_KEY: ${{ secrets.TAURI_PRIVATE_KEY }}
  TAURI_KEY_PASSWORD: ${{ secrets.TAURI_KEY_PASSWORD }}

# 之后（Tauri v2）
env:
  TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
  TAURI_SIGNING_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_KEY_PASSWORD }}
```

### ⚠️ 需要您操作：配置 GitHub Secrets

**必须配置以下 secrets：**

1. **`TAURI_SIGNING_PRIVATE_KEY`**
   - 访问：https://github.com/mingmingshen/NeoMind/settings/secrets/actions
   - 点击 "New repository secret"
   - Name: `TAURI_SIGNING_PRIVATE_KEY`
   - Value: 您的私钥内容

2. **`TAURI_SIGNING_KEY_PASSWORD`**（如果私钥有密码）
   - Name: `TAURI_SIGNING_KEY_PASSWORD`
   - Value: 私钥密码

---

## 🔑 如何获取私钥

### 方法 1: 从现有密钥对

如果您之前生成过密钥对：

```bash
# 查找私钥文件
ls -la ~/.tauri/
# 或者
ls -la ~/Library/Application\ Support/neomind.tauri/

# 私钥文件通常名为：
# - key.pem
# - private.key
# - <app-name>.key
```

### 方法 2: 重新生成

```bash
# 使用 Tauri CLI 生成
tauri signer generate -w ~/.tauri/mykey.key

# 输出示例：
# ✓ Generated new keypair
#
# Your public key is:
# dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6...
#
# Your private key is stored in: ~/.tauri/mykey.key
# Keep it safe! Don't share it with anyone!
```

**重要：**
- ✅ 将私钥内容保存到 GitHub Secrets
- ✅ 将公钥更新到 `tauri.conf.json` 的 `updater.pubkey`

---

## ⚡ 快速修复（临时）

如果需要立即通过构建，可以暂时禁用签名：<tool_call>Bash<arg_key>command</arg_key><arg_value>cd "/Users/shenmingming/CamThink Project/NeoMind" && cat web/src-tauri/tauri.conf.json | jq '.bundle.updater.active = false' > /tmp/tauri-temp.json && mv /tmp/tauri-temp.json web/src-tauri/tauri.conf.json && echo "✅ 已暂时禁用 updater"