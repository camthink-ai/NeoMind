#!/bin/bash
# Tauri v2 密钥生成和配置脚本

set -e

# 颜色
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
RED='\033[0;31m'
BOLD='\033[1m'
NC='\033[0m'

echo -e "${BOLD}${BLUE}╔════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BOLD}${BLUE}║         Tauri v2 密钥生成和配置脚本                          ║${NC}"
echo -e "${BOLD}${BLUE}╚════════════════════════════════════════════════════════════╝${NC}"
echo ""

# 检查 tauri-cli 是否安装
if ! command -v tauri &> /dev/null; then
    echo -e "${YELLOW}⚠️  Tauri CLI 未安装${NC}"
    echo "正在安装 Tauri CLI..."
    cargo install tauri-cli --version "^2.0.0"
    echo -e "${GREEN}✅ Tauri CLI 安装完成${NC}"
fi

# 获取应用名称
APP_NAME="${1:-neomind}"

echo -e "${BLUE}ℹ️  应用名称: ${APP_NAME}${NC}"
echo ""

# 密钥保存路径
KEY_DIR="$HOME/.tauri"
KEY_FILE="$KEY_DIR/${APP_NAME}.key"

# 创建密钥目录
mkdir -p "$KEY_DIR"

echo -e "${BOLD}🔑 生成 Tauri 密钥对...${NC}"
echo ""

# 生成密钥对
if tauri signer generate -w "$KEY_FILE"; then
    echo -e "${GREEN}✅ 密钥对生成成功${NC}"
else
    echo -e "${RED}❌ 密钥生成失败${NC}"
    echo "请确保已安装 Tauri CLI: cargo install tauri-cli"
    exit 1
fi

echo ""
echo -e "${BOLD}📋 密钥信息${NC}"
echo -e "   私钥文件: ${KEY_FILE}"
echo ""

# 显示公钥
echo -e "${BOLD}🔓 公钥 (用于 tauri.conf.json)${NC}"
PUBLIC_KEY=$(tauri signer generate --publish 2>/dev/null || true)

if [ -n "$PUBLIC_KEY" ]; then
    echo "$PUBLIC_KEY" | head -1
else
    # 手动提取公钥
    if command -v openssl &> /dev/null; then
        PUBLIC_KEY=$(openssl rsa -in "$KEY_FILE" -pubout 2>/dev/null | grep -v "PUBLIC" | tr -d '\n')
        echo "$PUBLIC_KEY"
    else
        echo -e "${YELLOW}⚠️  无法自动提取公钥${NC}"
        echo "请运行: tauri signer generate --publish"
    fi
fi

echo ""
echo -e "${BOLD}📝 下一步操作${NC}"
echo ""
echo "1. 添加私钥到 GitHub Secrets:"
echo "   - 访问: https://github.com/mingmingshen/NeoMind/settings/secrets/actions"
echo "   - 创建新的 Secret:"
echo "     Name: TAURI_SIGNING_PRIVATE_KEY"
echo "     Value: $(cat "$KEY_FILE" | tr '\n' ' ' | head -c 50)..."
echo ""
echo "2. 如果私钥有密码，添加:"
echo "     Name: TAURI_SIGNING_KEY_PASSWORD"
echo "     Value: <您的密码>"
echo ""
echo "3. 复制私钥内容:"
echo "   cat $KEY_FILE"
echo ""

# 显示私钥内容（前几行）
echo -e "${BOLD}🔐 私钥预览（前 5 行）:${NC}"
head -5 "$KEY_FILE"
echo "   ..."
echo ""

echo -e "${GREEN}✨ 完成！${NC}"
echo ""
echo "现在可以:"
echo "  1. 复制私钥: cat $KEY_FILE"
echo "  2. 添加到 GitHub Secrets"
echo "  3. 推送代码触发构建"
echo ""
