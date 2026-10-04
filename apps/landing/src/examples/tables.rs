use super::files::{FILES, ProjectFile};
use fusor::{FromInputs, Signal, signal};

#[derive(FromInputs)]
pub(crate) struct Tables {
    #[local(init = signal(false))]
    largest_first: Signal<bool>,
}

impl Tables {
    fn rows(&self) -> Vec<ProjectFile> {
        let mut files: Vec<_> = FILES
            .iter()
            .filter(|file| file.path.ends_with(".html"))
            .copied()
            .collect();
        if self.largest_first.get() {
            files.sort_by_key(|file| std::cmp::Reverse(file.bytes));
        }
        files
    }
}

fusor::template!(backend = "hypercmd", "ui/tables.html");
