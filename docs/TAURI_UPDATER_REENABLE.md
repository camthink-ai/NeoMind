# 重新启用 Tauri Updater 签名

## 🚨 当前状态

**Updater 已暂时禁用**以解决 CI/CD 构建问题。

```json
{
  "plugins": {
    "updater": {
      "active": false  // ← 暂时禁用
    }
  }
}
```

**影响：**
- ⚠️ 应用可以正常构建和运行
- ❌ 自动更新功能暂时不可用
- ❌ 更新包不会被签名验证

---

## ✅ 重新启用 Updater 的步骤

### 步骤 1: 生成新的 Tauri 密钥对

```bash
# 确保 Tauri CLI 已安装
cargo install tauri-cli --version "^2.0.0"

# 生成密钥对（无密码）
tauri signer generate -w ~/.tauri/neomind.key

# 当提示输入密码时，直接按 Enter（留空 = 无密码）

# 输出示例：
# ✓ Generated new keypair
#
# Your public key is:
# dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6...
#
# Your private key is stored in: ~/.tauri/neomind.key
# Keep it safe! Don't share it with anyone!
```

**⚠️ 重要：**
- ✅ **推荐使用无密码密钥**（直接按 Enter）
- ❌ 不要设置密码（会增加配置复杂度）

---

### 步骤 2: 配置 GitHub Secrets

访问：https://github.com/mingmingshen/NeoMind/settings/secrets/actions

**添加新的 Secret：**

| 名称 | 值 | 说明 |
|------|-----|------|
| `TAURI_SIGNING_PRIVATE_KEY` | 私钥内容 | 必需 |

**复制私钥内容：**

```bash
# macOS
cat ~/.tauri/neomind.key | pbcopy

# Linux
cat ~/.tauri/neomind.key

# 手动复制
cat ~/.tauri/neomind.key
# 然后手动复制全部内容（包括 BEGIN 和 END 行）
```

**⚠️ 注意事项：**
- ✅ 复制完整的密钥文件（包括所有行）
- ✅ 不要添加额外的空格或换行
- ✅ 确保 GitHub Secret 的值完全匹配密钥文件内容

---

### 步骤 3: 更新 tauri.conf.json

#### 选项 A: 如果公钥没变

如果之前的公钥仍然有效：

```bash
cd "/Users/shenmingming/CamThink Project/NeoMind"

# 编辑 tauri.conf.json
nano web/src-tauri/tauri.conf.json

# 找到 updater 配置，将 active 改为 true
{
  "plugins": {
    "updater": {
      "active": true,  // ← 改回 true
      "endpoints": [...],
      "dialog": false,
      "pubkey": "dW50cnVzdGVk..."
    }
  }
}
```

#### 选项 B: 如果公钥变了

如果生成了新的密钥对，公钥会变化：

```bash
# 获取新公钥
tauri signer generate --publish

# 输出示例：
# dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6...

# 更新 tauri.conf.json
nano web/src-tauri/tauri.conf.json

# 替换 pubkey 字段
{
  "plugins": {
    "updater": {
      "active": true,
      "pubkey": "新公钥"  // ← 粘贴新公钥
    }
  }
}
```

---

### 步骤 4: 提交并测试

```bash
# 提交更改
git add web/src-tauri/tauri.conf.json
git commit -m "feat: re-enable Tauri updater with new signing key"
git push origin main

# 观察构建
gh run watch
```

**成功标志：**
- ✅ 构建成功完成
- ✅ 没有 "failed to decode secret key" 错误
- ✅ DMG/APP 包正确签名
- ✅ `app.tar.gz` 生成成功

---

## 🔍 故障排除

### 问题 1: 仍然提示密码错误

**错误：**
```
failed to decode secret key: incorrect updater private key password
```

**原因：**
- GitHub Secret 中的密钥格式不对
- 或者复制时多了空格/换行

