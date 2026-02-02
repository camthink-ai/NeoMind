#!/bin/bash
set -e

echo "Building NeoMind..."
cd "$(dirname "$0")/.."
npm run build

echo "Building Tauri app..."
npm run tauri build || echo "Tauri build had warnings, continuing..."

APP_PATH="src-tauri/target/release/bundle/macos/NeoMind.app"
DMG_PATH="src-tauri/target/release/bundle/NeoMind_0.1.0_aarch64.dmg"

if [ ! -d "$APP_PATH" ]; then
    echo "Error: .app not found at $APP_PATH"
    exit 1
fi

echo "Creating DMG with instructions..."
TEMP_DMG="/tmp/neomind_dmg"
rm -rf "$TEMP_DMG"
mkdir -p "$TEMP_DMG"

cp -R "$APP_PATH" "$TEMP_DMG/"

cat > "$TEMP_DMG/安装说明.txt" <<'EOT'
NeoMind 安装说明

方法1：拖拽安装
1. 将 NeoMind.app 拖拽到"应用程序"文件夹
2. 在"启动台"中右键点击 NeoMind
3. 按住 Option 键，选择"打开"

方法2：如果提示损坏，运行以下命令：
xattr -cr /Applications/NeoMind.app

然后再次打开应用。

注意：此应用未签名，首次打开需要上述步骤。
EOT

hdiutil create -volname "NeoMind" -srcfolder "$TEMP_DMG" -ov -format UDZO "$DMG_PATH" > /dev/null
rm -rf "$TEMP_DMG"

echo "✓ DMG created: $DMG_PATH"
ls -lh "$DMG_PATH"
