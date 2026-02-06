#!/bin/bash
# Crate Rename Script for NeoMind Rebranding
#
# Usage: ./scripts/rename_crate.sh <old_name> <new_name>
#
# Example: ./scripts/rename_crate.sh core core
#          (edge-ai-core -> neomind-core)
#
# WARNING: This script modifies files in place. Commit your changes first!

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Check arguments
if [ $# -ne 2 ]; then
    echo -e "${RED}Error: Invalid arguments${NC}"
    echo "Usage: $0 <old_name> <new_name>"
    echo ""
    echo "Example:"
    echo "  $0 core core          # edge-ai-core -> neomind-core"
    echo "  $0 plugin-sdk plugin-sdk  # neomind-plugin-sdk -> neomind-plugin-sdk"
    exit 1
fi

OLD_NAME=$1
NEW_NAME=$2

OLD_CRATE_NAME="edge-ai-${OLD_NAME}"
if [ "$OLD_NAME" = "plugin-sdk" ]; then
    OLD_CRATE_NAME="neomind-plugin-sdk"
fi

NEW_CRATE_NAME="neomind-${NEW_NAME}"

echo -e "${YELLOW}========================================${NC}"
echo -e "${YELLOW}Crate Rename: NeoMind Rebranding${NC}"
echo -e "${YELLOW}========================================${NC}"
echo ""
echo "Old: ${OLD_CRATE_NAME}"
echo "New: ${NEW_CRATE_NAME}"
echo ""
echo -e "${YELLOW}This will modify files in place!${NC}"
read -p "Continue? (y/N) " -n 1 -r
echo
if [[ ! $REPLY =~ ^[Yy]$ ]]; then
    echo "Aborted."
    exit 1
fi

CRATE_PATH="crates/${OLD_NAME}"

# Check if crate exists
if [ ! -d "$CRATE_PATH" ]; then
    echo -e "${RED}Error: Crate directory not found: $CRATE_PATH${NC}"
    exit 1
fi

# Function to replace in files
replace_in_files() {
    local pattern=$1
    local replacement=$2
    local file_pattern=$3

    echo "Replacing: $pattern -> $replacement (in $file_pattern)"

    if [[ "$OSTYPE" == "darwin"* ]]; then
        # macOS
        find . -type f -name "$file_pattern" -exec sed -i '' "s/$pattern/$replacement/g" {} \;
    else
        # Linux
        find . -type f -name "$file_pattern" -exec sed -i "s/$pattern/$replacement/g" {} \;
    fi
}

echo ""
echo -e "${GREEN}Step 1: Renaming crate directory...${NC}"
mv "crates/${OLD_NAME}" "crates/${NEW_NAME}"

echo ""
echo -e "${GREEN}Step 2: Updating crate name in Cargo.toml...${NC}"
replace_in_files "name = \"${OLD_CRATE_NAME}\"" "name = \"${NEW_CRATE_NAME}\"" "Cargo.toml"
replace_in_files "name = \"${OLD_CRATE_NAME}\"" "name = \"${NEW_CRATE_NAME}\" "*/Cargo.toml"

# Handle lib.rs naming for plugin-sdk
if [ "$OLD_NAME" = "plugin-sdk" ]; then
    replace_in_files "name = \"neomind_plugin_sdk\"" "name = \"neomind_plugin_sdk\"" "*/Cargo.toml"
fi

echo ""
echo -e "${GREEN}Step 3: Updating dependency references...${NC}"
replace_in_files "${OLD_CRATE_NAME}" "${NEW_CRATE_NAME}" "Cargo.toml"
replace_in_files "${OLD_CRATE_NAME}" "${NEW_CRATE_NAME}" "*/Cargo.toml"

echo ""
echo -e "${GREEN}Step 4: Updating Rust use statements...${NC}"
replace_in_files "use ${OLD_CRATE_NAME}" "use ${NEW_CRATE_NAME}" "*.rs"
replace_in_files "${OLD_CRATE_NAME}::" "${NEW_CRATE_NAME}::" "*.rs"

# Special handling for edge_ai:: references (core module)
replace_in_files "use edge_ai::" "use neomind::" "*.rs"
replace_in_files "edge_ai::" "neomind::" "*.rs"

echo ""
echo -e "${GREEN}Step 5: Updating documentation...${NC}"
replace_in_files "edge-ai" "neomind" "*.md"
replace_in_files "NeoMind" "NeoMind" "*.md"

echo ""
echo -e "${GREEN}Step 6: Updating config files...${NC}"
replace_in_files "edge_ai" "neomind" "*.toml"
replace_in_files "neomind" "neomind" "*.toml"

echo ""
echo -e "${GREEN}Step 7: Checking for remaining references...${NC}"
REMAINING=$(grep -r "edge_ai\|neomind" --include="*.rs" --include="*.toml" . 2>/dev/null | grep -v "Binary file" | wc -l | tr -d ' ')
echo "Remaining references to edge_ai or neomind: $REMAINING"

echo ""
echo -e "${GREEN}========================================${NC}"
echo -e "${GREEN}Rename complete!${NC}"
echo -e "${GREEN}========================================${NC}"
echo ""
echo "Next steps:"
echo "1. Review changes: git diff"
echo "2. Run tests: cargo test"
echo "3. Commit: git commit -m \"refactor: rename ${OLD_CRATE_NAME} to ${NEW_CRATE_NAME}\""
echo ""
echo -e "${YELLOW}Note: Some manual fixes may be required in comments and documentation.${NC}"