**解决：**
```bash
# 重新复制密钥（确保没有多余字符）
cat ~/.tauri/neomind.key

# 或使用 jq 清理格式
cat ~/.tauri/neomind.key | jq -Rr .

# 重新设置 GitHub Secret
# 删除旧的 TAURI_SIGNING_PRIVATE_KEY
# 重新添加，确保粘贴时没有前后空格
```

---

### 问题 2: 密钥格式错误

**错误：**
```
Invalid input for the given encoding
```

**原因：**
- 密钥文件损坏
- Base64 编码问题

**解决：**
```bash
# 重新生成密钥对
rm ~/.tauri/neomind.key
tauri signer generate -w ~/.tauri/neomind.key

# 重新配置 GitHub Secrets
cat ~/.tauri/neomind.key | pbcopy
```

---

### 问题 3: 想使用有密码的密钥

**不推荐**，但如果需要：

#### 1. 生成有密码密钥

```bash
tauri signer generate -w ~/.tauri/neomind.key
# 当提示输入密码时，输入强密码
```

#### 2. 恢复 CI/CD 密码配置

```bash
cd "/Users/shenmingming/CamThink Project/NeoMind"

# 编辑 build.yml
nano .github/workflows/build.yml

# 找到 env 部分，添加密码
env:
  TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
  TAURI_SIGNING_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_KEY_PASSWORD }}  # ← 添加
```

#### 3. 配置 GitHub Secrets

添加第二个 Secret：

| 名称 | 值 |
|------|-----|
| `TAURI_SIGNING_PRIVATE_KEY` | 私钥内容 |
| `TAURI_SIGNING_KEY_PASSWORD` | 密码 |

---

## 📋 完整检查清单

在重新启用 updater 之前，确保：

- [ ] ✅ 已生成新的 Tauri 密钥对
- [ ] ✅ 已配置 `TAURI_SIGNING_PRIVATE_KEY` GitHub Secret
- [ ] ✅ 密钥格式正确（无多余空格/换行）
- [ ] ✅ `tauri.conf.json` 中 `updater.active = true`
- [ ] ✅ `tauri.conf.json` 中 `pubkey` 正确（如果使用了新密钥）
- [ ] ⚠️ **不需要** `TAURI_SIGNING_KEY_PASSWORD`（除非密钥有密码）

---

## 🎯 推荐配置

### 最佳实践：无密码密钥

```bash
# 1. 生成密钥（无密码）
tauri signer generate -w ~/.tauri/neomind.key
# 密码提示时直接按 Enter

# 2. 配置单个 GitHub Secret
TAURI_SIGNING_PRIVATE_KEY = <私钥内容>

# 3. 更新 tauri.conf.json
{
  "plugins": {
    "updater": {
      "active": true,
      "pubkey": "<对应的公钥>"
    }
  }
}
```

**优点：**
- ✅ 配置简单（只需一个 Secret）
- ✅ 不需要管理密码
- ✅ CI/CD 更可靠
- ✅ GitHub Secrets 已加密，安全足够

---

## 📚 相关资源

- **Tauri 签名文档**: https://v2.tauri.app/distribute/sign-updater/
- **密钥生成脚本**: `scripts/setup-tauri-key.sh`
- **密码问题修复**: `docs/TAURI_PASSWORD_FIX.md`
- **环境变量修复**: `docs/TAURI_SIGNING_FIX.md`

---

## 🚀 快速参考

### 生成密钥
```bash
tauri signer generate -w ~/.tauri/neomind.key
```

### 复制私钥
```bash
# macOS
cat ~/.tauri/neomind.key | pbcopy

# Linux
cat ~/.tauri/neomind.key
```

### 获取公钥
```bash
tauri signer generate --publish
```

### 验证构建
```bash
git push
gh run watch
```

---

**文档版本**: 1.0.0
**最后更新**: 2025-03-18
**当前状态**: Updater 已禁用，等待重新配置密钥
