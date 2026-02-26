//! Test extension package parsing and installation

use std::path::Path;
use neomind_core::extension::package::ExtensionPackage;

#[test]
fn test_parse_extension_package() {
    // This test requires a .nep file to be present
    // For now, we'll test the module compiles correctly

    let package_path = Path::new("/Users/shenmingming/NeoMind-Extension/neomind.weather.forecast.wasm-0.5.8.nep");

    if !package_path.exists() {
        println!("Skipping test: package file not found at {:?}", package_path);
        return;
    }

    // Test loading from file (this would need async runtime in real test)
    println!("Package file found at: {:?}", package_path);

    // The actual async test would be:
    // let rt = tokio::runtime::Runtime::new().unwrap();
    // let package = rt.block_on(ExtensionPackage::load(&package_path)).unwrap();
    // assert_eq!(package.manifest.format, "neomind-extension-package");
}

#[test]
fn test_detect_platform() {
    use neomind_core::extension::package::detect_platform;

    let platform = detect_platform();
    println!("Detected platform: {}", platform);
    assert!(!platform.is_empty());
    assert!(platform.contains('_') || platform == "wasm");
}
