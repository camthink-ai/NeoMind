#!/bin/bash
# CI/CD 优化迁移脚本 v2.0
#
# 功能：
# - 自动备份和恢复
# - 错误回滚
# - 干运行模式
# - 详细的进度报告
# - 交互式确认

set -o pipefail

# ============================================================================
# 配置
# ============================================================================

SCRIPT_VERSION="2.0.0"
BACKUP_TIMESTAMP=$(date +%Y%m%d-%H%M%S)
BACKUP_DIR=".github/workflows/backup-${BACKUP_TIMESTAMP}"
LOG_FILE="migration-${BACKUP_TIMESTAMP}.log"
DRY_RUN=false
SKIP_CONFIRM=false

# 颜色定义
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m' # No Color

# ============================================================================
# 工具函数
# ============================================================================

# 日志函数
log() {
    echo -e "$@" | tee -a "$LOG_FILE"
}

log_error() {
    echo -e "${RED}❌ ERROR: $*${NC}" | tee -a "$LOG_FILE" >&2
}

log_success() {
    echo -e "${GREEN}✅ $*${NC}" | tee -a "$LOG_FILE"
}

log_warning() {
    echo -e "${YELLOW}⚠️  WARNING: $*${NC}" | tee -a "$LOG_FILE"
}

log_info() {
    echo -e "${CYAN}ℹ️  $*${NC}" | tee -a "$LOG_FILE"
}

log_step() {
    echo -e "\n${BOLD}${BLUE}▶ $*${NC}\n" | tee -a "$LOG_FILE"
}

# 横幅
print_banner() {
    cat << "EOF"
╔════════════════════════════════════════════════════════════╗
║                                                              ║
║   NeoMind CI/CD 优化迁移脚本                                  ║
║                                                              ║
║   版本: 2.0.0                                               ║
║   作者: Claude Code                                         ║
║                                                              ║
╚════════════════════════════════════════════════════════════╝
EOF
}

# 帮助信息
show_help() {
    cat << EOF

用法: $0 [选项]

选项:
    -d, --dry-run       干运行模式（不实际执行更改）
    -y, --yes           跳过所有确认提示
    -h, --help          显示此帮助信息
    -r, --rollback      回滚到迁移前的状态
    -v, --version       显示版本信息

示例:
    $0                  # 交互式迁移
    $0 --dry-run        # 预览将要做的更改
    $0 --yes            # 自动确认所有提示
    $0 --rollback       # 回滚迁移

EOF
}

# ============================================================================
# 预检查函数
# ============================================================================

check_prerequisites() {
    log_step "步骤 0: 预检查"

    local errors=0

    # 检查是否在 git 仓库中
    log_info "检查 Git 仓库..."
    if ! git rev-parse --git-dir > /dev/null 2>&1; then
        log_error "不在 Git 仓库中"
        ((errors++))
    else
        log_success "Git 仓库检查通过"
    fi

    # 检查是否有未提交的更改
    log_info "检查未提交的更改..."
    if ! git diff-index --quiet HEAD -- 2>/dev/null; then
        log_warning "存在未提交的更改"
        git status --short
        ((errors++))

        if [ "$SKIP_CONFIRM" = false ]; then
            read -p "是否继续? (y/N) " -n 1 -r
            echo
            if [[ ! $REPLY =~ ^[Yy]$ ]]; then
                log_info "用户取消操作"
                exit 0
            fi
        fi
    else
        log_success "工作区干净"
    fi

    # 检查必需文件
    log_info "检查必需文件..."
    local required_files=(
        ".github/workflows/build.yml"
        ".github/workflows/build-optimized.yml"
        ".cargo/config.toml"
        "docs/RELEASE_WORKFLOW.md"
    )

    for file in "${required_files[@]}"; do
        if [ ! -f "$file" ]; then
            log_error "缺少必需文件: $file"
            ((errors++))
        fi
    done

    if [ $errors -eq 0 ]; then
        log_success "所有必需文件存在"
    fi

    # 检查磁盘空间（至少 100MB）
    log_info "检查磁盘空间..."
    local available_space=$(df . | tail -1 | awk '{print $4}')
    local required_space=102400  # 100MB in KB

    if [ "$available_space" -lt "$required_space" ]; then
        log_warning "磁盘空间不足（需要至少 100MB）"
        ((errors++))
    else
        log_success "磁盘空间充足"
    fi

    # 检查远程连接
    log_info "检查远程仓库..."
    if git remote get-url origin > /dev/null 2>&1; then
        log_success "远程仓库配置正常"
        log_info "远程 URL: $(git remote get-url origin)"
    else
        log_warning "未找到远程仓库 'origin'"
    fi

    if [ $errors -gt 0 ]; then
        log_error "预检查失败，发现 $errors 个问题"
        if [ "$SKIP_CONFIRM" = false ]; then
            read -p "是否忽略警告继续? (y/N) " -n 1 -r
            echo
            if [[ ! $REPLY =~ ^[Yy]$ ]]; then
                exit 1
            fi
        fi
    fi

    log_success "预检查完成"
}

