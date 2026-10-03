mod files;

use files::{Directory, Entry, Preview};
use fusor::{FromInputs, OwnerHandle, Signal, signal};
use fusor_async::{AsyncBoundary, AsyncValue, BoundaryStatus};
use hypercmd::{Error, History, Services};
use std::{
    path::{Path, PathBuf},
    rc::Rc,
    sync::atomic::AtomicBool,
};

struct Browser {
    path: Signal<PathBuf>,
    filter: Signal<String>,
    selected: Signal<PathBuf>,
    open: Rc<dyn Fn(Entry) -> Result<(), Error>>,
}

impl FromInputs for Browser {
    type Inputs = PathBuf;
    type Error = Error;

    fn from_inputs(initial: PathBuf, owner: OwnerHandle) -> Result<Self, Error> {
        let path = signal(initial.clone());
        let filter = signal(String::new());
        let selected = signal(initial);
        let history = History::install(&owner, "/")?;
        let (folder, query, file) = (path.clone(), filter.clone(), selected.clone());
        let navigate = history;
        let open = Rc::new(move |entry: Entry| {
            if !entry.supported {
                return Ok(());
            }
            if entry.directory {
                query.set(String::new());
                folder.set(entry.path);
            } else {
                file.set(entry.path);
                navigate.push("/preview")?;
            }
            Ok(())
        });
        Ok(Self {
            path,
            filter,
            selected,
            open,
        })
    }
}

struct DirectoryView {
    path: Signal<PathBuf>,
    filter: Signal<String>,
    open: Rc<dyn Fn(Entry) -> Result<(), Error>>,
    revision: Signal<u64>,
    boundary: AsyncBoundary,
    listing: AsyncValue<(PathBuf, u64), Directory, String>,
}

struct DirectoryViewInputs {
    path: Signal<PathBuf>,
    filter: Signal<String>,
    open: Rc<dyn Fn(Entry) -> Result<(), Error>>,
}

impl FromInputs for DirectoryView {
    type Inputs = DirectoryViewInputs;
    type Error = Error;

    fn from_inputs(inputs: Self::Inputs, owner: OwnerHandle) -> Result<Self, Error> {
        let revision = signal(0);
        let (path, refresh) = (inputs.path.clone(), revision.clone());
        let listing = worker_read(
            &owner,
            move || (path.get(), refresh.get()),
            files::read_directory,
        )?;
        Ok(Self {
            path: inputs.path,
            filter: inputs.filter,
            open: inputs.open,
            revision,
            boundary: AsyncBoundary::coherent(),
            listing,
        })
    }
}

impl DirectoryView {
    fn visible(&self, listing: &Directory) -> Vec<Entry> {
        let filter = self.filter.get().to_lowercase();
        listing
            .entries
            .iter()
            .filter(|entry| entry.name.to_lowercase().contains(&filter))
            .cloned()
            .collect()
    }

    fn status(&self) -> String {
        status(&self.boundary, "Loading folder…", "Cannot read folder")
    }

    fn up(&self) {
        if let Some(parent) = self.path.get().parent() {
            self.filter.set(String::new());
            self.path.set(parent.to_path_buf());
        }
    }
}

struct FilePreview {
    path: PathBuf,
    history: History,
    revision: Signal<u64>,
    boundary: AsyncBoundary,
    contents: AsyncValue<(PathBuf, u64), Preview, String>,
}

struct FilePreviewInputs {
    path: PathBuf,
}

impl FromInputs for FilePreview {
    type Inputs = FilePreviewInputs;
    type Error = Error;

    fn from_inputs(inputs: Self::Inputs, owner: OwnerHandle) -> Result<Self, Error> {
        let revision = signal(0);
        let (path, refresh) = (inputs.path.clone(), revision.clone());
        let contents = worker_read(
            &owner,
            move || (path.clone(), refresh.get()),
            files::read_preview,
        )?;
        Ok(Self {
            path: inputs.path,
            history: History::from_owner(&owner).expect("browser history is installed"),
            revision,
            boundary: AsyncBoundary::coherent(),
            contents,
        })
    }
}

impl FilePreview {
    fn status(&self) -> String {
        status(&self.boundary, "Reading file…", "Cannot preview")
    }
}

fn worker_read<T: Send + 'static>(
    owner: &OwnerHandle,
    key: impl Fn() -> (PathBuf, u64) + 'static,
    read: fn(&Path, &AtomicBool) -> Result<T, String>,
) -> Result<AsyncValue<(PathBuf, u64), T, String>, Error> {
    let services = Services::from_owner(owner)?;
    let workers = services.clone();
    Ok(AsyncValue::new(
        owner,
        key,
        move |(path, _), cancel| {
            let workers = workers.clone();
            async move {
                workers
                    .worker(cancel, move |cancel| read(&path, &cancel))
                    .map_err(|error| error.to_string())?
                    .await
                    .map_err(|error| error.to_string())?
            }
        },
        services.spawner(),
    ))
}
fn status(boundary: &AsyncBoundary, pending: &str, failed: &str) -> String {
    match boundary.status() {
        BoundaryStatus::Pending => pending.into(),
        BoundaryStatus::Error(error) | BoundaryStatus::Faulted(error) => {
            format!("{failed}: {error}")
        }
        _ => String::new(),
    }
}

fusor::template!(backend = "hypercmd", "ui/browser.html");

const USAGE: &str = "Usage: file-browser [DIRECTORY]";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let path = args.next().unwrap_or_else(|| ".".into());
    if path == "--help" || path == "-h" {
        println!("{USAGE}\nBrowse folders and preview UTF-8 text. Ctrl+C exits.");
        return Ok(());
    }
    if args.next().is_some() {
        return Err(USAGE.into());
    }
    let path = std::fs::canonicalize(path)?;
    if !path.is_dir() {
        return Err("The starting path must be a directory.".into());
    }
    hypercmd::native::run(hypercmd::mount::<Browser>(path)?)?;
    Ok(())
}
