//! Self-upgrade for pre-built release binaries, plus the startup update-check hint.
//!
//! Pure planning/parsing lives here; network and filesystem replacement are thin IO
//! boundaries injectable in tests via [`ReleaseClient`].

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const REPO: &str = "akunzai/duodiff";

/// How the running binary appears to have been installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallMethod {
    /// `install.sh` / `install.ps1`, manual release extract, or other standalone copy.
    Standalone,
    Homebrew,
    Scoop,
    CargoInstall,
    CargoBinstall,
    /// Managed/unknown layout — refuse self-upgrade.
    Refuse {
        hint: String,
    },
}

/// Platform asset naming shared with `install.sh`, `install.ps1`, and binstall metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Platform {
    pub target: String,
    pub archive_ext: &'static str,
    pub bin_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseAsset {
    pub version: String,
    pub pkg_name: String,
    pub archive_name: String,
    pub download_base: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpgradePlan {
    pub exe_path: PathBuf,
    pub method: InstallMethod,
    pub current_version: String,
    pub target_version: String,
    pub asset: ReleaseAsset,
    pub check_only: bool,
}

pub struct Options {
    pub check_only: bool,
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecuteOutcome {
    UpToDate,
    UpdateAvailable,
    Upgraded,
}

pub trait ReleaseClient {
    fn fetch_latest_tag(&self) -> Result<String>;
    fn download(&self, url: &str) -> Result<Vec<u8>>;
}

pub struct UreqClient;

impl ReleaseClient for UreqClient {
    fn fetch_latest_tag(&self) -> Result<String> {
        let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
        let mut response = ureq::get(&url)
            .header("User-Agent", "duodiff-upgrader")
            .header("Accept", "application/vnd.github+json")
            .call()
            .with_context(|| format!("could not query latest release at {url}"))?;
        let body = response
            .body_mut()
            .read_to_string()
            .context("failed to read latest-release response")?;
        parse_latest_release_tag(body.as_bytes())
    }

    fn download(&self, url: &str) -> Result<Vec<u8>> {
        let mut reader = ureq::get(url)
            .header("User-Agent", "duodiff-upgrader")
            .call()
            .with_context(|| format!("download failed: {url}"))?
            .into_body()
            .into_reader();
        let mut body = Vec::new();
        reader
            .read_to_end(&mut body)
            .with_context(|| format!("failed to read download body: {url}"))?;
        Ok(body)
    }
}

/// Lowercase hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// Entry point for `duodiff --upgrade`.
pub fn run(opts: Options) -> Result<()> {
    run_with_client(opts, &UreqClient)
}

pub fn run_with_client(opts: Options, client: &impl ReleaseClient) -> Result<()> {
    let exe_path = std::env::current_exe().context("could not resolve the running executable")?;
    let method = detect_install_method(&exe_path);
    match &method {
        InstallMethod::Homebrew
        | InstallMethod::Scoop
        | InstallMethod::CargoInstall
        | InstallMethod::CargoBinstall => {
            let hint = upgrade_hint(&method).expect("managed install should have a hint");
            bail!("self-upgrade is not supported for this install — use: {hint}");
        }
        InstallMethod::Refuse { hint } => {
            bail!("self-upgrade is not supported for this install ({hint})");
        }
        InstallMethod::Standalone => {}
    }

    let plan = UpgradePlan {
        exe_path,
        method,
        current_version: env!("CARGO_PKG_VERSION").to_string(),
        target_version: opts
            .version
            .as_deref()
            .map(normalize_tag)
            .unwrap_or_default(),
        asset: ReleaseAsset {
            version: String::new(),
            pkg_name: String::new(),
            archive_name: String::new(),
            download_base: String::new(),
        },
        check_only: opts.check_only,
    };

    match execute_plan(&plan, client)? {
        ExecuteOutcome::UpToDate | ExecuteOutcome::Upgraded => Ok(()),
        ExecuteOutcome::UpdateAvailable => std::process::exit(1),
    }
}

pub fn execute_plan(plan: &UpgradePlan, client: &impl ReleaseClient) -> Result<ExecuteOutcome> {
    let platform = detect_platform()?;
    let target_version = if plan.target_version.is_empty() {
        let tag = client.fetch_latest_tag()?;
        normalize_tag(&tag)
    } else {
        plan.target_version.clone()
    };

    let asset = release_asset(&target_version, &platform);

    if version_cmp(&target_version, &plan.current_version) == Ordering::Equal {
        println!(
            "duodiff {} is up to date ({})",
            plan.current_version,
            plan.exe_path.display()
        );
        return Ok(ExecuteOutcome::UpToDate);
    }

    if version_cmp(&target_version, &plan.current_version) == Ordering::Less {
        bail!(
            "requested release {target_version} is older than the running version {}",
            plan.current_version
        );
    }

    if plan.check_only {
        println!(
            "duodiff {} → {} available ({})",
            plan.current_version,
            target_version,
            plan.exe_path.display()
        );
        return Ok(ExecuteOutcome::UpdateAvailable);
    }

    let archive_url = format!(
        "{}/{}/{}",
        asset.download_base, asset.version, asset.archive_name
    );
    let checksum_url = format!("{archive_url}.sha256");

    let archive_bytes = client
        .download(&archive_url)
        .with_context(|| format!("could not download release asset for {}", platform.target))?;
    let checksum_bytes = client.download(&checksum_url)?;
    let expected_hash = parse_sha256_file(&String::from_utf8_lossy(&checksum_bytes))?;
    verify_sha256(&archive_bytes, &expected_hash)
        .with_context(|| format!("checksum mismatch for {}", asset.archive_name))?;

    let new_binary = extract_binary(&archive_bytes, &asset, &platform)?;
    replace_binary(&plan.exe_path, &new_binary)?;

    println!(
        "upgraded duodiff {} → {} ({})",
        plan.current_version,
        target_version,
        plan.exe_path.display()
    );
    Ok(ExecuteOutcome::Upgraded)
}

pub fn detect_platform() -> Result<Platform> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let (plat, ext) = match os {
        "linux" => ("unknown-linux-gnu", "tar.gz"),
        "macos" => ("apple-darwin", "tar.gz"),
        "windows" => ("pc-windows-msvc", "zip"),
        other => bail!("unsupported operating system: {other}"),
    };
    let cpu = match arch {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        other => bail!("unsupported architecture: {other}"),
    };
    if os == "windows" && cpu != "x86_64" {
        bail!("no prebuilt Windows binary for {cpu} (only x86_64 is published)");
    }
    let bin_name = if os == "windows" {
        "duodiff.exe".to_string()
    } else {
        "duodiff".to_string()
    };
    Ok(Platform {
        target: format!("{cpu}-{plat}"),
        archive_ext: ext,
        bin_name,
    })
}

pub fn release_tag(version: &str) -> String {
    let bare = normalize_tag(version);
    format!("v{bare}")
}

pub fn release_asset(version: &str, platform: &Platform) -> ReleaseAsset {
    let tag = release_tag(version);
    let pkg_name = format!("duodiff-{tag}-{}", platform.target);
    let archive_name = format!("{pkg_name}.{}", platform.archive_ext);
    let download_base = format!("https://github.com/{REPO}/releases/download");
    ReleaseAsset {
        version: tag,
        pkg_name,
        archive_name,
        download_base,
    }
}

/// Normalize a user- or API-supplied release tag to a bare `X.Y.Z` version.
///
/// Accepts optional `v`/`V` prefix and surrounding whitespace, so `v0.1.0` and
/// `0.1.0` resolve to the same release.
pub fn normalize_tag(tag: &str) -> String {
    let trimmed = tag.trim();
    trimmed
        .strip_prefix('v')
        .or_else(|| trimmed.strip_prefix('V'))
        .unwrap_or(trimmed)
        .to_string()
}

pub fn version_cmp(left: &str, right: &str) -> Ordering {
    let parse = |s: &str| -> Vec<u64> {
        normalize_tag(s)
            .split('.')
            .map(|p| p.parse().unwrap_or(0))
            .collect()
    };
    let a = parse(left);
    let b = parse(right);
    let len = a.len().max(b.len());
    for i in 0..len {
        let av = *a.get(i).unwrap_or(&0);
        let bv = *b.get(i).unwrap_or(&0);
        match av.cmp(&bv) {
            Ordering::Equal => {}
            other => return other,
        }
    }
    Ordering::Equal
}

pub fn parse_latest_release_tag(body: &[u8]) -> Result<String> {
    let value: serde_json::Value =
        serde_json::from_slice(body).context("invalid latest-release JSON")?;
    value
        .get("tag_name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .context("latest-release JSON missing tag_name")
}

pub fn parse_sha256_file(content: &str) -> Result<String> {
    let line = content
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .context("checksum file is empty")?;
    let hash = line
        .split_whitespace()
        .next()
        .context("checksum line missing hash")?;
    Ok(hash.to_ascii_lowercase())
}

pub fn verify_sha256(data: &[u8], expected_hex: &str) -> Result<()> {
    let actual = sha256_hex(data);
    if actual != expected_hex.to_ascii_lowercase() {
        bail!("expected {expected_hex}, got {actual}");
    }
    Ok(())
}

pub fn detect_install_method(exe: &Path) -> InstallMethod {
    let path = exe.to_string_lossy();
    let canonical = fs::canonicalize(exe).unwrap_or_else(|_| exe.to_path_buf());
    let canon = canonical.to_string_lossy();

    if cellar_path(&canon) {
        return InstallMethod::Homebrew;
    }

    if (path.contains("/homebrew/bin/")
        || path.contains("/usr/local/bin/")
        || path.contains("\\homebrew\\bin\\"))
        && symlink_points_to_cellar(exe)
    {
        return InstallMethod::Homebrew;
    }

    if scoop_install_path(&canon) {
        return InstallMethod::Scoop;
    }

    if is_cargo_bin_path(&canon) {
        return match cargo_install_kind() {
            CargoInstallKind::Binstall => InstallMethod::CargoBinstall,
            CargoInstallKind::Install => InstallMethod::CargoInstall,
            CargoInstallKind::Unknown => InstallMethod::Refuse {
                hint: "cargo toolchain install detected — use \
                       `cargo install duodiff --force` or `cargo binstall duodiff --force`"
                    .to_string(),
            },
        };
    }

    if looks_managed_system_path(&canon) {
        return InstallMethod::Refuse {
            hint: "system-managed path — re-run install.sh or install.ps1".to_string(),
        };
    }

    InstallMethod::Standalone
}

pub fn upgrade_hint(method: &InstallMethod) -> Option<&'static str> {
    match method {
        InstallMethod::Homebrew => Some("brew upgrade duodiff"),
        InstallMethod::Scoop => Some("scoop update duodiff"),
        InstallMethod::CargoInstall => Some("cargo install duodiff --force"),
        InstallMethod::CargoBinstall => Some("cargo binstall duodiff --force"),
        InstallMethod::Standalone | InstallMethod::Refuse { .. } => None,
    }
}

fn cellar_path(path: &str) -> bool {
    path.contains("/Cellar/duodiff/") || path.contains("\\Cellar\\duodiff\\")
}

/// Scoop installs resolve to `scoop/apps/duodiff/...`; the shim on PATH is
/// `scoop/shims/duodiff.exe` — treat both as managed.
fn scoop_install_path(path: &str) -> bool {
    const MARKERS: &[&str] = &[
        "scoop/apps/duodiff",
        "scoop\\apps\\duodiff",
        "scoop/shims/duodiff",
        "scoop\\shims\\duodiff",
    ];
    MARKERS.iter().any(|marker| path.contains(marker))
}

fn symlink_points_to_cellar(exe: &Path) -> bool {
    let meta = match fs::symlink_metadata(exe) {
        Ok(m) => m,
        Err(_) => return false,
    };
    if !meta.file_type().is_symlink() {
        return false;
    }
    let target = match fs::read_link(exe) {
        Ok(t) => t,
        Err(_) => return false,
    };
    cellar_path(&target.to_string_lossy())
}

fn is_cargo_bin_path(path: &str) -> bool {
    path.contains("/.cargo/bin/duodiff")
        || path.ends_with("\\.cargo\\bin\\duodiff.exe")
        || path.ends_with("/.cargo/bin/duodiff.exe")
}

enum CargoInstallKind {
    Binstall,
    Install,
    Unknown,
}

fn cargo_install_kind() -> CargoInstallKind {
    let Some(home) = dirs::home_dir() else {
        return CargoInstallKind::Unknown;
    };
    let crates2 = home.join(".cargo/.crates2.json");
    if let Ok(raw) = fs::read_to_string(&crates2) {
        if raw.contains("\"duodiff\"") && raw.contains("bin") {
            return CargoInstallKind::Binstall;
        }
    }
    let crates = home.join(".cargo/.crates.toml");
    if let Ok(raw) = fs::read_to_string(&crates) {
        if raw.contains("duodiff") {
            return CargoInstallKind::Install;
        }
    }
    CargoInstallKind::Unknown
}

fn looks_managed_system_path(path: &str) -> bool {
    path.starts_with("/usr/bin/")
        || path.starts_with("/bin/")
        || path.contains("\\Program Files\\")
        || path.contains("\\Program Files (x86)\\")
}

pub fn extract_binary(
    archive: &[u8],
    asset: &ReleaseAsset,
    platform: &Platform,
) -> Result<Vec<u8>> {
    match platform.archive_ext {
        "tar.gz" => extract_from_tar_gz(archive, &asset.pkg_name, &platform.bin_name),
        "zip" => extract_from_zip(archive, &asset.pkg_name, &platform.bin_name),
        other => bail!("unsupported archive format: {other}"),
    }
}

fn extract_from_tar_gz(archive: &[u8], pkg_name: &str, bin_name: &str) -> Result<Vec<u8>> {
    let decoder = flate2::read::GzDecoder::new(Cursor::new(archive));
    let mut archive = tar::Archive::new(decoder);
    let wanted = format!("{pkg_name}/{bin_name}");
    for entry in archive.entries().context("invalid tar archive")? {
        let mut entry = entry.context("invalid tar entry")?;
        let path = entry.path().context("tar entry missing path")?;
        if path.to_string_lossy() == wanted {
            let mut buf = Vec::new();
            entry
                .read_to_end(&mut buf)
                .context("failed to read binary from tar archive")?;
            return Ok(buf);
        }
    }
    bail!("binary not found in archive at {wanted}");
}

fn extract_from_zip(archive: &[u8], pkg_name: &str, bin_name: &str) -> Result<Vec<u8>> {
    let cursor = Cursor::new(archive);
    let mut zip = zip::ZipArchive::new(cursor).context("invalid zip archive")?;
    let wanted = format!("{pkg_name}/{bin_name}");
    let mut file = zip
        .by_name(&wanted)
        .with_context(|| format!("binary not found in archive at {wanted}"))?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)
        .context("failed to read binary from zip archive")?;
    Ok(buf)
}

