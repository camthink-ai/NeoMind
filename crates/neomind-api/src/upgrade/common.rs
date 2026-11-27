//! Shared release/semver/download primitives for server self-upgrade.
//!
//! Used by three callers:
//! - the interactive CLI path (`neomind upgrade`) in neomind-cli,
//! - the staged web-triggered path (stage phase, this crate's `service`),
//! - the root apply step (`neomind upgrade --apply-staged` via the systemd
//!   helper unit) in neomind-cli.
//!
//! Pure logic only: no printing, no stdin, no assumption about privileges.

use anyhow::{anyhow, Context, Result};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

/// GitHub repo that hosts releases (server tarballs + web tarball + OTA manifest).
pub const REPO: &str = "camthink-ai/NeoMind";

/// Hardcoded fallbacks mirroring `scripts/install.sh` (the installer's
/// generated unit is the real source of truth; these are the same defaults).
pub const DEFAULT_INSTALL_DIR: &str = "/usr/local/bin";
pub const DEFAULT_WEB_DIR: &str = "/var/www/neomind";

/// Download size cap for release tarballs (server bundle + web bundle).
/// The server tarball is ~100-200 MB; 2 GB leaves generous headroom for
/// bundles that grow (e.g. bundled models).
pub const MAX_RELEASE_DOWNLOAD_SIZE: u64 = 2 * 1024 * 1024 * 1024;

/// Maximum cumulative uncompressed bytes allowed when extracting a release tarball.
pub const MAX_TAR_EXTRACTED_BYTES: u64 = 512 * 1024 * 1024;

/// Maximum number of entries allowed inside a release tarball.
pub const MAX_TAR_ENTRIES: usize = 10_000;

/// Maximum size for a release `checksums.txt` file (1 MiB).
const MAX_CHECKSUMS_SIZE: usize = 1024 * 1024;

/// Name of the systemd helper units that run the privileged apply step.
/// The `.service` does the work as root; the `.path` watches for the trigger
/// file (written by this process) and starts the service — no sudo/polkit,
/// which matters because the API's own unit sets `NoNewPrivileges=true`
/// (sudo from inside it can never gain root).
pub const APPLY_UNIT_NAME: &str = "neomind-upgrade-apply.service";
pub const APPLY_PATH_UNIT_NAME: &str = "neomind-upgrade-apply.path";

/// Trigger file (inside the staging root) whose creation activates
/// [`APPLY_PATH_UNIT_NAME`]. Removed by the apply step first thing so the
/// path unit re-arms for the next upgrade.
pub const APPLY_TRIGGER_FILE: &str = "apply.trigger";

/// Staging manifest written by the stage phase, read by `--apply-staged`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StagingManifest {
    /// Target version (no leading `v`).
    pub version: String,
    /// Host arch the tarball was downloaded for ("amd64" | "arm64").
    pub arch: String,
    /// Epoch millis when staging completed.
    pub staged_at: i64,
    /// Relative path (within the staging dir) of the extracted `neomind` binary.
    pub server_binary: String,
    /// Relative path of the downloaded server tarball (within the staging dir),
    /// used by the privileged apply step to verify release SHA-256 and re-extract
    /// into a root-owned directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_tarball: Option<String>,
    /// Relative path of the extracted `neomind-extension-runner`, if present.
    pub extension_runner: Option<String>,
    /// Relative path of the web tarball, if one was staged.
    pub web_tarball: Option<String>,
}

// ---------------------------------------------------------------------------
// Version parsing / validation / comparison
// ---------------------------------------------------------------------------

/// Strictly validate an upgrade version string and return the canonical version
/// without a leading `'v'`.
///
/// Rules:
/// - Strips at most one leading `'v'`.
/// - Must match `^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$`.
/// - Maximum length 64 characters.
/// - Rejects `/`, `\`, `..`, `?`, `#`, `%`, `\0`, whitespace, and empty strings.
pub fn validate_semver_version(raw: &str) -> std::result::Result<String, String> {
    if raw.is_empty() {
        return Err("version must not be empty".to_string());
    }
    if raw.len() > 64 {
        return Err("version exceeds maximum length of 64 characters".to_string());
    }
    if raw.contains("..")
        || raw
            .chars()
            .any(|c| matches!(c, '/' | '\\' | '?' | '#' | '%' | '\0') || c.is_whitespace())
    {
        return Err(format!("invalid characters in version: {raw:?}"));
    }

    let stripped = raw.strip_prefix('v').unwrap_or(raw);
    if stripped.is_empty() {
        return Err("version must not be empty".to_string());
    }

    let (core, prerelease) = match stripped.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (stripped, None),
    };

    let mut parts = core.split('.');
    let major = parts.next();
    let minor = parts.next();
    let patch = parts.next();
    if parts.next().is_some() {
        return Err(format!("invalid semver version: {raw:?}"));
    }
    let is_digits = |part: Option<&str>| -> bool {
        matches!(part, Some(s) if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
    };
    if !is_digits(major) || !is_digits(minor) || !is_digits(patch) {
        return Err(format!("invalid semver version: {raw:?}"));
    }

    if let Some(pre) = prerelease {
        if pre.is_empty()
            || pre.starts_with('.')
            || pre.ends_with('.')
            || !pre
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        {
            return Err(format!("invalid semver prerelease: {raw:?}"));
        }
    }

    Ok(stripped.to_string())
}

