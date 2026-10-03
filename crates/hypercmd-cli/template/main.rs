use fusor::{Signal, signal};

struct Counter {
    count: Signal<i32>,
}

impl Counter {
    fn new() -> Self {
        Self { count: signal(0) }
    }

    fn increment(&self) {
        self.count.update(|n| *n += 1);
    }
}

fusor::template!(backend = "hypercmd", "ui/app.html");

fn main() -> Result<(), hypercmd::Error> {
    hypercmd::native::run(hypercmd_app()?)
}
