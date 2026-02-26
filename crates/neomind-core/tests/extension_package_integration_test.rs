//! Integration test for .nep package installation
//!
//! Tests the complete lifecycle: install → register → query → unregister → uninstall

use std::path::Path;

use neomind_core::extension::package::ExtensionPackage;
use neomind_core::extension::registry::ExtensionRegistry;
use neomind_core::extension::ExtensionRegistryTrait;

#[tokio::test]
async fn test_nep_package_lifecycle() {
    // Use a test package path - resolve from workspace root
    // CARGO_MANIFEST_DIR = crates/neomind-core
    // Go up twice to get workspace root, then to extension repo
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir.parent().and_then(|p| p.parent()).unwrap();
    let nep_path = workspace_root.join("../NeoMind-Extension/dist/weather-forecast-0.1.0.nep");

    if !nep_path.exists() {
        println!("Skipping test: .nep package not found at {}", nep_path.display());
        return;
    }

    println!("Testing .nep package: {}", nep_path.display());

    // Test 1: Load package
    println!("\n=== 1. Loading package ===");
    let package = ExtensionPackage::load(&nep_path)
        .await
        .expect("Failed to load package");

    println!("✓ Package loaded");
    println!("  ID: {}", package.manifest.id);
    println!("  Name: {}", package.manifest.name);
    println!("  Version: {}", package.manifest.version);
    println!("  Checksum: {}", package.checksum);
    println!("  Size: {} bytes", package.size);

    // Test 2: Install package
    println!("\n=== 2. Installing package ===");
    let extensions_dir = std::env::temp_dir().join("test_extensions");

    let install_result = package.install(&extensions_dir)
        .await
        .expect("Failed to install package");

    println!("✓ Package installed");
    println!("  Binary path: {:?}", install_result.binary_path);
    println!("  Extension ID: {}", install_result.extension_id);
    println!("  Version: {}", install_result.version);

    // Test 3: Register extension
    println!("\n=== 3. Registering extension ===");
    let registry = ExtensionRegistry::new();

    // First load to get metadata
    let _metadata = registry.load_from_path(&install_result.binary_path)
        .await
        .expect("Failed to load extension for registration");

    println!("✓ Extension registered");
    println!("  Extension ID: {}", install_result.extension_id);

    // Test 4: Query extension
    println!("\n=== 4. Querying extension ===");
    let contains = registry.contains(&install_result.extension_id).await;
    assert!(contains, "Extension not found in registry");

    let extensions = registry.get_extensions().await;
    println!("✓ Extension found in registry");
    println!("  Total extensions: {}", extensions.len());

    // Test 5: Unregister extension
    println!("\n=== 5. Unregistering extension ===");
    registry.unregister(&install_result.extension_id)
        .await
        .expect("Failed to unregister extension");

    println!("✓ Extension unregistered");

    // Verify it's gone
    let contains = registry.contains(&install_result.extension_id).await;
    assert!(!contains, "Extension still in registry after unregister");

    println!("✓ Extension removed from registry");

    // Test 6: Cleanup
    println!("\n=== 6. Cleanup ===");
    let _ = std::fs::remove_file(&install_result.binary_path);

    // Remove frontend directory if exists
    if let Some(frontend_dir) = install_result.frontend_dir {
        if frontend_dir.exists() {
            std::fs::remove_dir_all(frontend_dir)
                .expect("Failed to remove frontend directory");
        }
    }

    println!("✓ Cleanup complete");

    println!("\n=== ✅ All lifecycle tests passed! ===");
}