/// Validate a single filename component (no directory separators, no `.` / `..`,
/// no NUL bytes, not empty, not absolute).
pub fn validate_single_filename(name: &str) -> Result<&str> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.contains('\0')
    {
        return Err(anyhow!("invalid staged filename: {name:?}"));
    }
    let p = Path::new(name);
    if p.is_absolute() {
        return Err(anyhow!("staged filename must be relative: {name:?}"));
    }
    let mut comps = p.components();
    match (comps.next(), comps.next()) {
        (Some(Component::Normal(_)), None) => Ok(name),
        _ => Err(anyhow!(
            "staged filename must be a single component: {name:?}"
        )),
    }
}

/// Resolve a staged file inside `staging`, rejecting invalid filenames,
/// symlinks, non-regular files, and any canonical path escaping `staging`.
pub fn resolve_staged_file(staging: &Path, name: &str) -> Result<PathBuf> {
    validate_single_filename(name)?;
    let canonical_staging = staging
        .canonicalize()
        .with_context(|| format!("cannot canonicalize staging dir {}", staging.display()))?;
    let candidate = canonical_staging.join(name);
    let meta = std::fs::symlink_metadata(&candidate)
        .with_context(|| format!("staged file missing: {}", candidate.display()))?;
    if !meta.file_type().is_file() {
        return Err(anyhow!(
            "staged file {} is not a regular file (symlinks and directories are forbidden)",
            candidate.display()
        ));
    }
    let canonical_candidate = candidate
        .canonicalize()
        .with_context(|| format!("cannot canonicalize staged file {}", candidate.display()))?;
    if !canonical_candidate.starts_with(&canonical_staging)
        || canonical_candidate.parent() != Some(canonical_staging.as_path())
    {
        return Err(anyhow!(
            "staged file {} escapes staging directory {}",
            canonical_candidate.display(),
            canonical_staging.display()
        ));
    }
    Ok(canonical_candidate)
}

/// Whether the operator explicitly opted into skipping checksum verification
/// via `NEOMIND_ALLOW_UNVERIFIED_UPGRADE=1`.
pub fn allow_unverified_upgrade() -> bool {
    std::env::var("NEOMIND_ALLOW_UNVERIFIED_UPGRADE")
        .map(|v| v == "1")
        .unwrap_or(false)
}

/// RAII temporary directory with `0700` permissions on Unix.
pub struct UpgradeTempDir(tempfile::TempDir);

impl UpgradeTempDir {
    pub fn path(&self) -> &Path {
        self.0.path()
    }
}

/// Create a private (`0700`) temporary directory for upgrade extraction/staging.
pub fn create_upgrade_tempdir() -> Result<UpgradeTempDir> {
    let dir = tempfile::Builder::new()
        .prefix("neomind-upgrade-")
        .tempdir()
        .context("failed to create upgrade temporary directory")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(UpgradeTempDir(dir))
}

/// Parse the leading `major.minor.patch` ints out of a (possibly dirty)
/// version string. Non-digit chars act as separators; missing parts are 0.
pub fn semver(v: &str) -> [u64; 3] {
    let v = v.trim_start_matches('v');
    let mut parts = v.split(|c: char| !c.is_ascii_digit());
    let major = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let patch = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    [major, minor, patch]
}

/// True when `target` is strictly newer than `current` (pre-release suffixes
/// are ignored, same as the CLI path always did).
pub fn is_newer(target: &str, current: &str) -> bool {
    let t = semver(target);
    let c = semver(current);
    t[0] > c[0] || (t[0] == c[0] && t[1] > c[1]) || (t[0] == c[0] && t[1] == c[1] && t[2] > c[2])
}

// ---------------------------------------------------------------------------
// Release metadata + URLs + checksums
// ---------------------------------------------------------------------------

/// Release asset arch component for the current host.
pub fn host_arch() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "amd64"
    }
}

/// Validate a release arch token (`"amd64"` or `"arm64"`).
pub fn validate_arch(arch: &str) -> Result<&str> {
    match arch {
        "amd64" | "arm64" => Ok(arch),
        _ => Err(anyhow!("unsupported release arch: {arch:?}")),
    }
}

/// Release asset filename for the Linux server tarball.
pub fn server_tarball_filename(arch: &str) -> Result<String> {
    let arch = validate_arch(arch)?;
    Ok(format!("neomind-server-linux-{arch}.tar.gz"))
}

/// `releases/download` URL for the host-arch Linux server tarball.
pub fn server_tarball_url(version: &str, arch: &str) -> Result<String> {
    let clean = validate_semver_version(version).map_err(|e| anyhow!(e))?;
    let asset = server_tarball_filename(arch)?;
    Ok(format!(
        "https://github.com/{REPO}/releases/download/v{clean}/{asset}"
    ))
}

/// `releases/download` URL for the unversioned web-frontend tarball
/// (unversioned on purpose so `releases/latest/download/...` always works).
pub fn web_tarball_url(version: &str) -> Result<String> {
    let clean = validate_semver_version(version).map_err(|e| anyhow!(e))?;
    Ok(format!(
        "https://github.com/{REPO}/releases/download/v{clean}/neomind-web.tar.gz"
    ))
}

/// `releases/download` URL for the release `checksums.txt`.
pub fn checksums_url(version: &str) -> Result<String> {
    let clean = validate_semver_version(version).map_err(|e| anyhow!(e))?;
    Ok(format!(
        "https://github.com/{REPO}/releases/download/v{clean}/checksums.txt"
    ))
}