# ============================================================================
# 备份函数
# ============================================================================

backup_files() {
    log_step "步骤 1: 备份现有配置"

    if [ "$DRY_RUN" = true ]; then
        log_info "[DRY-RUN] 将创建备份目录: $BACKUP_DIR"
        return 0
    fi

    # 创建备份目录
    mkdir -p "$BACKUP_DIR" || {
        log_error "无法创建备份目录"
        return 1
    }

    # 备份文件
    local files_to_backup=(
        ".github/workflows/build.yml"
        ".github/workflows/build.yml"
        ".cargo/config.toml"
    )

    for file in "${files_to_backup[@]}"; do
        if [ -f "$file" ]; then
            cp "$file" "$BACKUP_DIR/" && log_success "已备份: $file" || log_warning "备份失败: $file"
        fi
    done

    # 保存当前分支信息
    git branch --show-current > "$BACKUP_DIR/original_branch.txt"
    git rev-parse HEAD > "$BACKUP_DIR/original_commit.txt"

    # 创建回滚脚本
    cat > "$BACKUP_DIR/rollback.sh" << 'ROLLBACK_EOF'
#!/bin/bash
# 自动生成的回滚脚本

set -e

echo "🔄 开始回滚..."

# 恢复文件
cp .github/workflows/build.yml .github/workflows/build-current.yml
cp build.yml .github/workflows/build.yml

# 恢复 config.toml
if [ -f config.toml ]; then
    cp config.toml ../.cargo/config.toml
fi

# 恢复分支
ORIGINAL_BRANCH=$(cat original_branch.txt 2>/dev/null || echo "main")
git checkout "$ORIGINAL_BRANCH" 2>/dev/null || true

echo "✅ 回滚完成"
echo "ℹ️  请手动提交更改"
ROLLBACK_EOF

    chmod +x "$BACKUP_DIR/rollback.sh"

    log_success "备份完成: $BACKUP_DIR"
    log_info "回滚脚本: $BACKUP_DIR/rollback.sh"
}

# ============================================================================
# 显示更改预览
# ============================================================================