pub fn replace_binary(target: &Path, bytes: &[u8]) -> Result<()> {
    let parent = target
        .parent()
        .context("executable has no parent directory")?;
    if !is_writable_dir(parent) {
        bail!("cannot write to {} (permission denied)", parent.display());
    }

    #[cfg(unix)]
    {
        let tmp_name = format!(".duodiff-upgrade-{}", std::process::id());
        let tmp_path = parent.join(tmp_name);
        fs::write(&tmp_path, bytes).context("failed to write staged binary")?;
        fs::set_permissions(&tmp_path, fs::Permissions::from_mode(0o755))
            .context("failed to set executable permissions")?;
        fs::rename(&tmp_path, target).context("failed to replace running binary")?;
        Ok(())
    }

    #[cfg(windows)]
    {
        replace_binary_windows(target, bytes)
    }
}

#[cfg(windows)]
fn replace_binary_windows(target: &Path, bytes: &[u8]) -> Result<()> {
    let parent = target
        .parent()
        .context("executable has no parent directory")?;
    let stem = target
        .file_stem()
        .and_then(|s| s.to_str())
        .context("executable has no file stem")?;
    let staging = parent.join(format!("{stem}.exe.new"));
    fs::write(&staging, bytes).context("failed to write staged binary")?;

    // A running .exe cannot be overwritten on Windows. Spawn a detached helper that waits for
    // this process to exit, then swaps the staged file into place.
    let script = parent.join(format!("{stem}.upgrade.ps1"));
    let script_body = format!(
        "$ErrorActionPreference = 'Stop'\n\
         $target = '{}'\n\
         $staging = '{}'\n\
         $pid = {}\n\
         while (Get-Process -Id $pid -ErrorAction SilentlyContinue) {{ Start-Sleep -Milliseconds 200 }}\n\
         Move-Item -Force $staging $target\n\
         Remove-Item -Force '{}'\n",
        target.display(),
        staging.display(),
        std::process::id(),
        script.display()
    );
    fs::write(&script, script_body).context("failed to write upgrade helper script")?;

    std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-WindowStyle",
            "Hidden",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            &script.to_string_lossy(),
        ])
        .spawn()
        .context("failed to spawn Windows upgrade helper")?;

    println!(
        "upgrade staged; exit this process to finish replacing {}",
        target.display()
    );
    std::process::exit(0);
}

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

