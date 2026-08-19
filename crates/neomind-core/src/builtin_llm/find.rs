use std::path::PathBuf;

fn runner_name() -> &'static str {
    if cfg!(windows) {
        "neomind-llama-server.exe"
    } else {
        "neomind-llama-server"
    }
}

pub fn find_llama_server() -> Result<PathBuf, String> {
    let name = runner_name();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join(name);
            if candidate.exists() {
                return Ok(candidate);
            }
        }
    }
    if let Ok(path_var) = std::env::var("PATH") {
        let sep = if cfg!(windows) { ";" } else { ":" };
        for p in path_var.split(sep) {
            let candidate = std::path::Path::new(p).join(name);
            if candidate.exists() {
                return Ok(candidate);
            }
        }
    }
    Err(format!(
        "{} not found in executable directory or PATH",
        name
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;

    fn bin_name() -> &'static str {
        if cfg!(windows) {
            "neomind-llama-server.exe"
        } else {
            "neomind-llama-server"
        }
    }

    #[test]
    fn finds_in_path() {
        static TMP: OnceLock<std::path::PathBuf> = OnceLock::new();
        let dir = TMP.get_or_init(|| {
            let d = std::env::temp_dir().join(format!("builtin-find-{}", std::process::id()));
            std::fs::create_dir_all(&d).unwrap();
            d
        });
        let fake = dir.join(bin_name());
        std::fs::write(&fake, "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let old = std::env::var("PATH").ok();
        std::env::set_var("PATH", dir);
        let found = find_llama_server();
        if let Some(o) = old {
            std::env::set_var("PATH", o);
        } else {
            std::env::remove_var("PATH");
        }
        assert_eq!(
            found.unwrap().file_name().unwrap().to_string_lossy(),
            bin_name()
        );
    }

    #[test]
    fn missing_returns_error() {
        let old = std::env::var("PATH").ok();
        std::env::set_var("PATH", "/nonexistent-dir-for-builtin-find");
        let found = find_llama_server();
        if let Some(o) = old {
            std::env::set_var("PATH", o);
        } else {
            std::env::remove_var("PATH");
        }
        assert!(found.is_err());
    }
}