preview_changes() {
    log_step "步骤 2: 更改预览"

    log_info "将进行以下更改："
    echo ""

    echo "📁 文件操作："
    echo "  - 重命名: .github/workflows/build.yml → build-old.yml.bak"
    echo "  - 移动: .github/workflows/build-optimized.yml → build.yml"
    echo "  - 新增: .github/workflows/release-alpha.yml"
    echo "  - 修改: .cargo/config.toml（添加 ci-release profile）"
    echo "  - 新增: docs/RELEASE_WORKFLOW.md"
    echo "  - 新增: scripts/migrate-cicd-optimized.sh"
    echo ""

    echo "🌿 分支操作："
    if git rev-parse --verify develop >/dev/null 2>&1; then
        echo "  - 跳过: develop 分支已存在"
    else
        echo "  - 创建: develop 分支（基于 main）"
    fi
    echo ""

    echo "📝 Git 提交："
    echo "  - 提交信息: \"chore: optimize CI/CD and release workflow\""
    echo "  - 包含文件: 所有 CI/CD 配置和文档"
    echo ""

    # 显示 diff 预览
    if [ -f ".github/workflows/build.yml" ] && [ -f ".github/workflows/build-optimized.yml" ]; then
        echo "📊 主要差异（build.yml）："
        echo "  - 前端构建: 独立 job（所有平台共享）"
        echo "  - 缓存策略: 更激进的跨分支缓存"
        echo "  - 编译优化: 并行构建，优化 Rust profile"
        echo "  - 预期效果: 构建时间减少 40-50%"
        echo ""
    fi

    if [ "$DRY_RUN" = true ]; then
        log_info "[DRY-RUN] 以上是预览，不会实际执行"
        return 0
    fi

    if [ "$SKIP_CONFIRM" = false ]; then
        read -p "是否继续? (y/N) " -n 1 -r
        echo
        if [[ ! $REPLY =~ ^[Yy]$ ]]; then
            log_info "用户取消操作"
            exit 0
        fi
    fi
}

# ============================================================================
# 执行迁移
# ============================================================================

migrate_files() {
    log_step "步骤 3: 执行文件迁移"

    if [ "$DRY_RUN" = true ]; then
        log_info "[DRY-RUN] 将执行文件操作"
        return 0
    fi

    # 重命名旧配置
    if [ -f ".github/workflows/build.yml" ]; then
        mv .github/workflows/build.yml .github/workflows/build-old.yml.bak && \
            log_success "已备份旧配置" || {
            log_error "无法重命名 build.yml"
            return 1
        }
    fi

    # 应用新配置
    if [ -f ".github/workflows/build-optimized.yml" ]; then
        mv .github/workflows/build-optimized.yml .github/workflows/build.yml && \
            log_success "已应用新配置" || {
            log_error "无法重命名 build-optimized.yml"
            return 1
        }
    else
        log_error "找不到 build-optimized.yml"
        return 1
    fi

    log_success "文件迁移完成"
}

setup_develop_branch() {
    log_step "步骤 4: 设置 develop 分支"

    if [ "$DRY_RUN" = true ]; then
        log_info "[DRY-RUN] 将设置 develop 分支"
        return 0
    fi

    # 保存当前分支
    local original_branch=$(git branch --show-current)

    if git rev-parse --verify develop >/dev/null 2>&1; then
        log_warning "develop 分支已存在"

        if [ "$SKIP_CONFIRM" = false ]; then
            read -p "是否重建 develop 分支? (y/N) " -n 1 -r
            echo
            if [[ ! $REPLY =~ ^[Yy]$ ]]; then
                log_info "保留现有 develop 分支"
                git checkout "$original_branch" 2>/dev/null || true
                return 0
            fi
        fi

        # 删除并重建
        git branch -D develop 2>/dev/null || true
        git checkout -b develop && \
            log_success "develop 分支已重建" || {
            log_error "无法创建 develop 分支"
            return 1
        }
    else
        git checkout -b develop && \
            log_success "develop 分支已创建" || {
            log_error "无法创建 develop 分支"
            return 1
        }
    fi

    # 返回原分支
    git checkout "$original_branch" 2>/dev/null || \
        git checkout main || {
        log_warning "无法返回原分支，当前在 develop"
    }
}

