mod boxes;
mod counter;
mod files;
mod menu;
mod search;
mod tables;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ExampleKind {
    Counter,
    Search,
    Tables,
    Boxes,
    Menu,
}

impl ExampleKind {
    pub(crate) fn mount(self) -> Result<hypercmd::Scope, hypercmd::Error> {
        match self {
            Self::Counter => hypercmd::mount::<counter::Counter>(counter::CounterInputs),
            Self::Search => hypercmd::mount::<search::Search>(search::SearchInputs),
            Self::Tables => hypercmd::mount::<tables::Tables>(tables::TablesInputs),
            Self::Boxes => hypercmd::mount::<boxes::Boxes>(boxes::BoxesInputs),
            Self::Menu => hypercmd::mount::<menu::Menu>(menu::MenuInputs),
        }
    }
}
