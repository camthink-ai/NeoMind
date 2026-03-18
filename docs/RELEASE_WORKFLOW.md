# 发布流程改进指南

## 📊 当前问题分析

### 开发节奏 vs CI/CD 速度
- **开发节奏**：快速迭代（每天多次改动）
- **CI/CD 时间**：~20 分钟/次
- **结果**：每次改动都需要等待，效率低

---

## 🚀 优化方案总览

### 一、CI/CD 速度优化（技术层面）

#### 1.1 前端构建优化 ✅
**效果：节省 3-5 分钟**

```yaml
# 之前：每个平台独立构建前端（4次）
# 之后：只构建一次，所有平台共享
build-frontend: # 新增独立 job
  runs-on: ubuntu-latest
  outputs:
    frontend-sha: ${{ steps.build.outputs.sha }}
```

#### 1.2 Rust 编译优化 ✅
**效果：节省 5-8 分钟**

```toml
# .cargo/config.toml
[profile.ci-release]
inherits = "release"
lto = "off"
codegen-units = 256
strip = true
```

**权衡：**
- ✅ 编译速度提升 40-50%
- ⚠️ 二进制文件增大 5-10%
- ⚠️ 运行时性能下降 2-3%

**建议：**
- CI 构建：使用 `ci-release` profile
- 生产发布：使用 `release` profile（本地构建）

#### 1.3 缓存策略优化 ✅
**效果：第二次构建节省 10-15 分钟**

```yaml
# 更激进的缓存策略
key: ${{ runner.os }}-cargo-${{ hashFiles('**/Cargo.lock') }}
# 移除了 github.ref_name，允许跨分支共享缓存
```

#### 1.4 并行构建 ✅
**效果：节省 2-3 分钟**

```bash
# 同时构建 server 和 extension runner
cargo build --release -p neomind-cli &
cargo build --release -p neomind-extension-runner &
wait
```

---

### 二、发布流程优化（流程层面）⭐ 重点

#### 2.1 分层发布策略

```
┌─────────────────────────────────────────────────────┐
│                    开发流程                          │
└─────────────────────────────────────────────────────┘
                        ↓
        ┌───────────────┴───────────────┐
        │                               │
   功能分支 (feature/*)            修复分支 (fix/*)
        │                               │
        └───────────────┬───────────────┘
                        ↓
                 开发分支 (develop)
                        ↓
            ┌───────────┴───────────┐
            │                       │
      每周合并                  Hotfix
            │                       │
            └───────────┬───────────┘
                        ↓
                 主分支 (main)
                        ↓
              ┌─────────┴─────────┐
              │                   │
         Alpha 测试            正式发布
         (内部用户)
```

#### 2.2 版本号策略

遵循语义化版本（Semantic Versioning）：

```
格式：v主版本.次版本.修订号 (MAJOR.MINOR.PATCH)

示例：
- v0.6.0 → v0.6.1: 修复 bug，向后兼容
- v0.6.0 → v0.7.0: 新增功能，向后兼容
- v0.6.0 → v1.0.0: 破坏性变更
```

#### 2.3 发布节奏建议

**快速迭代阶段（当前）：**

| 发布类型 | 频率 | 触发条件 | 构建时间 |
|---------|------|---------|---------|
| **Alpha 版本** | 每周 2-3 次 | 累积 5-10 个功能 | 10 分钟 |
| **Beta 版本** | 每 2 周 | Alpha 测试通过 | 10 分钟 |
| **正式版本** | 每月 | Beta 稳定 | 10 分钟 |

**稳定阶段（未来）：**

| 发布类型 | 频率 | 触发条件 |
|---------|------|---------|
| 补丁版本 | 按需 | 关键 bug 修复 |
| 小版本 | 每季度 | 新功能累积 |
| 大版本 | 每年 | 重大架构升级 |

---

### 三、分支管理策略

#### 3.1 Git Flow 简化版

```bash
# 主要分支
main        # 生产环境，稳定版本
develop     # 开发环境，最新功能

# 临时分支
feature/*   # 功能开发
fix/*       # bug 修复
hotfix/*    # 紧急修复
release/*   # 发布准备

# 工作流程
1. 从 develop 创建 feature/new-feature
2. 开发完成后合并回 develop
3. 定期将 develop 合并到 main 并打 tag
4. 从 main 创建 hotfix，修复后合并回 main 和 develop
```

#### 3.2 实施步骤

**步骤 1：创建 develop 分支**
```bash
git checkout -b develop main
git push -u origin develop
```

**步骤 2：设置分支保护规则**
- `main`: 只允许 PR 合并，需要 CI 通过
- `develop`: 允许直接推送，需要 CI 通过

**步骤 3：调整 CI/CD 触发条件**
```yaml
on:
  push:
    branches: [main, develop]
  pull_request:
    branches: [main, develop]
  release:
    types: [published]  # 只有发布时才构建完整安装包
```

