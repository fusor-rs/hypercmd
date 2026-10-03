use fusor::{FromInputs, Memo, OwnerHandle, Signal, memo, signal};
use fusor_async::{AsyncBoundary, AsyncValue, BoundaryStatus};
use hypercmd::{Error, History, Services};
use hypercmd_job_controls::{Job, JobRow};
use std::{cell::Cell, rc::Rc, time::Duration};

struct Inspector {
    filter: Signal<String>,
    jobs: Signal<Vec<Job>>,
    priority: Signal<u32>,
    notice: Signal<String>,
    inspect: Rc<dyn Fn(u32)>,
    visible: Memo<Vec<Job>>,
}

impl Inspector {
    fn new(owner: &OwnerHandle) -> Result<Self, Error> {
        let jobs = signal(
            (1..=24)
                .map(|id| Job::new(id, (id * 7) % 100))
                .collect::<Vec<_>>(),
        );
        let services = Services::from_owner(owner)?;
        let (progress, clock) = (jobs.clone(), services.clone());
        let notice = signal(String::new());
        let report = notice.clone();
        services.spawn(owner, async move {
            if let Err(error) = advance_jobs(progress, clock).await {
                report.set(error.to_string());
            }
        })?;
        let navigate = History::install(owner, "/")?;
        let report = notice.clone();
        let inspect = Rc::new(move |id| {
            if let Err(error) = navigate.push(&format!("/jobs/{id}")) {
                report.set(error.to_string());
            }
        });
        let filter = signal(String::new());
        let (query, entries) = (filter.clone(), jobs.clone());
        let visible = memo(move || {
            let query = query.get().to_lowercase();
            entries.with(|jobs| {
                jobs.iter()
                    .filter(|job| job.name.to_lowercase().contains(&query))
                    .cloned()
                    .collect()
            })
        });
        Ok(Self {
            filter,
            jobs,
            priority: signal(12),
            notice,
            inspect,
            visible,
        })
    }
}

struct JobDetails {
    id: u32,
    jobs: Signal<Vec<Job>>,
    history: History,
    boundary: AsyncBoundary,
    report: AsyncValue<u32, String, String>,
    revision: Signal<u32>,
    fail_next: Rc<Cell<bool>>,
}
struct JobDetailsInputs {
    id: String,
    jobs: Signal<Vec<Job>>,
}
impl FromInputs for JobDetails {
    type Inputs = JobDetailsInputs;
    type Error = Error;
    fn from_inputs(inputs: Self::Inputs, owner: OwnerHandle) -> Result<Self, Error> {
        let id = inputs
            .id
            .parse::<u32>()
            .map_err(|_| Error::new(hypercmd::ErrorKind::Construction, "invalid job identifier"))?;
        let services = Services::from_owner(&owner)?;
        let revision = signal(0);
        let key = revision.clone();
        let fail_next = Rc::new(Cell::new(false));
        let fail = fail_next.clone();
        let clock = services.clone();
        let jobs = inputs.jobs.clone();
        let report = AsyncValue::new(
            &owner,
            move || key.get(),
            move |_, _cancel| {
                let (clock, jobs) = (clock.clone(), jobs.clone());
                let fail = fail.replace(false);
                async move {
                    pause(&clock, REPORT_DELAY).await?;
                    if fail {
                        return Err("Simulated read failure. Retry to load the report.".into());
                    }
                    describe(&jobs, id)
                }
            },
            services.spawner(),
        );
        Ok(Self {
            id,
            jobs: inputs.jobs,
            history: History::from_owner(&owner).expect("inspector history is installed"),
            boundary: AsyncBoundary::coherent(),
            report,
            revision,
            fail_next,
        })
    }
}
impl JobDetails {
    fn status(&self) -> String {
        match self.boundary.status() {
            BoundaryStatus::Pending => "Loading report… (the previous report stays visible)".into(),
            BoundaryStatus::Error(error) | BoundaryStatus::Faulted(error) => error,
            BoundaryStatus::Ready => "Report ready".into(),
            _ => String::new(),
        }
    }
    fn progress(&self) -> u32 {
        self.jobs
            .with(|jobs| find(jobs, self.id).map_or(0, |job| job.progress))
    }
    fn reload(&self, fail: bool) {
        self.fail_next.set(fail);
        self.revision.update(|revision| *revision += 1);
    }
}

fn find(jobs: &[Job], id: u32) -> Option<&Job> {
    jobs.iter().find(|job| job.id == id)
}

const PROGRESS_TICK: Duration = Duration::from_millis(500);
const PROGRESS_STEP: u32 = 2;
const REPORT_DELAY: Duration = Duration::from_millis(600);

async fn advance_jobs(jobs: Signal<Vec<Job>>, clock: Services) -> Result<(), Error> {
    while jobs.with(|jobs| jobs.iter().any(|job| !job.cancelled && job.progress < 100)) {
        clock.sleep(PROGRESS_TICK)?.await?;
        jobs.update(|jobs| {
            for job in jobs.iter_mut().filter(|job| !job.cancelled) {
                job.progress = (job.progress + PROGRESS_STEP).min(100);
            }
        });
    }
    Ok(())
}

async fn pause(clock: &Services, delay: Duration) -> Result<(), String> {
    let sleep = clock.sleep(delay).map_err(|error| error.to_string())?;
    sleep.await.map_err(|error| error.to_string())
}

fn describe(jobs: &Signal<Vec<Job>>, id: u32) -> Result<String, String> {
    let job = jobs
        .with_untracked(|jobs| find(jobs, id).cloned())
        .ok_or("This job was removed.")?;
    let status = if job.cancelled {
        "cancelled"
    } else {
        "working"
    };
    Ok(format!("{} · {}% · {status}", job.name, job.progress))
}

fusor::template!(backend = "hypercmd", "ui/app.html");
fusor::template!(backend = "hypercmd", "ui/details.html");

fn main() -> Result<(), Error> {
    hypercmd::native::run(hypercmd_app()?)
}
