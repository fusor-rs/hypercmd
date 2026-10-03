//! Package-owned job controls with independent, additive rendering features.
#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    pub id: u32,
    pub name: String,
    pub progress: u32,
    pub cancelled: bool,
}

impl Job {
    pub fn new(id: u32, progress: u32) -> Self {
        Self {
            id,
            name: format!("Job {id:02} — 東京 👩‍💻"),
            progress,
            cancelled: false,
        }
    }
}

#[cfg(any(feature = "terminal", feature = "browser"))]
mod row {
    use super::Job;
    use fusor::{FromInputs, Memo, Signal, signal};
    use std::rc::Rc;

    /// A keyed row with private inspection state and application-owned job data.
    #[derive(FromInputs)]
    pub struct JobRow {
        #[input]
        job: Memo<Job>,
        #[input]
        jobs: Signal<Vec<Job>>,
        #[input]
        inspect: Rc<dyn Fn(u32)>,
        #[local(init = signal(0))]
        inspections: Signal<u32>,
    }

    impl JobRow {
        fn inspect(&self) {
            self.inspections.update(|n| *n += 1);
            (self.inspect)(self.job.get().id);
        }

        fn cancel(&self) {
            let id = self.job.get().id;
            self.jobs.update(|jobs| {
                if let Some(job) = jobs.iter_mut().find(|job| job.id == id) {
                    job.cancelled = true;
                }
            });
        }

        fn remove(&self) {
            let id = self.job.get().id;
            self.jobs.update(|jobs| jobs.retain(|job| job.id != id));
        }
    }

    #[cfg(feature = "terminal")]
    fusor::template!(backend = "hypercmd", "ui/job.html");
    #[cfg(feature = "browser")]
    #[expect(
        clippy::too_many_lines,
        clippy::excessive_nesting,
        clippy::redundant_clone,
        reason = "fusor-build 0.1.4's DOM codegen leaves these unsuppressed in generated mount code"
    )]
    mod dom {
        use super::JobRow;
        fusor::template!("ui/job.html");
    }
}
#[cfg(any(feature = "terminal", feature = "browser"))]
pub use row::JobRow;

/// Embed in the browser document's stylesheet; no package-source lookup is needed.
#[cfg(feature = "browser")]
pub const BROWSER_CSS: &str = include_str!("../ui/browser.css");