/// Parse standard `sha256sum` output (`<64-hex>  <filename>` or `<64-hex> *<filename>`).
pub fn parse_checksums_txt(body: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let mut parts = trimmed.splitn(2, char::is_whitespace);
        let Some(hash_raw) = parts.next() else {
            continue;
        };
        let Some(file_raw) = parts.next() else {
            continue;
        };
        let hash = hash_raw.trim().to_ascii_lowercase();
        if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        let file = file_raw
            .trim()
            .trim_start_matches('*')
            .trim_start_matches("./");
        if !file.is_empty() {
            map.insert(file.to_string(), hash);
        }
    }
    map
}

/// Fetch and parse `https://github.com/camthink-ai/NeoMind/releases/download/v{version}/checksums.txt`.
/// Fails if `checksums.txt` is missing (404) or unreachable.
pub async fn fetch_release_checksums(
    client: &reqwest::Client,
    version: &str,
) -> Result<HashMap<String, String>> {
    let url = checksums_url(version)?;
    let resp = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("failed to fetch release checksums: {url}"))?
        .error_for_status()
        .with_context(|| format!("release checksums unavailable: {url}"))?;
    if let Some(len) = resp.content_length() {
        if len as usize > MAX_CHECKSUMS_SIZE {
            return Err(anyhow!("checksums.txt exceeds size limit ({len} bytes)"));
        }
    }
    let bytes = resp
        .bytes()
        .await
        .context("failed to read release checksums body")?;
    if bytes.len() > MAX_CHECKSUMS_SIZE {
        return Err(anyhow!("checksums.txt exceeds size limit"));
    }
    let body = String::from_utf8(bytes.to_vec()).context("checksums.txt is not valid UTF-8")?;
    let map = parse_checksums_txt(&body);
    if map.is_empty() {
        return Err(anyhow!("checksums.txt contained no valid SHA-256 entries"));
    }
    Ok(map)
}

/// Fetch release `checksums.txt`, returning `Ok(None)` when the release does not
/// publish one (HTTP 404). Any other error (network, 5xx, malformed body) is
/// returned as `Err`.
pub async fn fetch_release_checksums_optional(
    client: &reqwest::Client,
    version: &str,
) -> Result<Option<HashMap<String, String>>> {
    let url = checksums_url(version)?;
    let resp = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("failed to fetch release checksums: {url}"))?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let resp = resp
        .error_for_status()
        .with_context(|| format!("release checksums request failed: {url}"))?;
    if let Some(len) = resp.content_length() {
        if len as usize > MAX_CHECKSUMS_SIZE {
            return Err(anyhow!("checksums.txt exceeds size limit ({len} bytes)"));
        }
    }
    let bytes = resp
        .bytes()
        .await
        .context("failed to read release checksums body")?;
    if bytes.len() > MAX_CHECKSUMS_SIZE {
        return Err(anyhow!("checksums.txt exceeds size limit"));
    }
    let body = String::from_utf8(bytes.to_vec()).context("checksums.txt is not valid UTF-8")?;
    let map = parse_checksums_txt(&body);
    if map.is_empty() {
        return Err(anyhow!("checksums.txt contained no valid SHA-256 entries"));
    }
    Ok(Some(map))
}

/// Compute the SHA-256 digest of `path` and verify it matches `expected_hex`.
pub fn verify_file_sha256(path: &Path, expected_hex: &str) -> Result<()> {
    let expected = expected_hex.trim().to_ascii_lowercase();
    if expected.len() != 64 || !expected.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(anyhow!(
            "invalid expected SHA-256 hex digest: {expected_hex:?}"
        ));
    }
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("cannot open {} for SHA-256 verification", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let actual = hex::encode(hasher.finalize());
    if actual != expected {
        return Err(anyhow!(
            "SHA-256 mismatch for {}: expected {}, got {}",
            path.display(),
            expected,
            actual
        ));
    }
    Ok(())
}

/// Resolve the latest release tag + release-notes body from the GitHub API
/// (tag has no leading `v`; body is markdown, may be absent).
pub async fn github_latest_release(client: &reqwest::Client) -> Result<(String, Option<String>)> {
    let resp: serde_json::Value = client
        .get(format!(
            "https://api.github.com/repos/{}/releases/latest",
            REPO
        ))
        .send()
        .await?
        .error_for_status()
        .context("GitHub latest-release lookup failed")?
        .json()
        .await?;
    let tag = resp["tag_name"]
        .as_str()
        .ok_or_else(|| anyhow!("release has no tag_name"))?;
    let clean = validate_semver_version(tag)
        .map_err(|e| anyhow!("GitHub release tag_name is not valid semver: {e}"))?;
    let body = resp["body"].as_str().map(str::to_string);
    Ok((clean, body))
}

/// Resolve the latest release tag from the GitHub API (no leading `v`).
pub async fn github_latest_version(client: &reqwest::Client) -> Result<String> {
    Ok(github_latest_release(client).await?.0)
}

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

/// Web-frontend dir the server serves static files from: `NEOMIND_WEB_DIR`
/// when set (the installer unit exports it), else `/var/www/neomind`.
///
/// Note: the pre-web-upgrade CLI hardcoded the default and ignored the env
/// var; both callers now go through here.
pub fn web_dir() -> PathBuf {
    match std::env::var_os("NEOMIND_WEB_DIR") {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => PathBuf::from(DEFAULT_WEB_DIR),
    }
}

/// Root of the writable staging area (inside the data dir the API can write).
pub fn staging_root(data_dir: &Path) -> PathBuf {
    data_dir.join("upgrade")
}

/// Staging dir for one specific target version (validated semver).
pub fn staging_dir(data_dir: &Path, version: &str) -> Result<PathBuf> {
    let clean = validate_semver_version(version).map_err(|e| anyhow!(e))?;
    Ok(staging_root(data_dir).join(format!("v{clean}")))
}

