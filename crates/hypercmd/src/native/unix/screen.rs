use crate::layout::Presentation;
use ratatui::{
    backend::{Backend, CrosstermBackend},
    buffer::Buffer,
    layout::Rect,
};
use std::{
    collections::BTreeMap,
    io::{self, Write},
    rc::Rc,
};

pub(super) struct Screen {
    output: CrosstermBackend<io::Stdout>,
    buffer: Buffer,
    hyperlinks: BTreeMap<(u16, u16), Rc<str>>,
}

impl Screen {
    pub(super) fn new() -> Self {
        Self {
            output: CrosstermBackend::new(io::stdout()),
            buffer: Buffer::empty(Rect::default()),
            hyperlinks: BTreeMap::new(),
        }
    }

    pub(super) fn clear(&mut self) -> io::Result<()> {
        self.output.clear()?;
        self.buffer.reset();
        self.hyperlinks.clear();
        Ok(())
    }

    pub(super) fn draw(&mut self, presentation: &Presentation) -> io::Result<()> {
        if self.buffer.area != presentation.buffer.area {
            self.clear()?;
            self.buffer.resize(presentation.buffer.area);
        }
        // Ratatui cells carry no hyperlink; a changed target must invalidate the glyph.
        for position in self.hyperlinks.keys().chain(presentation.hyperlinks.keys()) {
            if self.hyperlinks.get(position) != presentation.hyperlinks.get(position) {
                self.buffer[*position].set_symbol("");
            }
        }
        let updates = self.buffer.diff(&presentation.buffer);
        for group in updates.chunk_by(|left, right| {
            presentation.hyperlinks.get(&(left.0, left.1))
                == presentation.hyperlinks.get(&(right.0, right.1))
        }) {
            let target = presentation.hyperlinks.get(&(group[0].0, group[0].1));
            if let Some(target) = target {
                write!(self.output, "\x1b]8;;{target}\x1b\\")?;
            }
            self.output.draw(group.iter().copied())?;
            if target.is_some() {
                self.output.write_all(super::ScreenMode::Hyperlink.undo())?;
            }
        }
        Write::flush(&mut self.output)?;
        self.buffer.clone_from(&presentation.buffer);
        self.hyperlinks.clone_from(&presentation.hyperlinks);
        Ok(())
    }
}
