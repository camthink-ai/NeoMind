# 手动触发 CI/CD 构建指南

## 🎯 概述

NeoMind 的 CI/CD 构建现在需要**手动触发**，以节省 GitHub Actions 资源。

## 📋 触发方式

### 方法 1: 通过 GitHub Web 界面（推荐）

1. **打开 GitHub Actions 页面**
   ```
   https://github.com/mingmingshen/NeoMind/actions
   ```

2. **选择 workflow**
   - 在左侧菜单找到 "Build NeoMind (Optimized for Speed)"
   - 点击进入

3. **点击 "Run workflow" 按钮**
   - 位于页面右侧

4. **填写构建参数**
   - **构建原因** (可选): 例如 "test release", "bug fix", "feature test" 等
   - **构建类型** (必选):
     - `test` - 测试构建（默认）
     - `release` - 正式发布构建
     - `debug` - 调试构建

5. **点击绿色 "Run workflow" 按钮**
   - 构建将开始运行

### 方法 2: 通过 GitHub CLI

```bash
# 安装 GitHub CLI (如果还没安装)
# macOS
brew install gh

# 登录
gh auth login

# 触发构建
gh workflow run "Build NeoMind (Optimized for Speed)" \
  -f reason="test build" \
  -f build_type="test"
```

## 🔄 自动触发场景

以下情况仍然**自动触发**构建：

### 1. 创建版本 Tag
```bash
git tag v0.6.0
git push origin v0.6.0
```

### 2. 创建 GitHub Release
在 GitHub Releases 页面创建新版本发布

## 📊 查看构建状态

### 方式 1: Web 界面
```
https://github.com/mingmingshen/NeoMind/actions
```

### 方式 2: GitHub CLI
```bash
# 查看最近的 workflow runs
gh run list --workflow="build.yml"

# 查看特定 run 的详情
gh run view <run-id>

# 实时查看日志
gh run watch
```

### 方式 3: 命令行打开
```bash
# macOS
open "https://github.com/mingmingshen/NeoMind/actions"

# Linux
xdg-open "https://github.com/mingmingshen/NeoMind/actions"
```

## 🎯 构建类型说明

| 构建类型 | 用途 | 优化配置 | 缓存 |
|---------|------|---------|------|
| `test` | 日常测试、功能验证 | ThinLTO + codegen-units=256 | sccache |
| `release` | 正式发布版本 | 完整 LTO + 最大优化 | sccache |
| `debug` | 调试问题 | 无优化，快速编译 | sccache |

## ⏱️ 预期构建时间

| 构建类型 | 首次构建 | 二次构建（缓存命中） |
|---------|---------|-------------------|
| Test | 8-12 分钟 | 2-5 分钟 |
| Release | 15-20 分钟 | 5-8 分钟 |
| Debug | 5-8 分钟 | 1-3 分钟 |

## 🚨 常见问题

### Q: 为什么改为手动触发？
A: 节省 GitHub Actions 资源，避免每次 push 都触发完整构建。

### Q: 我需要测试代码改动怎么办？
A: 可以手动触发 `test` 类型构建，或者在本地使用 `cargo build --profile ci-release` 测试。

### Q: PR 检查怎么办？
A: PR 检查需要单独的 workflow（如果需要的话），当前 workflow 专注于完整构建。

### Q: 如何取消正在运行的构建？
A: 在 GitHub Actions 页面找到正在运行的 workflow，点击 "Cancel run"。

## 📝 最佳实践

1. **开发阶段**: 使用本地构建测试
   ```bash
   cargo build --profile ci-release
   ```

2. **提交前**: 手动触发 `test` 构建

3. **发布版本**: 创建 tag 自动触发 release 构建
   ```bash
   git tag v0.6.0
   git push origin v0.6.0
   ```

4. **调试问题**: 使用 `debug` 构建类型，快速编译

## 🔗 相关链接

- **GitHub Actions**: https://github.com/mingmingshen/NeoMind/actions
- **构建优化文档**: `memory/cicd-optimization.md`
- **快速参考**: `QUICK_REFERENCE.md`

---

**最后更新**: 2025-01-18
**状态**: ✅ 已配置