// ---------------------------------------------------------------------------
// Download + extract + verify
// ---------------------------------------------------------------------------

/// Stream `url` to `dest` (a file path), enforcing
/// [`MAX_RELEASE_DOWNLOAD_SIZE`] via Content-Length and a running counter.
///
/// `on_progress` fires per chunk with `(downloaded, total)`; callers throttle
/// event publishing themselves (the builtin-LLM downloader pattern).
pub async fn download_to_file(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    mut on_progress: Option<&mut (dyn FnMut(u64, Option<u64>) + Send)>,
) -> Result<u64> {
    use futures::StreamExt;
    use tokio::io::AsyncWriteExt;

    let resp = client
        .get(url)
        .send()
        .await?
        .error_for_status()
        .with_context(|| format!("download failed: {url}"))?;

    let declared_total = resp.content_length();
    if let Some(len) = declared_total {
        if len > MAX_RELEASE_DOWNLOAD_SIZE {
            return Err(anyhow!(
                "release asset too large ({len} bytes > {} cap)",
                MAX_RELEASE_DOWNLOAD_SIZE
            ));
        }
    }

    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let mut file = tokio::fs::File::create(dest)
        .await
        .with_context(|| format!("cannot create {}", dest.display()))?;
    let mut downloaded: u64 = 0;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("download stream error")?;
        downloaded += chunk.len() as u64;
        if downloaded > MAX_RELEASE_DOWNLOAD_SIZE {
            let _ = tokio::fs::remove_file(dest).await;
            return Err(anyhow!(
                "release asset exceeded {} cap while streaming",
                MAX_RELEASE_DOWNLOAD_SIZE
            ));
        }
        file.write_all(&chunk).await?;
        if let Some(cb) = on_progress.as_deref_mut() {
            cb(downloaded, declared_total);
        }
    }
    file.flush().await?;
    Ok(downloaded)
}

/// Extract a `.tar.gz` safely into `dest_dir` (created if missing).
///
/// Hardening:
/// - Iterates entries explicitly instead of calling `Archive::unpack`.
/// - Rejects symlinks, hardlinks, character/block devices, FIFOs, and any
///   entry type other than regular files and directories.
/// - Rejects absolute paths and `..` traversal components.
/// - Verifies canonical parent directory containment inside `dest_dir`.
/// - Enforces cumulative extracted byte cap ([`MAX_TAR_EXTRACTED_BYTES`]) and
///   maximum entry count ([`MAX_TAR_ENTRIES`]).
pub fn extract_tar_gz(tarball: &Path, dest_dir: &Path) -> Result<()> {
    use std::io::Write;
    use tar::EntryType;

    std::fs::create_dir_all(dest_dir)?;
    let canonical_dest = dest_dir
        .canonicalize()
        .with_context(|| format!("cannot canonicalize dest dir {}", dest_dir.display()))?;

    let file = std::fs::File::open(tarball)
        .with_context(|| format!("cannot open {}", tarball.display()))?;
    let gz = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(gz);

    let mut entry_count: usize = 0;
    let mut total_bytes: u64 = 0;
    let mut buf = [0u8; 64 * 1024];

    for entry_res in archive
        .entries()
        .with_context(|| format!("cannot read tar entries from {}", tarball.display()))?
    {
        entry_count += 1;
        if entry_count > MAX_TAR_ENTRIES {
            return Err(anyhow!(
                "tar archive {} exceeds maximum entry count ({})",
                tarball.display(),
                MAX_TAR_ENTRIES
            ));
        }

        let mut entry =
            entry_res.with_context(|| format!("corrupt tar entry in {}", tarball.display()))?;
        let entry_type = entry.header().entry_type();

        match entry_type {
            EntryType::Regular | EntryType::Directory => {}
            EntryType::Symlink | EntryType::Link => {
                return Err(anyhow!(
                    "tar archive {} contains forbidden link entry ({:?})",
                    tarball.display(),
                    entry_type
                ));
            }
            EntryType::Char | EntryType::Block | EntryType::Fifo => {
                return Err(anyhow!(
                    "tar archive {} contains forbidden special device/FIFO entry ({:?})",
                    tarball.display(),
                    entry_type
                ));
            }
            other => {
                return Err(anyhow!(
                    "tar archive {} contains unsupported entry type ({:?})",
                    tarball.display(),
                    other
                ));
            }
        }

        let rel_path = entry
            .path()
            .with_context(|| format!("invalid path in tar archive {}", tarball.display()))?
            .into_owned();
        if rel_path.is_absolute() {
            return Err(anyhow!(
                "tar archive {} contains absolute path: {}",
                tarball.display(),
                rel_path.display()
            ));
        }

        let mut clean_rel = PathBuf::new();
        for comp in rel_path.components() {
            match comp {
                Component::Normal(part) => clean_rel.push(part),
                Component::CurDir => {}
                Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                    return Err(anyhow!(
                        "tar archive {} contains disallowed path traversal: {}",
                        tarball.display(),
                        rel_path.display()
                    ));
                }
            }
        }

        if clean_rel.as_os_str().is_empty() {
            if entry_type == EntryType::Directory {
                continue;
            }
            return Err(anyhow!(
                "tar archive {} contains file with empty relative path",
                tarball.display()
            ));
        }

        let out_path = canonical_dest.join(&clean_rel);

        if entry_type == EntryType::Directory {
            std::fs::create_dir_all(&out_path)?;
            let canonical_out = out_path.canonicalize()?;
            if !canonical_out.starts_with(&canonical_dest) {
                return Err(anyhow!(
                    "tar directory {} escapes destination {}",
                    out_path.display(),
                    canonical_dest.display()
                ));
            }
            continue;
        }

        // Regular file
        let declared_size = entry.header().size().unwrap_or(0);
        if total_bytes.saturating_add(declared_size) > MAX_TAR_EXTRACTED_BYTES {
            return Err(anyhow!(
                "tar archive {} exceeds maximum extracted size ({} bytes)",
                tarball.display(),
                MAX_TAR_EXTRACTED_BYTES
            ));
        }

        let parent = out_path
            .parent()
            .ok_or_else(|| anyhow!("tar entry has no parent dir: {}", out_path.display()))?;
        std::fs::create_dir_all(parent)?;
        let canonical_parent = parent.canonicalize()?;
        if !canonical_parent.starts_with(&canonical_dest) {
            return Err(anyhow!(
                "tar entry {} escapes destination {}",
                out_path.display(),
                canonical_dest.display()
            ));
        }

        if let Ok(meta) = std::fs::symlink_metadata(&out_path) {
            if meta.file_type().is_symlink() {
                return Err(anyhow!(
                    "refusing to overwrite symlink during tar extraction: {}",
                    out_path.display()
                ));
            }
        }

        let mode = entry.header().mode().unwrap_or(0o644);
        let mut out_file = std::fs::File::create(&out_path)
            .with_context(|| format!("cannot create extracted file {}", out_path.display()))?;

        loop {
            let n = entry.read(&mut buf)?;
            if n == 0 {
                break;
            }
            total_bytes = total_bytes.saturating_add(n as u64);
            if total_bytes > MAX_TAR_EXTRACTED_BYTES {
                drop(out_file);
                let _ = std::fs::remove_file(&out_path);
                return Err(anyhow!(
                    "tar archive {} exceeded maximum extracted size ({} bytes) while reading",
                    tarball.display(),
                    MAX_TAR_EXTRACTED_BYTES
                ));
            }
            out_file.write_all(&buf[..n])?;
        }
        out_file.flush()?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // Strip setuid/setgid/sticky bits; keep only standard 0755 or 0644.
            let safe_mode = if (mode & 0o111) != 0 { 0o755 } else { 0o644 };
            std::fs::set_permissions(&out_path, std::fs::Permissions::from_mode(safe_mode))?;
        }
        #[cfg(not(unix))]
        let _ = mode;
    }

    Ok(())
}