commit_changes() {
    log_step "步骤 5: 提交更改"

    if [ "$DRY_RUN" = true ]; then
        log_info "[DRY-RUN] 将提交以下文件："
        echo "  - .github/workflows/build.yml"
        echo "  - .cargo/config.toml"
        echo "  - docs/RELEASE_WORKFLOW.md"
        echo "  - .github/workflows/release-alpha.yml"
        echo "  - scripts/migrate-cicd-optimized.sh"
        return 0
    fi

    # 添加文件到 Git
    local files_added=0
    local files_to_add=(
        ".github/workflows/build.yml"
        ".cargo/config.toml"
        "docs/RELEASE_WORKFLOW.md"
        ".github/workflows/release-alpha.yml"
        "scripts/migrate-cicd-optimized.sh"
    )

    for file in "${files_to_add[@]}"; do
        if [ -f "$file" ]; then
            git add "$file" && ((files_added++)) || log_warning "无法添加: $file"
        fi
    done

    log_success "已添加 $files_added 个文件到暂存区"

    # 提交
    git commit -m "chore: optimize CI/CD and release workflow

- Switch to optimized CI/CD config (faster builds)
- Add develop branch for staged releases
- Add automated alpha releases from develop branch
- Update Rust compilation config for CI

Expected improvements:
- Build time: 20min → 10min (50% faster)
- Release frequency: daily → weekly
- Developer wait time: -70%

Migration performed with migrate-cicd-optimized.sh v${SCRIPT_VERSION}
" && log_success "更改已提交" || {
        log_warning "没有新的更改需要提交"
    }
}

push_changes() {
    log_step "步骤 6: 推送到远程"

    if [ "$DRY_RUN" = true ]; then
        log_info "[DRY-RUN] 将推送分支到远程"
        return 0
    fi

    local push_main=false
    local push_develop=false

    # 检查分支是否存在
    git rev-parse --verify main >/dev/null 2>&1 && push_main=true
    git rev-parse --verify develop >/dev/null 2>&1 && push_develop=true

    if [ "$SKIP_CONFIRM" = false ]; then
        echo "将要推送的分支："
        [ "$push_main" = true ] && echo "  - main"
        [ "$push_develop" = true ] && echo "  - develop"
        echo ""
        read -p "是否立即推送? (y/N) " -n 1 -r
        echo
        if [[ ! $REPLY =~ ^[Yy]$ ]]; then
            log_info "跳过推送，请手动推送"
            return 0
        fi
    fi

    # 推送
    if [ "$push_main" = true ]; then
        git push origin main && log_success "已推送 main 分支" || \
            log_warning "推送 main 分支失败（可能需要手动处理）"
    fi

    if [ "$push_develop" = true ]; then
        git push origin develop && log_success "已推送 develop 分支" || \
            log_warning "推送 develop 分支失败（可能需要手动处理）"
    fi
}

cleanup() {
    log_step "步骤 7: 清理"

    local cleanup_files=(
        ".github/workflows/build-old.yml.bak"
    )

    local should_cleanup=false

    if [ "$DRY_RUN" = true ]; then
        log_info "[DRY-RUN] 将清理临时文件"
        return 0
    fi

    for file in "${cleanup_files[@]}"; do
        if [ -f "$file" ]; then
            log_info "找到临时文件: $file"
            should_cleanup=true
        fi
    done

    if [ "$should_cleanup" = false ]; then
        log_info "没有需要清理的临时文件"
        return 0
    fi

    if [ "$SKIP_CONFIRM" = false ]; then
        read -p "是否删除临时文件? (y/N) " -n 1 -r
        echo
        if [[ ! $REPLY =~ ^[Yy]$ ]]; then
            log_info "保留临时文件"
            return 0
        fi
    fi

    for file in "${cleanup_files[@]}"; do
        if [ -f "$file" ]; then
            rm "$file" && log_success "已删除: $file" || \
                log_warning "删除失败: $file"
        fi
    done

    log_success "清理完成"
}

# ============================================================================
# 回滚函数
# ============================================================================

