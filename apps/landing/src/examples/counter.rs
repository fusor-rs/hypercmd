use fusor::{FromInputs, Signal, signal};

#[derive(FromInputs)]
pub(crate) struct Counter {
    #[local(init = signal(0))]
    count: Signal<u32>,
}

impl Counter {
    fn add(&self) {
        self.count.update(|count| *count += 1);
    }

    fn reset(&self) {
        self.count.set(0);
    }
}

fusor::template!(backend = "hypercmd", "ui/counter.html");