/// Run `bin --version` and require the output to reference `expected`.
///
/// This is the only content check before swapping binaries in (same policy
/// as the CLI path): it catches wrong-arch, truncated, and foreign binaries.
pub fn verify_binary_version(bin: &Path, expected: &str) -> Result<String> {
    let out = std::process::Command::new(bin)
        .arg("--version")
        .output()
        .with_context(|| format!("could not run the downloaded binary {}", bin.display()))?;
    let vstr = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !vstr.ends_with(expected) && !vstr.contains(&format!(" {expected}")) {
        return Err(anyhow!(
            "downloaded binary version mismatch: got {:?}, expected v{expected} — aborting",
            vstr
        ));
    }
    Ok(vstr)
}

// ---------------------------------------------------------------------------
// Apply-phase helpers (run as root via the systemd helper unit)
// ---------------------------------------------------------------------------

/// Run a command, optionally prefixing with sudo; error on non-zero exit.
/// Sync on purpose: only called from the CLI (interactive or root apply),
/// never from async API handlers.
pub fn run_shell(sudo: Option<&str>, args: &[&str]) -> Result<()> {
    let mut cmd = match sudo {
        Some(s) => {
            let mut c = std::process::Command::new(s);
            c.args(args);
            c
        }
        None => {
            let (first, rest) = args.split_first().ok_or_else(|| anyhow!("empty command"))?;
            let mut c = std::process::Command::new(first);
            c.args(rest);
            c
        }
    };
    let status = cmd
        .status()
        .with_context(|| format!("failed to run: {args:?}"))?;
    if !status.success() {
        return Err(anyhow!("command failed ({args:?}): {status}"));
    }
    Ok(())
}

/// `None` when already root, else `Some("sudo")` when sudo exists.
pub fn sudo_prefix() -> Option<String> {
    let is_root = std::process::Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim()
                .parse::<u32>()
                .ok()
        })
        .map(|u| u == 0)
        .unwrap_or(false);
    if is_root {
        None
    } else if std::process::Command::new("which")
        .arg("sudo")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        Some("sudo".to_string())
    } else {
        eprintln!(
            "⚠️ this operation needs root (run as root or with sudo); continuing best-effort"
        );
        None
    }
}

/// Result of a successful apply: where the rollback backups landed.
pub struct ApplyOutcome {
    pub backup_binary: PathBuf,
    pub backup_runner: Option<PathBuf>,
}

