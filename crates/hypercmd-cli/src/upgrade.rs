use crate::Result;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::{
    env,
    ffi::OsStr,
    fs,
    io::{self, ErrorKind, IsTerminal},
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const NOTIFICATION_TIMEOUT: Duration = Duration::from_secs(2);
const UPGRADE_TIMEOUT: Duration = Duration::from_secs(30);
// The release workflow uploads this only after binaries and crates are published.
const READY_ASSET: &str = "install.sh";

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
}

#[derive(Deserialize, Serialize)]
struct VersionCheck {
    checked_at: u64,
    available: Option<Version>,
}

pub(crate) fn run() -> Result {
    let current = Version::parse(CURRENT_VERSION)?;
    let Some(version) = latest(&current, UPGRADE_TIMEOUT)? else {
        println!("hypercmd {current} is already up to date.");
        return Ok(());
    };
    let executable = fs::canonicalize(env::current_exe()?)?;
    println!("Upgrading hypercmd {current} → {version}");
    let status = match cargo_installation(&executable)? {
        Some(root) => Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
            .args(["install", env!("CARGO_PKG_NAME"), "--locked", "--version"])
            .arg(format!("={version}"))
            .arg("--root")
            .arg(root)
            .status()?,
        None => installer()
            .arg(version.to_string())
            .env("HYPERCMD_BIN", &executable)
            .status()?,
    };
    if !status.success() {
        return Err(
            format!("upgrade failed ({status}); see the messages above and try again").into(),
        );
    }
    Ok(())
}

pub(crate) fn notify() {
    if env::var_os("CI").is_some() || !io::stderr().is_terminal() || !io::stdout().is_terminal() {
        return;
    }
    // An unavailable cache must not prevent the user's command from running.
    if let Ok(Some(version)) = notification() {
        eprintln!("⚠️ New version available: {CURRENT_VERSION} → {version}. Run hypercmd upgrade");
    }
}

fn notification() -> Result<Option<Version>> {
    let current = Version::parse(CURRENT_VERSION)?;
    let directory = env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .ok_or("no home or cache directory is configured")?
        .join("hypercmd");
    let path = directory.join("version.json");
    let previous = match fs::read(&path) {
        Ok(bytes) => {
            // A corrupt cache is disposable; the next check reconstructs it.
            serde_json::from_slice::<VersionCheck>(&bytes).ok()
        }
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let recent = previous.as_ref().is_some_and(|check| {
        now.checked_sub(check.checked_at)
            .is_some_and(|age| age < CHECK_INTERVAL.as_secs())
    });
    let available = if recent {
        previous.and_then(|check| check.available)
    } else {
        fs::create_dir_all(&directory)?;
        let cache = tempfile::NamedTempFile::new_in(&directory)?;
        let available = match latest(&current, NOTIFICATION_TIMEOUT) {
            Ok(available) => available,
            // Offline and rate-limited checks retain the last known release.
            Err(_) => previous.and_then(|check| check.available),
        };
        let check = VersionCheck {
            checked_at: now,
            available,
        };
        serde_json::to_writer(&cache, &check)?;
        cache.persist(path)?;
        check.available
    };
    Ok(available.filter(|version| version.cmp_precedence(&current).is_gt()))
}

fn latest(current: &Version, timeout: Duration) -> Result<Option<Version>> {
    let repository = env!("CARGO_PKG_REPOSITORY")
        .strip_prefix("https://github.com/")
        .expect("the workspace repository is hosted on GitHub");
    let url = env::var_os("HYPERCMD_RELEASE_URL").unwrap_or_else(|| {
        format!("https://api.github.com/repos/{repository}/releases/latest").into()
    });
    let output = Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--max-time",
        ])
        .arg(timeout.as_secs().to_string())
        .arg(url)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "could not check for a release; try again later: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    let release: Release = serde_json::from_slice(&output.stdout)?;
    let version = Version::parse(release.tag_name.trim_start_matches('v'))?;
    if release.draft || release.prerelease || !version.pre.is_empty() {
        return Err("the latest release is not stable; try again after it is published".into());
    }
    if !version.cmp_precedence(current).is_gt() {
        return Ok(None);
    }
    let output = installer()
        .arg("--archive")
        .arg(version.to_string())
        .output()?;
    if !output.status.success() {
        return Err(String::from_utf8(output.stderr)?.into());
    }
    let archive = String::from_utf8(output.stdout)?;
    for required in [
        READY_ASSET,
        archive.trim(),
        &format!("{}.sha256", archive.trim()),
    ] {
        if !release.assets.iter().any(|asset| asset.name == required) {
            return Err(
                format!("release {version} is still being published; try again later").into(),
            );
        }
    }
    Ok(Some(version))
}

fn installer() -> Command {
    let mut command = Command::new("sh");
    command.args(["-c", include_str!("../install.sh"), "hypercmd-installer"]);
    command
}

fn cargo_installation(executable: &Path) -> Result<Option<&Path>> {
    let Some(directory) = executable.parent().filter(|path| path.ends_with("bin")) else {
        return Ok(None);
    };
    let root = directory
        .parent()
        .ok_or("the installation has no parent directory")?;
    if !root.join(".crates.toml").try_exists()? {
        return Ok(None);
    }
    let output = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .args(["install", "--list", "--root"])
        .arg(root)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "could not inspect the Cargo installation: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    let package = format!("{} v", env!("CARGO_PKG_NAME"));
    let listing = String::from_utf8(output.stdout)?;
    let mut lines = listing.lines();
    if !lines.any(|line| line.starts_with(&package)) {
        return Ok(None);
    }
    Ok(lines
        .take_while(|line| line.starts_with(' '))
        .any(|line| Some(OsStr::new(line.trim())) == executable.file_name())
        .then_some(root))
}
