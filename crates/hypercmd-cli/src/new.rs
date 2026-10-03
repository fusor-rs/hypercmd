//! `hypercmd new`: a complete starter app, checked before it is handed over.
use crate::Result;
use std::{
    fs,
    path::{Path, PathBuf},
};
use toml::{Table, Value};

/// The fusor release this hypercmd is built against; a test keeps it in sync.
pub(crate) const FUSOR_VERSION: &str = "0.1.4";

const BUILD_RS: &str = "fn main() -> Result<(), Box<dyn std::error::Error>> {
    hypercmd_build::compile_app()
}
";

pub(crate) fn run(path: &Path, checkout: Option<&Path>) -> Result {
    if path.exists() {
        return Err(format!("{} already exists; choose a new directory", path.display()).into());
    }
    let manifest = manifest(&package_name(path)?, checkout)?;
    fs::create_dir_all(path.join("src"))?;
    fs::create_dir_all(path.join("ui"))?;
    for (file, text) in [
        ("Cargo.toml", manifest.as_str()),
        ("build.rs", BUILD_RS),
        ("src/main.rs", include_str!("../template/main.rs")),
        ("ui/app.html", include_str!("../template/app.html")),
        ("ui/terminal.css", include_str!("../template/terminal.css")),
        (".gitignore", "/target\n/dist\n"),
    ] {
        fs::write(path.join(file), text)?;
    }
    println!(
        "Created {}. Checking that it builds; the first check downloads and compiles dependencies.",
        path.display()
    );
    crate::cargo::compile(
        "check",
        [
            "--manifest-path".as_ref(),
            path.join("Cargo.toml").as_os_str(),
        ],
    )?;
    println!(
        "\nYour app is ready:\n\n  cd {}\n  hypercmd run\n\nEdit ui/app.html for the screen and src/main.rs for its state.",
        path.display()
    );
    Ok(())
}

/// The directory name becomes the package and program name.
fn package_name(path: &Path) -> Result<String> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let usable = name.starts_with(|first: char| first.is_ascii_lowercase())
        && name.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '-' | '_')
        })
        && !["test", "core", "std", "alloc", "self", "super", "crate"].contains(&name);
    if !usable {
        return Err(format!(
            "{name:?} cannot name an app; use lowercase letters, digits, '-' or '_', starting with a letter"
        )
        .into());
    }
    Ok(name.to_owned())
}

fn manifest(name: &str, checkout: Option<&Path>) -> Result<String> {
    let local = |member: &str| {
        dependency(
            env!("CARGO_PKG_VERSION"),
            checkout.map(|root| root.join("crates").join(member)),
        )
    };
    let (runtime, build) = (local("hypercmd")?, local("hypercmd-build")?);
    Ok(format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2024"
rust-version = "1.85"

[workspace]

[dependencies]
hypercmd = {runtime}
fusor = {{ package = "fusor-core", version = "={FUSOR_VERSION}", default-features = false }}
fusor-components = {{ version = "={FUSOR_VERSION}", default-features = false }}

[build-dependencies]
hypercmd-build = {build}

[package.metadata.hypercmd]
entry = "ui/app.html"
templates = ["ui"]
styles = ["ui/terminal.css"]
"#
    ))
}

/// An exact version, optionally from a local path. Rendered through TOML so paths are escaped.
fn dependency(version: &str, path: Option<PathBuf>) -> Result<String> {
    let mut fields = Table::new();
    fields.insert("version".into(), format!("={version}").into());
    if let Some(path) = path {
        let path = path.canonicalize()?;
        fields.insert(
            "path".into(),
            path.to_str().ok_or("checkout paths must be UTF-8")?.into(),
        );
    }
    Ok(Value::Table(fields).to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn starter_uses_the_workspace_fusor_release() {
        let workspace: toml::Table = include_str!("../../../Cargo.toml")
            .parse()
            .expect("workspace manifest");
        let fusor = &workspace["workspace"]["dependencies"]["fusor"]["version"];
        assert_eq!(fusor.as_str(), Some(&*format!("={}", super::FUSOR_VERSION)));
    }
}