/// Back up + atomically install the staged binaries (step shared by the CLI
/// interactive path and `--apply-staged`). `sudo` may be `None` when root.
///
/// Safety: `install -m 755` replaces atomically; the running process keeps
/// executing the old inode until restarted.
pub fn apply_binaries(
    sudo: Option<&str>,
    install_dir: &Path,
    new_bin: &Path,
    new_runner: Option<&Path>,
    current_version: &str,
) -> Result<ApplyOutcome> {
    let cur_bin = install_dir.join("neomind");
    let cur_runner = install_dir.join("neomind-extension-runner");
    let bak_bin = install_dir.join(format!("neomind.{current_version}.bak"));
    let bak_runner = install_dir.join(format!("neomind-extension-runner.{current_version}.bak"));

    let had_cur_bin = cur_bin.exists();
    if had_cur_bin {
        run_shell(
            sudo,
            &[
                "cp",
                "-a",
                &cur_bin.to_string_lossy(),
                &bak_bin.to_string_lossy(),
            ],
        )?;
    }
    if new_runner.is_some() && cur_runner.exists() {
        run_shell(
            sudo,
            &[
                "cp",
                "-a",
                &cur_runner.to_string_lossy(),
                &bak_runner.to_string_lossy(),
            ],
        )?;
    }

    run_shell(
        sudo,
        &[
            "install",
            "-m",
            "755",
            &new_bin.to_string_lossy(),
            &cur_bin.to_string_lossy(),
        ],
    )?;
    if let Some(runner) = new_runner {
        // Only swap the runner when one is already installed — a standalone
        // binary-only install shouldn't grow a runner it never had.
        if cur_runner.exists() {
            if let Err(err) = run_shell(
                sudo,
                &[
                    "install",
                    "-m",
                    "755",
                    &runner.to_string_lossy(),
                    &cur_runner.to_string_lossy(),
                ],
            ) {
                // Roll back the already-swapped main binary so main & runner stay in sync.
                if had_cur_bin && bak_bin.exists() {
                    let _ = run_shell(
                        sudo,
                        &[
                            "cp",
                            "-a",
                            &bak_bin.to_string_lossy(),
                            &cur_bin.to_string_lossy(),
                        ],
                    );
                }
                return Err(err).context(
                    "failed to install neomind-extension-runner (rolled back neomind binary)",
                );
            }
        }
    }

    Ok(ApplyOutcome {
        backup_binary: bak_bin,
        backup_runner: if bak_runner.exists() {
            Some(bak_runner)
        } else {
            None
        },
    })
}

