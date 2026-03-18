# CI/CD 迁移脚本使用指南

## 📖 概述

`migrate-cicd.sh` 是一个自动化脚本，帮助您安全地将 NeoMind 项目迁移到优化的 CI/CD 配置。

**版本：** 2.0.0
**兼容性：** macOS, Linux, WSL

---

## ✨ 特性

### 安全性
- ✅ 自动备份所有配置文件
- ✅ 错误自动回滚
- ✅ 详细的预检查
- ✅ 干运行模式（Dry-run）

### 易用性
- ✅ 交互式确认
- ✅ 彩色输出和进度显示
- ✅ 详细的日志记录
- ✅ 一键回滚功能

### 功能性
- ✅ 自动创建 develop 分支
- ✅ 智能处理分支冲突
- ✅ Git 提交和推送
- ✅ 临时文件清理

---

## 🚀 快速开始

### 标准模式（推荐）

```bash
# 1. 进入项目目录
cd "/Users/shenmingming/CamThink Project/NeoMind"

# 2. 运行脚本（交互式）
bash scripts/migrate-cicd.sh
```

### 自动模式（跳过所有确认）

```bash
bash scripts/migrate-cicd.sh --yes
```

### 干运行模式（预览更改）

```bash
bash scripts/migrate-cicd.sh --dry-run
```

---

## 📋 命令选项

| 选项 | 长选项 | 说明 |
|------|--------|------|
| `-d` | `--dry-run` | 干运行模式，不实际执行更改 |
| `-y` | `--yes` | 跳过所有确认提示 |
| `-h` | `--help` | 显示帮助信息 |
| `-r` | `--rollback` | 回滚到迁移前的状态 |
| `-v` | `--version` | 显示版本信息 |

### 示例

```bash
# 查看帮助
bash scripts/migrate-cicd.sh --help

# 预览将要做的更改
bash scripts/migrate-cicd.sh --dry-run

# 自动确认所有提示（适合 CI/CD）
bash scripts/migrate-cicd.sh --yes

# 回滚迁移
bash scripts/migrate-cicd.sh --rollback
```

---

## 🔍 工作流程

脚本将按以下步骤执行：

### 步骤 0: 预检查
- ✅ 检查 Git 仓库状态
- ✅ 检查未提交的更改
- ✅ 验证必需文件存在
- ✅ 检查磁盘空间
- ✅ 检查远程仓库连接

### 步骤 1: 备份
- 📦 备份所有配置文件
- 📦 保存当前分支信息
- 📦 生成自动回滚脚本

### 步骤 2: 更改预览
- 👀 显示将要进行的所有更改
- 👀 显示主要差异
- 👀 确认后继续

### 步骤 3: 文件迁移
- 🔄 替换 CI/CD 配置文件
- 🔄 应用优化配置

### 步骤 4: 分支设置
- 🌿 创建 develop 分支（如果不存在）
- 🌿 处理分支冲突

### 步骤 5: Git 提交
- 📝 添加所有更改到 Git
- 📝 创建提交（包含详细说明）

### 步骤 6: 推送到远程
- 🚀 推送 main 分支
- 🚀 推送 develop 分支

### 步骤 7: 清理
- 🧹 删除临时文件
- 🧹 清理备份（可选）

---

## 🔄 回滚

如果迁移出现问题，可以使用以下方法回滚：

### 方法 1: 自动回滚（推荐）

```bash
# 使用自动生成的回滚脚本
bash .github/workflows/backup-YYYYMMDD-HHMMSS/rollback.sh
```

### 方法 2: 使用脚本回滚

```bash
bash scripts/migrate-cicd.sh --rollback
```

### 方法 3: 手动回滚

```bash
# 1. 从备份恢复文件
cp .github/workflows/backup-*/build.yml .github/workflows/

# 2. 恢复 config.toml
cp .github/workflows/backup-*/config.toml .cargo/

# 3. 返回原分支
git checkout main

# 4. 提交恢复的文件
git add .
git commit -m "revert: rollback CI/CD migration"
git push
```

---

## 📊 迁移前后对比

### 优化前

```yaml
build.yml:
  - 前端构建: 每个平台独立（4次）
  - 缓存策略: 保守
  - 构建时间: ~20 分钟
  - 并行度: 低
```

### 优化后

```yaml
build.yml:
  - 前端构建: 独立 job（共享结果）
  - 缓存策略: 激进（跨分支）
  - 构建时间: ~10 分钟（首次），~5 分钟（后续）
  - 并行度: 高（同时构建多个组件）
```

**预期改善：**
- ✅ 构建时间减少 50%
- ✅ 第二次构建减少 75%
- ✅ 开发者等待时间减少 70%

---

## 🔧 故障排除

### 问题 1: 权限被拒绝

```bash
chmod +x scripts/migrate-cicd.sh
```

### 问题 2: 不在 Git 仓库中

```bash
cd /path/to/NeoMind
git status  # 确认在仓库中
```

### 问题 3: 有未提交的更改

```bash
# 提交或暂存更改
git add .
git commit -m "Save work before migration"

# 或者使用 --force 跳过检查（不推荐）
bash scripts/migrate-cicd.sh --yes
```

### 问题 4: 推送失败

```bash
# 手动推送
git push origin main
git push origin develop --force-with-lease
```

### 问题 5: 分支冲突

```bash
# 如果 develop 分支已存在
git branch -D develop  # 删除本地 develop
bash scripts/migrate-cicd.sh  # 重新运行
```

---

## 📝 日志和调试

脚本会生成详细的日志文件：

```bash
migration-YYYYMMDD-HHMMSS.log
```

日志包含：
- ✅ 所有操作的时间戳
- ✅ 错误和警告信息
- ✅ 文件操作记录
- ✅ Git 命令输出

查看日志：

```bash
cat migration-*.log
tail -f migration-*.log  # 实时查看
```

---

## 🎯 最佳实践

### 1. 首次迁移

```bash
# 1. 先干运行预览
bash scripts/migrate-cicd.sh --dry-run

# 2. 检查预览结果
cat migration-*.log

# 3. 确认后执行
bash scripts/migrate-cicd.sh
```

### 2. 团队协作

```bash
# 1. 在测试分支先测试
git checkout -b test/cicd-migration
bash scripts/migrate-cicd.sh

# 2. 验证 CI/CD 正常工作

# 3. 合并到 main
git checkout main
git merge test/cicd-migration
```

### 3. CI/CD 环境迁移

```bash
# 使用自动模式
bash scripts/migrate-cicd.sh --yes --dry-run  # 先预览
bash scripts/migrate-cicd.sh --yes            # 执行
```

---

## 🔗 相关资源

- **发布流程文档:** `docs/RELEASE_WORKFLOW.md`
- **CI/CD 配置:** `.github/workflows/build.yml`
- **Alpha 发布:** `.github/workflows/release-alpha.yml`
- **Rust 配置:** `.cargo/config.toml`

---

## 🆘 获取帮助

如果遇到问题：

1. **查看日志** - `cat migration-*.log`
2. **运行回滚** - `bash scripts/migrate-cicd.sh --rollback`
3. **查看文档** - `docs/RELEASE_WORKFLOW.md`
4. **提交 Issue** - https://github.com/camthink-ai/NeoMind/issues

---

## 📜 许可证

Apache-2.0

---

**最后更新:** 2025-03-18
**维护者:** NeoMind Team