fn is_writable_dir(dir: &Path) -> bool {
    fs::metadata(dir)
        .map(|m| !m.permissions().readonly())
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Startup update check (throttled "is there a newer release?" hint)
// ---------------------------------------------------------------------------

/// Minimum gap between startup checks (once per day), to be polite to the GitHub API
/// (unauthenticated `api.github.com` is 60 requests/hour/IP).
pub const CHECK_INTERVAL_SECS: u64 = 86_400;

/// Outcome of an attempted background check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateCheckOutcome {
    /// A newer release is available (normalized version, e.g. `"0.2.0"`).
    Newer(String),
    /// Checked successfully; already on the latest version.
    UpToDate,
    /// The check could not complete (offline / rate-limited / parse error). Stay silent and
    /// retry on the next launch — do NOT record it against the throttle.
    Failed,
}

/// Persisted throttle state (`$XDG_CACHE_HOME/duodiff/update_check.json`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateCheckState {
    /// Unix seconds of the last completed check.
    #[serde(default)]
    pub last_check: u64,
    /// Latest release tag seen at the last check (normalized); lets the hint persist across
    /// launches within the throttle window without re-hitting the network.
    #[serde(default)]
    pub latest_seen: String,
}

pub fn state_path() -> Result<PathBuf> {
    let dir = dirs::cache_dir().context("locate cache directory")?;
    Ok(dir.join("duodiff").join("update_check.json"))
}

