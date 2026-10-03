use std::{
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

pub const MAX_ENTRIES: usize = 1_000;
pub const PREVIEW_BYTES: usize = 64 * 1024;

#[derive(Default)]
pub struct Directory {
    pub entries: Vec<Entry>,
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub directory: bool,
    pub supported: bool,
}

pub struct Preview {
    pub text: String,
    pub truncated: bool,
}

impl Entry {
    pub fn label(&self) -> String {
        format!(
            "{}{}",
            self.name,
            if self.directory {
                "/"
            } else if !self.supported {
                " [unsupported]"
            } else {
                ""
            }
        )
    }
}

pub fn read_directory(path: &Path, cancelled: &AtomicBool) -> Result<Directory, String> {
    check_cancelled(cancelled)?;
    let at = |error| format!("{}: {error}", path.display());
    let metadata = fs::symlink_metadata(path).map_err(at)?;
    if !metadata.is_dir() {
        return Err("Choose a folder; symbolic links are not followed.".into());
    }
    let source = fs::read_dir(path).map_err(at)?;
    let mut directory = Directory::default();
    for entry in source {
        check_cancelled(cancelled)?;
        let entry = entry.map_err(at)?;
        if directory.entries.len() == MAX_ENTRIES {
            directory.truncated = true;
            break;
        }
        let kind = entry
            .file_type()
            .map_err(|error| format!("{}: {error}", entry.path().display()))?;
        directory.entries.push(Entry {
            path: entry.path(),
            name: entry.file_name().to_string_lossy().into_owned(),
            directory: kind.is_dir(),
            supported: kind.is_dir() || kind.is_file(),
        });
    }
    directory
        .entries
        .sort_by(|left, right| (!left.directory, &left.path).cmp(&(!right.directory, &right.path)));
    check_cancelled(cancelled)?;
    Ok(directory)
}

pub fn read_preview(path: &Path, cancelled: &AtomicBool) -> Result<Preview, String> {
    check_cancelled(cancelled)?;
    let at = |error| format!("{}: {error}", path.display());
    let file = open_preview(path).map_err(at)?;
    if !file.metadata().map_err(at)?.is_file() {
        return Err("Only regular text files can be previewed.".into());
    }
    let mut bytes = Vec::new();
    file.take((PREVIEW_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(at)?;
    check_cancelled(cancelled)?;
    let truncated = bytes.len() > PREVIEW_BYTES;
    bytes.truncate(PREVIEW_BYTES);
    let text = match std::str::from_utf8(&bytes) {
        Ok(text) => text,
        Err(error) if truncated && error.error_len().is_none() => {
            std::str::from_utf8(&bytes[..error.valid_up_to()]).expect("validated UTF-8 prefix")
        }
        Err(_) => {
            return Err("Preview requires UTF-8 text; this file has another encoding.".into());
        }
    };
    if text.contains('\0') {
        return Err("Binary files cannot be previewed.".into());
    }
    Ok(Preview {
        text: text.to_owned(),
        truncated,
    })
}

#[cfg(unix)]
fn open_preview(path: &Path) -> io::Result<File> {
    use rustix::fs::{Mode, OFlags, open};

    // A listed file may become a FIFO or link before it is opened.
    let flags = OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK;
    Ok(File::from(open(path, flags, Mode::empty())?))
}

#[cfg(not(unix))]
fn open_preview(_path: &Path) -> io::Result<File> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "This demo supports native Unix file previews.",
    ))
}

fn check_cancelled(cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::Relaxed) {
        Err("File operation cancelled.".into())
    } else {
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{
        ffi::OsString,
        os::unix::{ffi::OsStringExt, fs::symlink},
    };

    #[test]
    fn bounded_read_only_loading_preserves_paths_and_rejects_non_text_sources() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        fs::create_dir(root.join("z-folder")).unwrap();
        fs::write(root.join("a.txt"), "Hello, 世界\n").unwrap();
        let unusual = root.join(if cfg!(target_os = "linux") {
            OsString::from_vec(b"z-\xff.txt".to_vec())
        } else {
            OsString::from("z-日本語.txt")
        });
        fs::write(&unusual, "Exact path").unwrap();
        symlink(root.join("a.txt"), root.join("link")).unwrap();
        assert!(
            std::process::Command::new("mkfifo")
                .arg(root.join("pipe"))
                .status()
                .unwrap()
                .success()
        );
        let cancelled = AtomicBool::new(false);
        let listing = read_directory(root, &cancelled).unwrap();
        assert!(!listing.truncated);
        assert_eq!(listing.entries[0].name, "z-folder");
        assert!(listing.entries[0].directory);
        assert_eq!(listing.entries[1].name, "a.txt");
        for entry in &listing.entries[2..4] {
            assert!(!entry.supported);
            assert!(read_preview(&entry.path, &cancelled).is_err());
        }
        assert_eq!(listing.entries[4].path, unusual);
        assert_eq!(
            read_preview(&unusual, &cancelled).unwrap().text,
            "Exact path"
        );
        assert_eq!(
            read_preview(&root.join("a.txt"), &cancelled).unwrap().text,
            "Hello, 世界\n"
        );

        let large = root.join("large.txt");
        fs::write(&large, format!("{}世界", "a".repeat(PREVIEW_BYTES - 1))).unwrap();
        let preview = read_preview(&large, &cancelled).unwrap();
        assert!(preview.truncated);
        assert_eq!(preview.text, "a".repeat(PREVIEW_BYTES - 1));
        for bytes in [&b"binary\0data"[..], &b"invalid\xff"[..]] {
            fs::write(&large, bytes).unwrap();
            assert!(read_preview(&large, &cancelled).is_err());
        }
        for index in 0..MAX_ENTRIES {
            fs::write(root.join(format!("entry-{index}")), []).unwrap();
        }
        let listing = read_directory(root, &cancelled).unwrap();
        assert!(listing.truncated);
        assert_eq!(listing.entries.len(), MAX_ENTRIES);
        cancelled.store(true, Ordering::Relaxed);
        assert!(read_directory(root, &cancelled).is_err());
        assert!(read_preview(&large, &cancelled).is_err());
    }
}
