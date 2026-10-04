use fusor::{FromInputs, Signal, signal};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Command {
    Build,
    Test,
    Deploy,
}

const COMMANDS: &[Command] = &[Command::Build, Command::Test, Command::Deploy];

impl Command {
    fn label(self) -> &'static str {
        match self {
            Self::Build => "Build",
            Self::Test => "Test",
            Self::Deploy => "Deploy",
        }
    }
}

#[derive(FromInputs)]
pub(crate) struct Menu {
    #[local(init = signal(Command::Build))]
    selected: Signal<Command>,
}

fusor::template!(backend = "hypercmd", "ui/menu.html");
