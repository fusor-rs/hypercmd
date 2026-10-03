use fusor::{Signal, signal};
use hypercmd_job_controls::{Job, JobRow};
use std::rc::Rc;
use wasm_bindgen::JsValue;

struct App {
    jobs: Signal<Vec<Job>>,
    notice: Signal<String>,
    inspect: Rc<dyn Fn(u32)>,
}

impl App {
    fn new() -> Result<Self, JsValue> {
        let document = web_sys::window()
            .and_then(|window| window.document())
            .ok_or_else(|| JsValue::from_str("browser document is unavailable"))?;
        let style = document.create_element("style")?;
        style.set_text_content(Some(hypercmd_job_controls::BROWSER_CSS));
        document
            .head()
            .ok_or_else(|| JsValue::from_str("browser document has no head"))?
            .append_child(&style)?;
        let jobs = signal((1..=2).map(|id| Job::new(id, id * 10)).collect());
        let notice = signal(String::new());
        let report = notice.clone();
        Ok(Self {
            jobs,
            notice,
            inspect: Rc::new(move |id| report.set(format!("Inspecting job {id}"))),
        })
    }
}

fusor::template!("web/index.html");