/// Stage-swap `web_tarball` into the live web dir (best-effort, atomic-ish).
///
/// Ownership follows `install.sh` (www-data preferred, neomind fallback) —
/// the old CLI path chowned to neomind unconditionally, which broke nginx
/// reads on www-data-owned installs.
///
/// Extraction happens as the INVOKING user in a dir they can write (next to
/// the tarball — the CLI temp dir or the API staging dir), then the result
/// is moved into place with sudo. Extracting directly into a sudo-created
/// stage would fail for a non-root caller (root-owned dir, non-root unpack).
pub fn apply_web_dir(sudo: Option<&str>, web_tarball: &Path) -> Result<bool> {
    let web = web_dir();
    let web_str = web.to_string_lossy().to_string();
    let stage = format!("{}.new.{}", web_str, std::process::id());
    let old = format!("{}.old.{}", web_str, std::process::id());

    let extract_dir = web_tarball
        .parent()
        .unwrap_or_else(|| Path::new("/tmp"))
        .join(format!("neomind-web-extract-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&extract_dir);
    extract_tar_gz(web_tarball, &extract_dir)?;

    run_shell(sudo, &["rm", "-rf", &stage])?;
    // `mv` falls back to copy across filesystems (e.g. /tmp → /var/www).
    let into_stage = run_shell(sudo, &["mv", &extract_dir.to_string_lossy(), &stage]);
    // No-op after a successful mv; cleans up when it failed.
    let _ = std::fs::remove_dir_all(&extract_dir);
    into_stage?;
    let owner = if std::process::Command::new("id")
        .arg("www-data")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        "www-data:www-data"
    } else {
        "neomind:neomind"
    };
    run_shell(sudo, &["chown", "-R", owner, &stage])?;
    run_shell(sudo, &["rm", "-rf", &old])?;
    let had_existing = web.exists();
    if had_existing {
        run_shell(sudo, &["mv", &web_str, &old])?;
    }
    match run_shell(sudo, &["mv", &stage, &web_str]) {
        Ok(()) => {
            if had_existing {
                let _ = run_shell(sudo, &["rm", "-rf", &old]);
            }
            Ok(true)
        }
        Err(err) => {
            if had_existing {
                let _ = run_shell(sudo, &["mv", &old, &web_str]);
            }
            let _ = run_shell(sudo, &["rm", "-rf", &stage]);
            Err(err).context("failed to move staged web directory into place (restored previous)")
        }
    }
}

/// Locate the install dir from the running binary (canonicalized), with the
/// hardcoded default as fallback for exotic launch paths.
pub fn install_dir_from_exe() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.canonicalize().ok())
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from(DEFAULT_INSTALL_DIR))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_parses_dirty_versions() {
        assert_eq!(semver("0.9.21"), [0, 9, 21]);
        assert_eq!(semver("v1.2.3"), [1, 2, 3]);
        assert_eq!(semver("1.2.3-rc.1"), [1, 2, 3]);
        assert_eq!(semver("2"), [2, 0, 0]);
    }

    #[test]
    fn is_newer_compares_componentwise() {
        assert!(is_newer("0.9.22", "0.9.21"));
        assert!(is_newer("1.0.0", "0.9.99"));
        assert!(!is_newer("0.9.21", "0.9.21"));
        assert!(!is_newer("0.9.20", "0.9.21"));
        assert!(!is_newer("0.10.0-rc1", "0.10.0")); // pre-release NOT newer
    }

    #[test]
    fn urls_have_expected_shape() {
        assert_eq!(
            server_tarball_url("0.9.22", "arm64").unwrap(),
            "https://github.com/camthink-ai/NeoMind/releases/download/v0.9.22/neomind-server-linux-arm64.tar.gz"
        );
        assert_eq!(
            web_tarball_url("0.9.22").unwrap(),
            "https://github.com/camthink-ai/NeoMind/releases/download/v0.9.22/neomind-web.tar.gz"
        );
        assert_eq!(
            checksums_url("v1.4.0").unwrap(),
            "https://github.com/camthink-ai/NeoMind/releases/download/v1.4.0/checksums.txt"
        );
        assert!(server_tarball_url("../1.0.0", "arm64").is_err());
        assert!(server_tarball_url("1.0.0", "../evil").is_err());
        assert!(web_tarball_url("1.0.0?foo=bar").is_err());
    }

    #[test]
    fn staging_paths_nest_under_data_dir() {
        let root = staging_root(Path::new("/var/lib/neomind"));
        assert_eq!(root, Path::new("/var/lib/neomind/upgrade"));
        assert_eq!(
            staging_dir(Path::new("/var/lib/neomind"), "0.9.22").unwrap(),
            Path::new("/var/lib/neomind/upgrade/v0.9.22")
        );
        assert!(staging_dir(Path::new("/var/lib/neomind"), "../../etc").is_err());
    }

    #[test]
    fn validate_semver_accepts_valid_and_rejects_malicious() {
        assert_eq!(validate_semver_version("1.4.0").unwrap(), "1.4.0");
        assert_eq!(validate_semver_version("v1.4.0").unwrap(), "1.4.0");
        assert_eq!(
            validate_semver_version("v1.4.0-rc.1").unwrap(),
            "1.4.0-rc.1"
        );
        assert_eq!(
            validate_semver_version("0.10.2-beta-2.1").unwrap(),
            "0.10.2-beta-2.1"
        );

        let invalid = [
            "",
            "v",
            "vv1.4.0",
            "1.4",
            "1.4.0.1",
            "1.4.0-",
            "1.4.0-.rc",
            "1.4.0-rc.",
            "1.4.0-rc..1",
            "../1.4.0",
            "1.4.0/../../etc",
            "1.4.0\\evil",
            "1.4.0?token=1",
            "1.4.0#frag",
            "1.4.0%2f",
            "1.4.0 ",
            " 1.4.0",
            "1.4.0\n",
            "1.4.0+build",
            "1.4.0-rc_1",
        ];
        for bad in invalid {
            assert!(
                validate_semver_version(bad).is_err(),
                "expected invalid version to be rejected: {bad:?}"
            );
        }

        let too_long = format!("1.4.0-{}", "a".repeat(60));
        assert!(too_long.len() > 64);
        assert!(validate_semver_version(&too_long).is_err());
    }

    #[test]
    fn validate_single_filename_and_resolve_staged_file_block_escapes() {
        assert_eq!(validate_single_filename("neomind").unwrap(), "neomind");
        assert_eq!(
            validate_single_filename("neomind-web.tar.gz").unwrap(),
            "neomind-web.tar.gz"
        );
        for bad in [
            "",
            ".",
            "..",
            "../neomind",
            "a/b",
            "a\\b",
            "/etc/passwd",
            "a\0b",
        ] {
            assert!(
                validate_single_filename(bad).is_err(),
                "expected filename to be rejected: {bad:?}"
            );
        }

        let tmp = tempfile::tempdir().unwrap();
        let staging = tmp.path().join("staging");
        std::fs::create_dir_all(&staging).unwrap();
        let regular = staging.join("neomind");
        std::fs::write(&regular, b"binary").unwrap();
        let resolved = resolve_staged_file(&staging, "neomind").unwrap();
        assert_eq!(resolved, regular.canonicalize().unwrap());

        // Reject directory
        std::fs::create_dir_all(staging.join("subdir")).unwrap();
        assert!(resolve_staged_file(&staging, "subdir").is_err());

        // Reject symlink inside staging pointing outside
        #[cfg(unix)]
        {
            let outside = tmp.path().join("outside");
            std::fs::write(&outside, b"evil").unwrap();
            std::os::unix::fs::symlink(&outside, staging.join("link")).unwrap();
            assert!(resolve_staged_file(&staging, "link").is_err());
        }
    }

    #[test]
    fn parse_and_verify_sha256_works() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("asset.tar.gz");
        std::fs::write(&f, b"hello neomind").unwrap();
        let mut hasher = Sha256::new();
        hasher.update(b"hello neomind");
        let digest = hex::encode(hasher.finalize());

        let txt = format!(
            "# release checksums\n{digest}  neomind-server-linux-amd64.tar.gz\n{digest} *./neomind-web.tar.gz\ninvalid line\n"
        );
        let parsed = parse_checksums_txt(&txt);
        assert_eq!(
            parsed
                .get("neomind-server-linux-amd64.tar.gz")
                .map(String::as_str),
            Some(digest.as_str())
        );
        assert_eq!(
            parsed.get("neomind-web.tar.gz").map(String::as_str),
            Some(digest.as_str())
        );

        verify_file_sha256(&f, &digest).unwrap();
        let wrong = "0".repeat(64);
        assert!(verify_file_sha256(&f, &wrong).is_err());
        assert!(verify_file_sha256(&f, "not-hex").is_err());
    }

    #[test]
    fn extract_tar_gz_accepts_regular_and_rejects_symlinks_and_traversal() {
        use flate2::write::GzEncoder;
        use flate2::Compression;

        let tmp = tempfile::tempdir().unwrap();

        // 1. Valid tarball with ./ prefix (like neomind-web.tar.gz)
        let good_tgz = tmp.path().join("good.tar.gz");
        {
            let f = std::fs::File::create(&good_tgz).unwrap();
            let enc = GzEncoder::new(f, Compression::default());
            let mut builder = tar::Builder::new(enc);
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Regular);
            let data = b"#!/bin/sh\necho ok\n";
            header.set_size(data.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder
                .append_data(&mut header, "./neomind", &data[..])
                .unwrap();
            builder.into_inner().unwrap().finish().unwrap();
        }
        let dest = tmp.path().join("out-good");
        extract_tar_gz(&good_tgz, &dest).unwrap();
        assert_eq!(
            std::fs::read(dest.join("neomind")).unwrap(),
            b"#!/bin/sh\necho ok\n"
        );

        // 2. Symlink entry must be rejected
        let sym_tgz = tmp.path().join("sym.tar.gz");
        {
            let f = std::fs::File::create(&sym_tgz).unwrap();
            let enc = GzEncoder::new(f, Compression::default());
            let mut builder = tar::Builder::new(enc);
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_size(0);
            header.set_mode(0o777);
            builder
                .append_link(&mut header, "evil", "/etc/passwd")
                .unwrap();
            builder.into_inner().unwrap().finish().unwrap();
        }
        let dest_sym = tmp.path().join("out-sym");
        assert!(extract_tar_gz(&sym_tgz, &dest_sym).is_err());

        // 3. Hardlink entry must be rejected
        let hard_tgz = tmp.path().join("hard.tar.gz");
        {
            let f = std::fs::File::create(&hard_tgz).unwrap();
            let enc = GzEncoder::new(f, Compression::default());
            let mut builder = tar::Builder::new(enc);
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Link);
            header.set_size(0);
            header.set_mode(0o644);
            builder
                .append_link(&mut header, "hard", "/etc/passwd")
                .unwrap();
            builder.into_inner().unwrap().finish().unwrap();
        }
        let dest_hard = tmp.path().join("out-hard");
        assert!(extract_tar_gz(&hard_tgz, &dest_hard).is_err());

        // 4. Path traversal (`../escape`) must be rejected
        let trav_tgz = tmp.path().join("trav.tar.gz");
        {
            let mut raw_header = [0u8; 512];
            let name = b"../escape.txt";
            raw_header[..name.len()].copy_from_slice(name);
            raw_header[100..108].copy_from_slice(b"0000644\0");
            raw_header[108..116].copy_from_slice(b"0000000\0");
            raw_header[116..124].copy_from_slice(b"0000000\0");
            raw_header[124..136].copy_from_slice(b"00000000004\0");
            raw_header[136..148].copy_from_slice(b"00000000000\0");
            raw_header[148..156].copy_from_slice(b"        ");
            raw_header[156] = b'0'; // regular file
            let cksum: u32 = raw_header.iter().map(|&b| b as u32).sum();
            let cksum_str = format!("{cksum:06o}\0 ");
            raw_header[148..156].copy_from_slice(cksum_str.as_bytes());

            let mut data_block = [0u8; 512];
            data_block[..4].copy_from_slice(b"evil");
            let eof_blocks = [0u8; 1024];

            let f = std::fs::File::create(&trav_tgz).unwrap();
            let mut enc = GzEncoder::new(f, Compression::default());
            use std::io::Write;
            enc.write_all(&raw_header).unwrap();
            enc.write_all(&data_block).unwrap();
            enc.write_all(&eof_blocks).unwrap();
            enc.finish().unwrap();
        }
        let dest_trav = tmp.path().join("out-trav");
        assert!(extract_tar_gz(&trav_tgz, &dest_trav).is_err());
        assert!(!tmp.path().join("escape.txt").exists());
    }

    #[cfg(unix)]
    #[test]
    fn apply_binaries_rolls_back_main_when_runner_install_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let install_dir = tmp.path().join("bin");
        let staged_dir = tmp.path().join("staged");
        std::fs::create_dir_all(&install_dir).unwrap();
        std::fs::create_dir_all(&staged_dir).unwrap();

        let cur_bin = install_dir.join("neomind");
        let cur_runner = install_dir.join("neomind-extension-runner");
        let new_bin = staged_dir.join("neomind");
        let missing_runner = staged_dir.join("missing-runner");

        std::fs::write(&cur_bin, b"old-main-v1").unwrap();
        std::fs::write(&cur_runner, b"old-runner-v1").unwrap();
        std::fs::write(&new_bin, b"new-main-v2").unwrap();

        let err = apply_binaries(None, &install_dir, &new_bin, Some(&missing_runner), "1.3.0");
        assert!(err.is_err(), "expected runner install failure");
        assert_eq!(
            std::fs::read(&cur_bin).unwrap(),
            b"old-main-v1",
            "neomind binary must be rolled back on runner failure"
        );
        assert_eq!(std::fs::read(&cur_runner).unwrap(), b"old-runner-v1");
    }

    #[test]
    fn manifest_round_trips() {
        let m = StagingManifest {
            version: "0.9.22".into(),
            arch: "amd64".into(),
            staged_at: 12345,
            server_binary: "neomind".into(),
            server_tarball: Some("neomind.tar.gz".into()),
            extension_runner: Some("neomind-extension-runner".into()),
            web_tarball: Some("neomind-web.tar.gz".into()),
        };
        let json = serde_json::to_string(&m).unwrap();
        let back: StagingManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.version, "0.9.22");
        assert_eq!(back.server_tarball.as_deref(), Some("neomind.tar.gz"));
        assert_eq!(back.web_tarball.as_deref(), Some("neomind-web.tar.gz"));
    }
}
