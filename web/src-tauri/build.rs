// Tauri build script
use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    tauri_build::build();

    // Setup extension runner for Tauri bundling
    let binaries_dir = PathBuf::from("binaries");
    fs::create_dir_all(&binaries_dir).expect("Failed to create binaries directory");

    // Detect target platform
    let target = env::var("TARGET").unwrap_or_else(|_| String::from("unknown"));
    let profile = env::var("PROFILE").unwrap_or_else(|_| String::from("release"));

    // Get project root (web/src-tauri)
    let project_root = env::var("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));

    // Navigate to workspace root (NeoMind/)
    // web/src-tauri -> web -> NeoMind
    let workspace_root = project_root
        .parent() // web/
        .and_then(|p| p.parent()) // NeoMind/
        .unwrap_or(&project_root);

    // Source runner location
    let source_runner = workspace_root
        .join("target")
        .join(&profile)
        .join("neomind-extension-runner");

    // Tauri expects: binaries/neomind-extension-runner-{target_triple}
    let target_triple = target.as_str();
    let platform_runner = binaries_dir.join(format!("neomind-extension-runner-{}", target_triple));

    // Copy the runner if it exists
    if source_runner.exists() {
        // Remove old file
        let _ = fs::remove_file(&platform_runner);

        // Copy to platform-specific name
        fs::copy(&source_runner, &platform_runner).expect("Failed to copy extension runner");

        // Make executable on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&platform_runner).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&platform_runner, perms).expect("Failed to set permissions");
        }

        println!("cargo:warning=✅ Extension runner ready: {}", target_triple);
    } else {
        println!(
            "cargo:warning=⚠️  Extension runner not found at: {:?}",
            source_runner
        );
        println!("cargo:warning=   Run: cargo build --release -p neomind-extension-runner");
    }
}
