#!/usr/bin/env node

/**
 * Version Synchronization Script
 *
 * Syncs version from Cargo.toml (workspace root) to:
 * - web/package.json
 * - web/src-tauri/tauri.conf.json
 * - web/src-tauri/Cargo.toml
 *
 * Usage: node scripts/sync-version.js [--dry-run]
 */

const fs = require('fs');
const path = require('path');

// Parse command line arguments
const args = process.argv.slice(2);
const dryRun = args.includes('--dry-run');

// Get project root (assumes script is in scripts/ at project root)
const projectRoot = path.join(__dirname, '..');
const cargoTomlPath = path.join(projectRoot, 'Cargo.toml');
const packageJsonPath = path.join(projectRoot, 'web', 'package.json');
const tauriConfigPath = path.join(projectRoot, 'web', 'src-tauri', 'tauri.conf.json');
const tauriCargoPath = path.join(projectRoot, 'web', 'src-tauri', 'Cargo.toml');

/**
 * Parse version from Cargo.toml
 * Handles both [workspace.package] and [package] sections
 */
function parseCargoVersion(tomlContent) {
  // Try workspace.package.version first
  const workspaceMatch = tomlContent.match(/\[workspace\.package\][\s\S]*?version\s*=\s*"([^"]+)"/);
  if (workspaceMatch) {
    return workspaceMatch[1];
  }

  // Fall back to [package] version
  const packageMatch = tomlContent.match(/\[package\][\s\S]*?version\s*=\s*"([^"]+)"/);
  if (packageMatch) {
    return packageMatch[1];
  }

  throw new Error('Could not find version in Cargo.toml');
}

/**
 * Update version in package.json
 */
function updatePackageJson(version, filePath) {
  const content = fs.readFileSync(filePath, 'utf8');
  const pkg = JSON.parse(content);

  if (pkg.version === version) {
    console.log(`✓ ${path.relative(projectRoot, filePath)}: already at v${version}`);
    return false;
  }

  if (dryRun) {
    console.log(`[DRY RUN] Would update ${path.relative(projectRoot, filePath)}: v${pkg.version} → v${version}`);
  } else {
    pkg.version = version;
    fs.writeFileSync(filePath, JSON.stringify(pkg, null, 2) + '\n');
    console.log(`✓ Updated ${path.relative(projectRoot, filePath)}: v${pkg.version} → v${version}`);
  }
  return true;
}

/**
 * Update version in tauri.conf.json
 */
function updateTauriConfig(version, filePath) {
  const content = fs.readFileSync(filePath, 'utf8');
  const config = JSON.parse(content);

  if (config.version === version) {
    console.log(`✓ ${path.relative(projectRoot, filePath)}: already at v${version}`);
    return false;
  }

  if (dryRun) {
    console.log(`[DRY RUN] Would update ${path.relative(projectRoot, filePath)}: v${config.version} → v${version}`);
  } else {
    config.version = version;
    fs.writeFileSync(filePath, JSON.stringify(config, null, 2) + '\n');
    console.log(`✓ Updated ${path.relative(projectRoot, filePath)}: v${config.version} → v${version}`);
  }
  return true;
}

/**
 * Update version in Cargo.toml
 */
function updateCargoToml(version, filePath) {
  const content = fs.readFileSync(filePath, 'utf8');

  // Check if already at this version
  const currentVersionMatch = content.match(/^version\s*=\s*"([^"]+)"/m);
  const currentVersion = currentVersionMatch ? currentVersionMatch[1] : null;

  if (currentVersion === version) {
    console.log(`✓ ${path.relative(projectRoot, filePath)}: already at v${version}`);
    return false;
  }

  if (dryRun) {
    console.log(`[DRY RUN] Would update ${path.relative(projectRoot, filePath)}: v${currentVersion} → v${version}`);
  } else {
    const updated = content.replace(
      /^(version\s*=\s*)"[^"]+"/m,
      `$1"${version}"`
    );
    fs.writeFileSync(filePath, updated);
    console.log(`✓ Updated ${path.relative(projectRoot, filePath)}: v${currentVersion} → v${version}`);
  }
  return true;
}

/**
 * Main execution
 */
function main() {
  console.log('🔄 Syncing version across project...\n');

  // Read version from workspace Cargo.toml
  const cargoToml = fs.readFileSync(cargoTomlPath, 'utf8');
  const version = parseCargoVersion(cargoToml);
  console.log(`📦 Source version: v${version}\n`);

  // Track if any changes were made
  let changes = false;

  // Update package.json
  if (updatePackageJson(version, packageJsonPath)) {
    changes = true;
  }

  // Update tauri.conf.json
  if (updateTauriConfig(version, tauriConfigPath)) {
    changes = true;
  }

  // Update src-tauri/Cargo.toml
  if (updateCargoToml(version, tauriCargoPath)) {
    changes = true;
  }

  console.log('\n' + (dryRun ? '🚫 Dry run complete - no files modified' : changes ? '✅ Version sync complete' : '✅ All files already in sync'));

  if (changes && !dryRun) {
    console.log('\n⚠️  Don\'t forget to commit the updated files!');
  }

  return 0;
}

// Run if called directly
if (require.main === module) {
  try {
    process.exit(main());
  } catch (error) {
    console.error('❌ Error:', error.message);
    process.exit(1);
  }
}

module.exports = { parseCargoVersion, updatePackageJson, updateTauriConfig, updateCargoToml };
