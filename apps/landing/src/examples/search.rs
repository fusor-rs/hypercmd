use super::files::{FILES, ProjectFile};
use fusor::{FromInputs, Signal, signal};

#[derive(FromInputs)]
pub(crate) struct Search {
    #[local(init = signal(String::from(".html")))]
    query: Signal<String>,
}

impl Search {
    fn matches(&self) -> Vec<ProjectFile> {
        let query = self.query.get().to_lowercase();
        FILES
            .iter()
            .filter(|file| file.path.contains(&query))
            .copied()
            .collect()
    }
}

fusor::template!(backend = "hypercmd", "ui/search.html");