pub fn load_state(path: &Path) -> UpdateCheckState {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn save_state(path: &Path, state: &UpdateCheckState) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(state) {
        let _ = std::fs::write(path, json);
    }
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Whether enough time has passed since `last_check` to run another network check.
pub fn should_check(last_check: u64, now: u64) -> bool {
    now.saturating_sub(last_check) >= CHECK_INTERVAL_SECS
}

/// `Some(normalized)` when `latest` is a strictly newer version than `current`, else `None`.
pub fn is_newer(latest: &str, current: &str) -> Option<String> {
    let latest = normalize_tag(latest);
    if latest.is_empty() {
        return None;
    }
    (version_cmp(&latest, current) == Ordering::Greater).then_some(latest)
}

/// Footer hint for an available update, tailored to how the binary was installed.
pub fn update_hint(latest: &str, method: &InstallMethod) -> String {
    let cmd = upgrade_hint(method)
        .map(str::to_string)
        .or_else(|| match method {
            InstallMethod::Standalone => Some("duodiff --upgrade".to_string()),
            _ => None,
        });
    match cmd {
        Some(cmd) => format!("⬆ v{latest} available — run {cmd}"),
        None => format!("⬆ v{latest} available"),
    }
}

/// Run the network check against `client`, comparing the latest tag with `current`.
/// Thin IO boundary; pure classification lives in [`is_newer`].
pub fn check(client: &impl ReleaseClient, current: &str) -> UpdateCheckOutcome {
    match client.fetch_latest_tag() {
        Ok(tag) => match is_newer(&tag, current) {
            Some(v) => UpdateCheckOutcome::Newer(v),
            None => UpdateCheckOutcome::UpToDate,
        },
        Err(_) => UpdateCheckOutcome::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    struct FakeClient {
        latest: String,
        files: std::collections::HashMap<String, Vec<u8>>,
    }

    impl ReleaseClient for FakeClient {
        fn fetch_latest_tag(&self) -> Result<String> {
            Ok(self.latest.clone())
        }

        fn download(&self, url: &str) -> Result<Vec<u8>> {
            self.files
                .get(url)
                .cloned()
                .with_context(|| format!("missing fixture for {url}"))
        }
    }

    #[test]
    fn normalize_tag_accepts_v_prefix_whitespace_and_bare_versions() {
        assert_eq!(normalize_tag("v0.1.0"), "0.1.0");
        assert_eq!(normalize_tag("V0.1.0"), "0.1.0");
        assert_eq!(normalize_tag("0.1.0"), "0.1.0");
        assert_eq!(normalize_tag("  v0.1.0  "), "0.1.0");
        assert_eq!(normalize_tag(" 0.1.0 "), "0.1.0");
    }

    #[test]
    fn release_asset_treats_v_prefixed_and_bare_versions_equally() {
        let platform = Platform {
            target: "aarch64-apple-darwin".to_string(),
            archive_ext: "tar.gz",
            bin_name: "duodiff".to_string(),
        };
        let with_v = release_asset("v0.1.0", &platform);
        let bare = release_asset("0.1.0", &platform);
        assert_eq!(with_v, bare);
        assert_eq!(with_v.version, "v0.1.0");
        assert_eq!(
            with_v.archive_name,
            "duodiff-v0.1.0-aarch64-apple-darwin.tar.gz"
        );
    }

    #[test]
    fn version_cmp_orders_semver_like_tags() {
        assert_eq!(version_cmp("0.2.0", "0.1.0"), Ordering::Greater);
        assert_eq!(version_cmp("v0.1.0", "0.1.0"), Ordering::Equal);
        assert_eq!(version_cmp("0.0.9", "0.1.0"), Ordering::Less);
    }

    #[test]
    fn parse_latest_release_tag_reads_tag_name() {
        let body = br#"{"tag_name":"v0.1.0"}"#;
        assert_eq!(parse_latest_release_tag(body).unwrap(), "v0.1.0");
    }

    #[test]
    fn parse_sha256_file_tolerates_crlf_and_two_field_format() {
        let hash = "a".repeat(64);
        let content = format!("{hash}  duodiff-v0.1.0-x86_64-apple-darwin.tar.gz\r\n");
        assert_eq!(parse_sha256_file(&content).unwrap(), hash);
    }

    #[test]
    fn verify_sha256_checks_digest() {
        let data = b"hello";
        let expected = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        verify_sha256(data, expected).unwrap();
        assert!(verify_sha256(data, "00").is_err());
    }

    #[test]
    fn detect_install_method_recognizes_homebrew_cellar() {
        let method =
            detect_install_method(Path::new("/opt/homebrew/Cellar/duodiff/0.1.0/bin/duodiff"));
        assert_eq!(method, InstallMethod::Homebrew);
        assert_eq!(upgrade_hint(&method), Some("brew upgrade duodiff"));
    }

    #[test]
    fn detect_install_method_recognizes_scoop_layout() {
        let method = detect_install_method(Path::new(
            "C:/Users/me/scoop/apps/duodiff/current/duodiff.exe",
        ));
        assert_eq!(method, InstallMethod::Scoop);
    }

    #[test]
    fn detect_install_method_recognizes_scoop_shim_on_path() {
        for path in [
            "C:\\Users\\me\\scoop\\shims\\duodiff.exe",
            "C:/Users/me/scoop/shims/duodiff.exe",
        ] {
            let method = detect_install_method(Path::new(path));
            assert_eq!(method, InstallMethod::Scoop, "path: {path}");
            assert_eq!(upgrade_hint(&method), Some("scoop update duodiff"));
        }
    }

    #[test]
    fn scoop_install_path_matches_apps_and_shims() {
        assert!(scoop_install_path(
            "C:\\Users\\me\\scoop\\apps\\duodiff\\1.0.0\\duodiff.exe"
        ));
        assert!(scoop_install_path("C:/Users/me/scoop/shims/duodiff.exe"));
        assert!(!scoop_install_path("C:/Users/me/.local/bin/duodiff.exe"));
    }

    #[test]
    fn looks_managed_system_path_flags_common_locations() {
        assert!(looks_managed_system_path("/usr/bin/duodiff"));
        assert!(looks_managed_system_path(
            "C:\\Program Files\\duodiff\\duodiff.exe"
        ));
        assert!(!looks_managed_system_path("/Users/me/.local/bin/duodiff"));
    }

    #[test]
    fn detect_install_method_treats_local_bin_as_standalone() {
        let method = detect_install_method(Path::new("/Users/me/.local/bin/duodiff"));
        assert_eq!(method, InstallMethod::Standalone);
    }

    #[test]
    fn extract_binary_reads_tar_gz_layout() {
        let dir = tempdir().unwrap();
        let archive_path = dir.path().join("asset.tar.gz");
        {
            let file = fs::File::create(&archive_path).unwrap();
            let enc = flate2::write::GzEncoder::new(file, flate2::Compression::default());
            let mut builder = tar::Builder::new(enc);
            let mut header = tar::Header::new_gnu();
            header
                .set_path("duodiff-v0.1.0-x86_64-apple-darwin/duodiff")
                .unwrap();
            header.set_size(6);
            header.set_cksum();
            builder.append(&header, b"BINARY".as_slice()).unwrap();
            builder.into_inner().unwrap().finish().unwrap();
        }
        let bytes = fs::read(&archive_path).unwrap();
        // Use a fixed tar.gz platform so this test is not host-dependent
        // (Windows hosts would otherwise try to parse the payload as zip).
        let platform = Platform {
            target: "x86_64-apple-darwin".to_string(),
            archive_ext: "tar.gz",
            bin_name: "duodiff".to_string(),
        };
        let asset = release_asset("0.1.0", &platform);
        assert_eq!(
            extract_binary(&bytes, &asset, &platform).unwrap(),
            b"BINARY"
        );
    }

    #[test]
    fn execute_plan_check_only_exits_when_update_available() {
        let dir = tempdir().unwrap();
        let exe = dir.path().join("duodiff");
        fs::write(&exe, b"old").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let platform = Platform {
            target: "x86_64-apple-darwin".to_string(),
            archive_ext: "tar.gz",
            bin_name: "duodiff".to_string(),
        };
        let asset = release_asset("99.0.0", &platform);
        let archive_url = format!(
            "{}/{}/{}",
            asset.download_base, asset.version, asset.archive_name
        );
        let checksum_url = format!("{archive_url}.sha256");

        let mut tar_bytes = Vec::new();
        {
            let enc = flate2::write::GzEncoder::new(&mut tar_bytes, flate2::Compression::default());
            let mut builder = tar::Builder::new(enc);
            let payload = b"NEW".to_vec();
            let mut header = tar::Header::new_gnu();
            header
                .set_path(format!("{}/duodiff", asset.pkg_name))
                .unwrap();
            header.set_size(3);
            header.set_cksum();
            builder.append(&header, &payload[..]).unwrap();
            builder.into_inner().unwrap().finish().unwrap();
        }
        let hash = sha256_hex(&tar_bytes);

        let client = FakeClient {
            latest: "v99.0.0".to_string(),
            files: std::collections::HashMap::from([
                (archive_url.clone(), tar_bytes),
                (
                    checksum_url,
                    format!("{hash}  {}\n", asset.archive_name).into_bytes(),
                ),
            ]),
        };

        let plan = UpgradePlan {
            exe_path: exe.clone(),
            method: InstallMethod::Standalone,
            current_version: env!("CARGO_PKG_VERSION").to_string(),
            target_version: String::new(),
            asset: ReleaseAsset {
                version: String::new(),
                pkg_name: String::new(),
                archive_name: String::new(),
                download_base: String::new(),
            },
            check_only: true,
        };

        assert_eq!(
            execute_plan(&plan, &client).unwrap(),
            ExecuteOutcome::UpdateAvailable
        );
    }

    #[test]
    fn execute_plan_replaces_standalone_binary_on_unix() {
        if std::env::consts::OS == "windows" {
            return;
        }
        let dir = tempdir().unwrap();
        let exe = dir.path().join("duodiff");
        fs::write(&exe, b"old").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let platform = detect_platform().unwrap();
        let asset = release_asset("99.0.0", &platform);
        let archive_url = format!(
            "{}/{}/{}",
            asset.download_base, asset.version, asset.archive_name
        );
        let checksum_url = format!("{archive_url}.sha256");

        let mut tar_bytes = Vec::new();
        {
            let enc = flate2::write::GzEncoder::new(&mut tar_bytes, flate2::Compression::default());
            let mut builder = tar::Builder::new(enc);
            let payload = b"NEW";
            let mut header = tar::Header::new_gnu();
            header
                .set_path(format!("{}/{}", asset.pkg_name, platform.bin_name))
                .unwrap();
            header.set_size(3);
            header.set_cksum();
            builder.append(&header, &payload[..]).unwrap();
            builder.into_inner().unwrap().finish().unwrap();
        }
        let hash = sha256_hex(&tar_bytes);

        let client = FakeClient {
            latest: "v99.0.0".to_string(),
            files: std::collections::HashMap::from([
                (archive_url, tar_bytes),
                (
                    checksum_url,
                    format!("{hash}  {}\n", asset.archive_name).into_bytes(),
                ),
            ]),
        };

        let plan = UpgradePlan {
            exe_path: exe.clone(),
            method: InstallMethod::Standalone,
            current_version: "0.0.1".to_string(),
            target_version: String::new(),
            asset: ReleaseAsset {
                version: String::new(),
                pkg_name: String::new(),
                archive_name: String::new(),
                download_base: String::new(),
            },
            check_only: false,
        };

        execute_plan(&plan, &client).unwrap();
        assert_eq!(fs::read(&exe).unwrap(), b"NEW");
    }

    #[test]
    fn should_check_respects_the_daily_interval() {
        assert!(should_check(0, CHECK_INTERVAL_SECS));
        assert!(should_check(0, CHECK_INTERVAL_SECS + 10));
        assert!(!should_check(100, 100));
        assert!(!should_check(100, 100 + CHECK_INTERVAL_SECS - 1));
    }

    #[test]
    fn is_newer_only_for_strictly_greater() {
        assert_eq!(is_newer("v0.2.0", "0.1.0"), Some("0.2.0".to_string()));
        assert_eq!(is_newer("0.1.0", "0.1.0"), None);
        assert_eq!(is_newer("v0.0.9", "0.1.0"), None);
        assert_eq!(is_newer("", "0.1.0"), None);
    }

    #[test]
    fn update_hint_is_install_method_aware() {
        assert_eq!(
            update_hint("0.2.0", &InstallMethod::Homebrew),
            "⬆ v0.2.0 available — run brew upgrade duodiff"
        );
        assert_eq!(
            update_hint("0.2.0", &InstallMethod::Standalone),
            "⬆ v0.2.0 available — run duodiff --upgrade"
        );
        assert_eq!(
            update_hint("0.2.0", &InstallMethod::Refuse { hint: "x".into() }),
            "⬆ v0.2.0 available"
        );
    }

    #[test]
    fn check_classifies_outcomes_via_a_fake_client() {
        struct Fake {
            tag: Option<&'static str>,
        }
        impl ReleaseClient for Fake {
            fn fetch_latest_tag(&self) -> Result<String> {
                self.tag
                    .map(str::to_string)
                    .ok_or_else(|| anyhow::anyhow!("offline"))
            }
            fn download(&self, _url: &str) -> Result<Vec<u8>> {
                unreachable!("update check never downloads")
            }
        }
        assert_eq!(
            check(
                &Fake {
                    tag: Some("v0.2.0")
                },
                "0.1.0"
            ),
            UpdateCheckOutcome::Newer("0.2.0".to_string())
        );
        assert_eq!(
            check(
                &Fake {
                    tag: Some("v0.1.0")
                },
                "0.1.0"
            ),
            UpdateCheckOutcome::UpToDate
        );
        assert_eq!(
            check(&Fake { tag: None }, "0.1.0"),
            UpdateCheckOutcome::Failed
        );
    }

    #[test]
    fn state_round_trips_through_disk() {
        let dir = std::env::temp_dir().join(format!("duodiff-uctest-{}", std::process::id()));
        let path = dir.join("update_check.json");
        let state = UpdateCheckState {
            last_check: 12345,
            latest_seen: "0.2.0".to_string(),
        };
        save_state(&path, &state);
        assert_eq!(load_state(&path), state);
        // Missing file loads the default.
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(load_state(&path), UpdateCheckState::default());
    }
}
