//! Compile package-owned HTML with `[package.metadata.hypercmd]`.
//!
//! `entry` names an optional application template, `templates` lists discovery
//! directories, and `profile` defaults to `terminal-v1`. Generated implementations
//! are included with `fusor::template!(backend = "hypercmd", "ui/view.html")`.
//! Each component library compiles its own templates in its owning Rust modules.

mod backend;
mod css;
use backend::HypercmdBackend;
pub use backend::profile;

use fusor_build::backend::{build::BuildInputs, build::compile_cargo, generate};
use serde::Deserialize;
use std::{
    collections::BTreeSet,
    env,
    error::Error,
    fs,
    hash::{Hash, Hasher},
    path::{Component, Path, PathBuf},
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Deserialize)]
struct Manifest {
    package: Package,
}

#[derive(Deserialize)]
struct Package {
    metadata: Metadata,
}

#[derive(Deserialize)]
struct Metadata {
    hypercmd: Config,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    entry: Option<PathBuf>,
    #[serde(default)]
    templates: Vec<PathBuf>,
    #[serde(default)]
    styles: Vec<PathBuf>,
    profile: Option<String>,
}

/// Compile Hypercmd templates from Cargo's current package.
///
/// Plain Cargo reports later Rust errors against generated files. Run
/// `hypercmd check` to additionally display authored HTML locations.
pub fn compile_app() -> Result<()> {
    const _: () = assert!(fusor_build::backend::build::OUTPUT_VERSION == 1);
    let root = env::var_os("CARGO_MANIFEST_DIR").ok_or("compile_app must run from build.rs")?;
    let root = Path::new(&root).canonicalize()?;
    let manifest_path = root.join("Cargo.toml");
    println!("cargo::rerun-if-changed={}", manifest_path.display());
    let Manifest { package } = toml::from_str(&fs::read_to_string(&manifest_path)?)?;
    let config = package.metadata.hypercmd;
    if let Some(profile) = config
        .profile
        .as_deref()
        .filter(|name| *name != profile::NAME)
    {
        return Err(format!(
            "unsupported Hypercmd profile {profile:?}; use {:?}",
            profile::NAME
        )
        .into());
    }
    let backend = HypercmdBackend {
        styles: styles(&root, &config)?,
        file: std::cell::Cell::default(),
    };
    let inputs = BuildInputs::new(&root, discover(&root, &config)?)?;
    compile_cargo(&inputs, "hypercmd", |path, html| {
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        path.path().hash(&mut hash);
        backend.file.set(hash.finish());
        generate(html, &backend)
    })
    .map(|_| ())
}

