use fusor::{FromInputs, Signal, signal};

#[derive(FromInputs)]
pub(crate) struct Boxes {
    #[local(init = signal(false))]
    stacked: Signal<bool>,
}

fusor::template!(backend = "hypercmd", "ui/boxes.html");