#[tokio::test]
async fn test_nep_package_lifecycle_native() {
    // Test native extensions (image-analyzer)
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir.parent().and_then(|p| p.parent()).unwrap();
    let nep_path = workspace_root.join("../NeoMind-Extension/dist/image-analyzer-0.1.0.nep");

    if !nep_path.exists() {
        println!("Skipping test: .nep package not found at {}", nep_path.display());
        return;
    }

    println!("Testing .nep package: {}", nep_path.display());

    // Test 1: Load package
    println!("\n=== 1. Loading package ===");
    let package = ExtensionPackage::load(&nep_path)
        .await
        .expect("Failed to load package");

    println!("✓ Package loaded");
    println!("  ID: {}", package.manifest.id);
    println!("  Name: {}", package.manifest.name);
    println!("  Version: {}", package.manifest.version);
    println!("  Checksum: {}", package.checksum);
    println!("  Size: {} bytes", package.size);

    // Test 2: Install package
    println!("\n=== 2. Installing package ===");
    let extensions_dir = std::env::temp_dir().join("test_extensions_native");

    let install_result = package.install(&extensions_dir)
        .await
        .expect("Failed to install package");

    println!("✓ Package installed");
    println!("  Binary path: {:?}", install_result.binary_path);
    println!("  Extension ID: {}", install_result.extension_id);

    // Test 3: Register extension
    println!("\n=== 3. Registering extension ===");
    let registry = ExtensionRegistry::new();

    let _metadata = registry.load_from_path(&install_result.binary_path)
        .await
        .expect("Failed to load extension for registration");

    println!("✓ Extension registered");

    // Test 4: Query extension
    println!("\n=== 4. Querying extension ===");
    let contains = registry.contains(&install_result.extension_id).await;
    assert!(contains, "Extension not found in registry");

    let extensions = registry.get_extensions().await;
    println!("✓ Extension found in registry");
    println!("  Total extensions: {}", extensions.len());

    // Test 5: Unregister extension
    println!("\n=== 5. Unregistering extension ===");
    registry.unregister(&install_result.extension_id)
        .await
        .expect("Failed to unregister extension");

    println!("✓ Extension unregistered");

    // Verify it's gone
    let contains = registry.contains(&install_result.extension_id).await;
    assert!(!contains, "Extension still in registry after unregister");

    println!("✓ Extension removed from registry");

    // Test 6: Cleanup
    println!("\n=== 6. Cleanup ===");
    let _ = std::fs::remove_file(&install_result.binary_path);

    if let Some(frontend_dir) = install_result.frontend_dir {
        if frontend_dir.exists() {
            std::fs::remove_dir_all(frontend_dir)
                .expect("Failed to remove frontend directory");
        }
    }

    println!("✓ Cleanup complete");
    println!("\n=== ✅ Native extension lifecycle tests passed! ===");
}

#[tokio::test]
async fn test_nep_package_yolo_video() {
    // Test yolo-video extension
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir.parent().and_then(|p| p.parent()).unwrap();
    let nep_path = workspace_root.join("../NeoMind-Extension/dist/yolo-video-0.1.0.nep");

    if !nep_path.exists() {
        println!("Skipping test: .nep package not found at {}", nep_path.display());
        return;
    }

    println!("Testing .nep package: {}", nep_path.display());

    // Load package
    let package = ExtensionPackage::load(&nep_path)
        .await
        .expect("Failed to load package");

    println!("✓ Package loaded: {} v{}", package.manifest.name, package.manifest.version);

    // Install package
    let extensions_dir = std::env::temp_dir().join("test_extensions_yolo");
    let install_result = package.install(&extensions_dir)
        .await
        .expect("Failed to install package");

    println!("✓ Package installed: {:?}", install_result.binary_path);

    // Register extension
    let registry = ExtensionRegistry::new();
    let _metadata = registry.load_from_path(&install_result.binary_path)
        .await
        .expect("Failed to load extension for registration");

    println!("✓ Extension registered");

    // Cleanup
    let _ = std::fs::remove_file(&install_result.binary_path);
    if let Some(frontend_dir) = install_result.frontend_dir {
        let _ = std::fs::remove_dir_all(frontend_dir);
    }

    println!("✓ YoloVideo extension lifecycle test passed!");
}