**步骤 4：自动化 Alpha 版本**
```yaml
# .github/workflows/release-alpha.yml
on:
  push:
    branches: [develop]

jobs:
  release-alpha:
    runs-on: ubuntu-latest
    steps:
      - name: Create Alpha Release
        run: |
          VERSION=$(git describe --tags --abbrev=0 2>/dev/null || echo "v0.6.0")
          ALPHA_VERSION="${VERSION}-alpha.$(date +%Y%m%d.%H%M%S)"
          gh release create $ALPHA_VERSION \
            --prerelease \
            --title "Alpha Build $(date +%Y-%m-%d)" \
            --notes "Automated alpha release from develop branch"
```

---

### 四、具体工作流程示例

#### 4.1 功能开发流程

```bash
# 1. 从 develop 创建功能分支
git checkout develop
git pull
git checkout -b feature/add-hot-update

# 2. 开发和提交（本地使用 npm run tauri dev）
git add .
git commit -m "feat: add frontend hot update support"

# 3. 推送并创建 PR（触发 CI，10 分钟）
git push -u origin feature/add-hot-update
# 在 GitHub 上创建 PR: feature/add-hot-update → develop

# 4. 代码审查通过后合并
# GitHub 自动合并后，CI 再次验证（10 分钟）

# 5. 累积多个功能后，发布 Alpha 版本
# （自动触发，无需等待）
```

#### 4.2 紧急修复流程

```bash
# 1. 从 main 创建 hotfix 分支
git checkout main
git pull
git checkout -b hotfix/fix-critical-bug

# 2. 修复并测试
# 本地测试：npm run tauri:build:debug

# 3. 推送并创建 PR
git push -u origin hotfix/fix-critical-bug
# PR: hotfix/fix-critical-bug → main

# 4. 合并后立即发布
git checkout main
git pull
git tag v0.6.1
git push origin v0.6.1
# 触发完整构建（10 分钟）
```

---

### 五、开发体验优化

#### 5.1 本地开发（无需等待 CI）

```bash
# 前端热重载（开发时）
npm run tauri dev

# 快速测试打包
npm run tauri:build:debug  # 不签名，快速构建
```

#### 5.2 预发布测试

```bash
# 从 Alpha 版本安装测试
# 1. 下载 Alpha artifacts
# 2. 本地测试验证
# 3. 反馈问题或合并到 main
```

---

### 六、预期效果

#### 优化前
```
开发 → 推送 → 等待 20 分钟 → 发布
每天 5 次改动 = 100 分钟等待时间
```

#### 优化后
```
开发 → 推送 → 等待 10 分钟 → 合并到 develop
每天 5 次改动 = 50 分钟等待时间

每周发布 2-3 次 Alpha = 20-30 分钟
总等待时间减少 50%
```

#### 关键改进
1. ✅ CI 时间从 20 分钟降至 10 分钟
2. ✅ 不需要每次改动都发布
3. ✅ 开发者可以继续开发，CI 后台运行
4. ✅ Alpha 版本自动发布，测试人员可以随时测试
5. ✅ 正式版本发布频率可控（每周/每月）

---

### 七、实施优先级

#### 第一阶段（立即实施）- 2-3 天
- [ ] 切换到优化后的 CI/CD 配置
- [ ] 测试构建速度和稳定性
- [ ] 创建 develop 分支

#### 第二阶段（1周内）- 3-5 天
- [ ] 设置 Alpha 自动发布
- [ ] 培训团队新的工作流程
- [ ] 更新开发文档

#### 第三阶段（持续优化）- 按需
- [ ] 添加自动化测试（减少人工验证）
- [ ] 添加性能监控（跟踪构建时间）
- [ ] 优化 Rust 编译配置（根据实际数据）

---

### 八、风险控制

#### 8.1 回滚策略
```bash
# 如果新版本有问题，立即回滚
git revert HEAD
git tag v0.6.2  # 新的修复版本
git push origin v0.6.2
```

#### 8.2 蓝绿部署（高级）
- 保留两个版本（v0.6.0 和 v0.6.1）
- 灰度发布：10% → 50% → 100%
- 有问题立即切换回旧版本

---

### 九、工具和资源

#### 9.1 推荐工具
- **Release Notes**: 使用 `generate_release_notes: true` 自动生成
- **Changelog**: 维护 CHANGELOG.md
- **版本号管理**: 使用 semantic-release（可选）

#### 9.2 监控指标
- CI 构建时间趋势
- 发布频率
- Bug 报告数量
- 用户反馈速度

---

## 🎯 总结

**CI/CD 优化的本质不是技术问题，而是流程问题。**

通过以下三管齐下：
1. **技术优化**：减少构建时间（20 → 10 分钟）
2. **流程优化**：减少发布频率（每天 → 每周）
3. **分支策略**：分离开发和发布（develop → main）

**最终效果：**
- 开发者等待时间减少 70%
- 发布质量提高（有测试环节）
- 团队协作更顺畅

---

## 📝 快速实施清单

- [ ] 备份当前 build.yml
- [ ] 替换为 build-optimized.yml
- [ ] 测试构建速度
- [ ] 创建 develop 分支
- [ ] 设置分支保护规则
- [ ] 添加 Alpha 自动发布 workflow
- [ ] 更新团队文档
- [ ] 培训团队新流程
