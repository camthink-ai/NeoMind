// Tauri build script
use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    // Setup extension runner for Tauri bundling.
    //
    // ORDER MATTERS: tauri_build::build() validates tauri.conf.json's
    // externalBin entries and FAILS the build when the sidecar file does
    // not exist — so the runner must be staged BEFORE that call. The
    // original script ran staging after it, which only ever worked
    // because local dev machines had binaries/ populated by earlier
    // workspace builds; a clean CI checkout (the desktop compile-check
    // job) died with "resource path ... doesn't exist".
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

    // Windows binaries carry the .exe suffix
    let exe_suffix = if target.contains("windows") { ".exe" } else { "" };

    // Tauri expects the sidecar at: binaries/neomind-extension-runner-{target_triple}[.exe]
    let platform_runner = binaries_dir
        .join(format!("neomind-extension-runner-{}{}", target, exe_suffix));

    // 1. If CI (or a local dev) already staged the sidecar in binaries/, use it as-is.
    //    fs::copy overwrites, so no need to remove a stale file first.
    if platform_runner.exists() {
        println!("cargo:warning=✅ Extension runner sidecar ready: {}", target);
    } else {
        // 2. Otherwise, copy it from the workspace build output if present.
        let source_runner = workspace_root
            .join("target")
            .join(&profile)
            .join(format!("neomind-extension-runner{}", exe_suffix));

        if source_runner.exists() {
            fs::copy(&source_runner, &platform_runner).expect("Failed to copy extension runner");

            // Make executable on Unix
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = fs::metadata(&platform_runner).unwrap().permissions();
                perms.set_mode(0o755);
                fs::set_permissions(&platform_runner, perms).expect("Failed to set permissions");
            }

            println!("cargo:warning=✅ Extension runner copied: {}", target);
        } else {
            // 3. Neither staged nor pre-built. A real bundle (tauri build)
            //    MUST NOT ship a placeholder — CI's release jobs stage the
            //    real binary first, and a local dev gets told how to build
            //    it. But a plain compile check (`cargo check` on a fresh
            //    checkout, e.g. the CI desktop job) only needs
            //    tauri_build's existence validation to pass. Write a
            //    placeholder so the check compiles, and scream in the
            //    warnings so nobody bundles it by accident. The release
            //    workflow's smoke tests would catch a placeholder instantly.
            println!("cargo:warning=⚠️  Extension runner not found. Looked for one of:");
            println!("cargo:warning=   staged sidecar : {}", platform_runner.display());
            println!("cargo:warning=   workspace build: {}", source_runner.display());
            println!("cargo:warning=   Build it first: cargo build --{} -p neomind-extension-runner", profile);
            println!("cargo:warning=Writing a PLACEHOLDER at {} — compile-check only, NEVER bundle this.", platform_runner.display());
            fs::write(&platform_runner, b"placeholder: compile-check only; run cargo build -p neomind-extension-runner\n")
                .expect("Failed to write placeholder sidecar");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = fs::metadata(&platform_runner).unwrap().permissions();
                perms.set_mode(0o755);
                fs::set_permissions(&platform_runner, perms).expect("Failed to set permissions");
            }
        }
    }

    tauri_build::build();

    // NOTE: neomind-cli is no longer bundled as a Tauri sidecar. The agent's
    // shell tool dispatches data commands in-process via neomind-cli-ops
    // (compiled into this binary), eliminating the need for a separate CLI
    // binary in PATH or bundled as a sidecar. The standalone neomind-cli is
    // still built for server/Docker distributions via CI.
}