fn styles(root: &Path, config: &Config) -> Result<proc_macro2::TokenStream> {
    let mut rules = Vec::new();
    let mut sources = BTreeSet::new();
    for relative in &config.styles {
        let path = watched(root, relative)?;
        if !sources.insert(path.clone()) {
            return Err(format!(
                "stylesheet registered more than once: {}",
                relative.display()
            )
            .into());
        }
        let source = fs::read_to_string(&path)?;
        rules.extend(css::parse(&source).map_err(|(offset, message)| {
            let before = &source[..offset];
            let line = before.bytes().filter(|byte| *byte == b'\n').count() + 1;
            let column = before
                .rsplit('\n')
                .next()
                .unwrap_or_default()
                .chars()
                .count()
                + 1;
            format!("{}:{line}:{column}: {message}", path.display())
        })?);
    }
    Ok(quote::quote!(&[#(#rules),*]))
}

fn package_path(root: &Path, path: &Path) -> Result<PathBuf> {
    if path.as_os_str().is_empty()
        || path.to_str().is_none()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(format!(
            "expected a package-relative path without '..': {}",
            path.display()
        )
        .into());
    }
    let canonical = root.join(path).canonicalize()?;
    if !canonical.starts_with(root) {
        return Err(format!("path escapes the package: {}", path.display()).into());
    }
    Ok(canonical)
}

fn watched(root: &Path, relative: &Path) -> Result<PathBuf> {
    let path = package_path(root, relative)?;
    println!("cargo::rerun-if-changed={}", path.display());
    if path != root.join(relative) {
        println!("cargo::rerun-if-changed={}", root.join(relative).display());
    }
    Ok(path)
}

fn discover(root: &Path, config: &Config) -> Result<Vec<PathBuf>> {
    let mut sources = Vec::new();
    let mut directories = BTreeSet::new();
    for directory in &config.templates {
        discover_directory(root, directory, &mut directories, &mut sources)?;
    }
    if let Some(entry) = &config.entry {
        if entry
            .extension()
            .is_none_or(|extension| extension != "html")
        {
            return Err("Hypercmd entry must name an .html template".into());
        }
        if !sources.contains(entry) {
            sources.push(entry.clone());
        }
    }
    Ok(sources)
}

fn discover_directory(
    root: &Path,
    relative: &Path,
    visited: &mut BTreeSet<PathBuf>,
    sources: &mut Vec<PathBuf>,
) -> Result<()> {
    let directory = watched(root, relative)?;
    if !directory.is_dir() {
        return Err(format!(
            "template discovery requires a directory: {}",
            relative.display()
        )
        .into());
    }
    if !visited.insert(directory.clone()) {
        return Err(format!(
            "template directory discovered more than once or through a symlink cycle: {}",
            relative.display()
        )
        .into());
    }
    for entry in fs::read_dir(&directory)? {
        let path = root.join(relative).join(entry?.file_name());
        let relative = path.strip_prefix(root)?;
        if path.is_dir() {
            discover_directory(root, relative, visited, sources)?;
        } else if path
            .extension()
            .is_some_and(|extension| extension == "html")
        {
            sources.push(relative.to_owned());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_keeps_sources_local_unique_and_ordered() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir_all(directory.path().join("ui/nested")).unwrap();
        fs::write(directory.path().join("ui/z.html"), "z").unwrap();
        fs::write(directory.path().join("ui/nested/a.html"), "a").unwrap();
        let root = directory.path().canonicalize().unwrap();
        let mut config = Config {
            entry: Some("ui/z.html".into()),
            templates: vec!["ui".into()],
            styles: Vec::new(),
            profile: Some(profile::NAME.into()),
        };
        let sources = BuildInputs::new(&root, discover(&root, &config).unwrap()).unwrap();
        assert_eq!(
            sources
                .sources()
                .iter()
                .map(|source| source.path().to_owned())
                .collect::<Vec<_>>(),
            [
                PathBuf::from("ui/nested/a.html"),
                PathBuf::from("ui/z.html")
            ]
        );
        config.templates.push("ui/nested".into());
        assert!(
            discover(&root, &config)
                .unwrap_err()
                .to_string()
                .contains("more than once")
        );
        config.templates = vec!["../outside".into()];
        assert!(discover(&root, &config).is_err());

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(env::temp_dir(), root.join("outside")).unwrap();
            config.templates = vec!["outside".into()];
            assert!(
                discover(&root, &config)
                    .unwrap_err()
                    .to_string()
                    .contains("escapes")
            );
            std::os::unix::fs::symlink(root.join("ui/z.html"), root.join("ui/alias.html")).unwrap();
            config.templates = vec!["ui".into()];
            let sources = discover(&root, &config).unwrap();
            assert!(
                BuildInputs::new(&root, sources)
                    .unwrap_err()
                    .to_string()
                    .contains("more than once")
            );
        }
    }

    #[test]
    fn configuration_rejects_unknown_keys() {
        let manifest = "[package]\nname='example'\n[package.metadata.hypercmd]\nprofile='terminal-v1'\ntemplates=['ui']\nstyle=['ui/style.css']";
        let error = toml::from_str::<Manifest>(manifest)
            .err()
            .expect("unknown key must fail");
        assert!(error.to_string().contains("unknown field `style`"));
    }
}
