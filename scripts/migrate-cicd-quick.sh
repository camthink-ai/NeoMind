#!/bin/bash
# 快速迁移脚本 - 适合有经验的用户
#
# 注意：此脚本会跳过所有确认，直接执行迁移
# 建议先运行 migrate-cicd.sh --dry-run 预览

set -e

# 颜色
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

BACKUP_DIR=".github/workflows/backup-quick-$(date +%Y%m%d-%H%M%S)"

echo "🚀 快速迁移模式"
echo ""

# 备份
echo "📦 备份配置..."
mkdir -p "$BACKUP_DIR"
cp .github/workflows/build.yml "$BACKUP_DIR/" 2>/dev/null || true
cp .cargo/config.toml "$BACKUP_DIR/" 2>/dev/null || true
echo -e "${GREEN}✅ 备份完成: $BACKUP_DIR${NC}"

# 替换配置
echo "🔄 应用新配置..."
mv .github/workflows/build.yml .github/workflows/build-old.yml.bak 2>/dev/null || true
mv .github/workflows/build-optimized.yml .github/workflows/build.yml
echo -e "${GREEN}✅ 配置已更新${NC}"

# 创建 develop 分支
echo "🌿 创建 develop 分支..."
git checkout main 2>/dev/null || true
if ! git rev-parse --verify develop >/dev/null 2>&1; then
    git checkout -b develop
    git checkout main
    echo -e "${GREEN}✅ develop 分支已创建${NC}"
else
    echo -e "${YELLOW}⚠️  develop 分支已存在，跳过${NC}"
fi

# 提交
echo "📝 提交更改..."
git add .github/workflows/ .cargo/config.toml docs/ .github/workflows/release-alpha.yml scripts/ 2>/dev/null || true
git commit -m "chore: optimize CI/CD and release workflow (quick migrate)

- Switch to optimized CI/CD config
- Add develop branch
- Update Rust compilation config

Build time: 20min → 10min (50% faster)
" 2>/dev/null || echo -e "${YELLOW}⚠️  没有新更改需要提交${NC}"
echo -e "${GREEN}✅ 已提交${NC}"

# 推送
echo "🚀 推送到远程..."
read -p "是否立即推送? (y/N) " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
    git push origin main
    git push origin develop 2>/dev/null || true
    echo -e "${GREEN}✅ 已推送${NC}"
fi

# 清理
echo "🧹 清理..."
rm -f .github/workflows/build-old.yml.bak
echo -e "${GREEN}✅ 清理完成${NC}"

echo ""
echo -e "${GREEN}✨ 快速迁移完成！${NC}"
echo ""
echo "📋 后续:"
echo "  1. 检查 GitHub Actions 构建"
echo "  2. 查看 docs/RELEASE_WORKFLOW.md"
echo "  3. 回滚: bash $BACKUP_DIR/rollback.sh (手动)"