rollback() {
    log_step "回滚迁移"

    # 查找最新的备份
    local latest_backup=$(ls -td .github/workflows/backup-* 2>/dev/null | head -1)

    if [ -z "$latest_backup" ]; then
        log_error "未找到备份目录"
        exit 1
    fi

    log_info "找到备份: $latest_backup"

    if [ "$SKIP_CONFIRM" = false ]; then
        read -p "是否回滚到此备份? (y/N) " -n 1 -r
        echo
        if [[ ! $REPLY =~ ^[Yy]$ ]]; then
            log_info "取消回滚"
            exit 0
        fi
    fi

    # 恢复文件
    log_info "恢复文件..."

    if [ -f "$latest_backup/build.yml" ]; then
        cp "$latest_backup/build.yml" .github/workflows/build.yml && \
            log_success "已恢复 build.yml"
    fi

    if [ -f "$latest_backup/config.toml" ]; then
        cp "$latest_backup/config.toml" .cargo/config.toml && \
            log_success "已恢复 config.toml"
    fi

    # 恢复分支
    local original_branch=$(cat "$latest_backup/original_branch.txt" 2>/dev/null || echo "main")
    log_info "恢复到分支: $original_branch"
    git checkout "$original_branch" 2>/dev/null || true

    log_success "回滚完成"
    log_info "请手动提交恢复的文件"
}

# ============================================================================
# 完成报告
# ============================================================================

print_summary() {
    echo ""
    log_success "╔════════════════════════════════════════════════════════════╗"
    log_success "║                                                              ║"
    log_success "║                    ✨ 迁移完成！                              ║"
    log_success "║                                                              ║"
    log_success "╚════════════════════════════════════════════════════════════╝"
    echo ""

    cat << EOF
📋 后续步骤:
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

1️⃣  验证 CI/CD
   $ 在 GitHub 查看 Actions 标签页
   $ 观察构建状态和日志
   $ 确认构建时间有所改善

2️⃣  设置分支保护（可选但推荐）
   $ Settings → Branches
   $ 添加规则:
     - main: 需要PR，需要CI通过
     - develop: 需要CI通过

3️⃣  测试 Alpha 发布
   $ 切换到 develop: git checkout develop
   $ 做一些测试改动
   $ 推送并观察自动Alpha发布

4️⃣  团队培训
   $ 向团队介绍新的工作流程
   $ 参考: docs/RELEASE_WORKFLOW.md

🔗 常用命令:
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

  git checkout develop          # 切换到开发分支
  git checkout main             # 切换到主分支
  git merge develop             # 将develop合并到main
  git flow feature start XYZ    # 创建功能分支

📚 文档:
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

  发布流程:       docs/RELEASE_WORKFLOW.md
  CI/CD 配置:     .github/workflows/build.yml
  Alpha 发布:     .github/workflows/release-alpha.yml
  本日志:         $LOG_FILE
  备份位置:       $BACKUP_DIR

💾 回滚:
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

  如果需要回滚，运行:
  $ bash $BACKUP_DIR/rollback.sh

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
EOF
}

# ============================================================================
# 错误处理
# ============================================================================

error_handler() {
    local line_number=$1
    log_error "脚本在第 $line_number 行出错"

    if [ "$DRY_RUN" = false ]; then
        log_warning "请检查日志: $LOG_FILE"
        log_info "可以使用以下命令回滚:"
        log_info "  bash $BACKUP_DIR/rollback.sh"
    fi

    exit 1
}

trap 'error_handler ${LINENO}' ERR

# ============================================================================
# 主函数
# ============================================================================

main() {
    # 解析参数
    while [[ $# -gt 0 ]]; do
        case $1 in
            -d|--dry-run)
                DRY_RUN=true
                shift
                ;;
            -y|--yes)
                SKIP_CONFIRM=true
                shift
                ;;
            -h|--help)
                show_help
                exit 0
                ;;
            -r|--rollback)
                rollback
                exit 0
                ;;
            -v|--version)
                echo "迁移脚本 v${SCRIPT_VERSION}"
                exit 0
                ;;
            *)
                log_error "未知选项: $1"
                show_help
                exit 1
                ;;
        esac
    done

    # 开始执行
    print_banner

    if [ "$DRY_RUN" = true ]; then
        log_warning "🔍 DRY-RUN 模式：不会实际执行更改"
    fi

    # 执行步骤
    check_prerequisites
    backup_files
    preview_changes
    migrate_files
    setup_develop_branch
    commit_changes
    push_changes
    cleanup

    # 完成
    print_summary
}

# 运行主函数
main "$@"
