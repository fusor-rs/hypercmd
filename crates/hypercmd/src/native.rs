//! Native terminal ownership and the bounded, wake-driven input loop.
use crate::{Error, Scope, layout::LayoutOptions};

/// Limits are checked before terminal modes change and before decoded edits grow.
pub struct NativeOptions {
    pub max_paste_bytes: usize,
    pub max_edit_bytes: usize,
    pub layout: LayoutOptions,
}

impl Default for NativeOptions {
    fn default() -> Self {
        Self {
            max_paste_bytes: crate::controls::PASTE_LIMIT,
            max_edit_bytes: crate::controls::EDIT_LIMIT,
            layout: LayoutOptions::default(),
        }
    }
}

/// Run a prepared root for the terminal application lifetime.
/// See [`run_with`] for the process-signal lifetime contract.
pub fn run(scope: Scope) -> Result<(), Error> {
    run_with(scope, NativeOptions::default())
}

/// Run with explicit input and layout limits.
///
/// This has the same process-signal lifetime contract as [`run`]: on Unix the
/// upstream registry retains its OS handlers after callback removal. A caller
/// that continues after return must establish its subsequent signal policy.
/// Initial scene/layout validation precedes signal registration and mode changes.
pub fn run_with(scope: Scope, options: NativeOptions) -> Result<(), Error> {
    #[cfg(unix)]
    {
        unix::run(scope, options)
    }
    #[cfg(not(unix))]
    {
        let _ = (scope, options);
        Err(Error::terminal(
            "native input is currently tested on Unix; Windows support is experimental and unavailable",
        ))
    }
}

#[cfg(unix)]
mod unix {
    use super::*;
    use crate::{
        Controller, Services,
        input::{Decoded, Decoder},
        layout,
    };
    use crossterm::{
        cursor::Hide,
        event::EnableBracketedPaste,
        execute,
        terminal::{self, EnterAlternateScreen},
    };
    use mio::{Events, Interest, Poll, Token, unix::SourceFd};
    use ratatui::{Terminal, backend::CrosstermBackend, style::Color};
    use rustix::fs::{OFlags, fcntl_setfl};
    use signal_hook::{
        SigId,
        consts::{SIGCONT, SIGHUP, SIGINT, SIGSTOP, SIGTERM, SIGTSTP, SIGWINCH},
    };
    use std::{
        io::{self, IsTerminal, Read, Write},
        os::{fd::AsRawFd, unix::net::UnixStream},
        panic::{self, AssertUnwindSafe},
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
        },
        time::{Duration, Instant},
    };

    static RUNNING: AtomicBool = AtomicBool::new(false);
    const INPUT: Token = Token(0);
    const SIGNAL: Token = Token(1);
    const TASK: Token = Token(2);
    const BYTES_PER_TURN: usize = 256;

    pub(super) fn run(scope: Scope, options: NativeOptions) -> Result<(), Error> {
        if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
            return Err(Error::terminal(
                "interactive mode requires terminal stdin and stdout; redirected output is unsupported",
            ));
        }
        if std::env::var("TERM").as_deref() == Ok("dumb") {
            return Err(Error::terminal(
                "TERM=dumb cannot provide an interactive screen",
            ));
        }
        if options.max_paste_bytes == 0 || options.max_edit_bytes == 0 {
            return Err(Error::limit("input limits must be greater than zero"));
        }
        if RUNNING.swap(true, Ordering::AcqRel) {
            return Err(Error::terminal(
                "only one Hypercmd terminal session may run at a time",
            ));
        }
        let _running = Running;
        // The UI thread's panic is reported only after Session has unwound.
        let original: Arc<dyn Fn(&panic::PanicHookInfo<'_>) + Send + Sync> =
            panic::take_hook().into();
        let other_threads = original.clone();
        let ui_thread = std::thread::current().id();
        let message = Arc::new(Mutex::new(String::new()));
        let captured = message.clone();
        panic::set_hook(Box::new(move |info| {
            if crate::services::capture_worker_panic(info) {
                return;
            }
            if std::thread::current().id() == ui_thread {
                if let Ok(mut text) = captured.lock() {
                    *text = info.to_string();
                }
            } else {
                other_threads(info);
            }
        }));
        let result = panic::catch_unwind(AssertUnwindSafe(|| run_loop(&scope, options)));
        panic::set_hook(Box::new(move |info| original(info)));
        match result {
            Ok(result) => result,
            Err(payload) => {
                if let Ok(message) = message.lock() {
                    eprintln!("{}", safe_diagnostic(&message));
                }
                panic::resume_unwind(payload)
            }
        }
    }

    struct NativeWake {
        inner: mio::Waker,
        failed: AtomicBool,
    }
    impl std::task::Wake for NativeWake {
        fn wake(self: Arc<Self>) {
            self.wake_by_ref();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            if self.inner.wake().is_err() {
                self.failed.store(true, Ordering::Release);
            }
        }
    }

    struct Running;
    impl Drop for Running {
        fn drop(&mut self) {
            RUNNING.store(false, Ordering::Release);
        }
    }

    fn run_loop(scope: &Scope, options: NativeOptions) -> Result<(), Error> {
        let terminal_path = rustix::termios::ttyname(io::stdin(), Vec::new()).io()?;
        let input = std::fs::File::open(
            <std::ffi::OsStr as std::os::unix::ffi::OsStrExt>::from_bytes(terminal_path.to_bytes()),
        )
        .io()?;
        // An independent open description avoids setting inherited stdout
        // nonblocking when a launcher has duplicated one terminal descriptor.
        fcntl_setfl(&input, OFlags::NONBLOCK).io()?;
        let mut poll = Poll::new().io()?;
        poll.registry()
            .register(&mut SourceFd(&input.as_raw_fd()), INPUT, Interest::READABLE)
            .io()?;
        let services = Services::from_owner(&scope.owner())?;
        let wake = Arc::new(NativeWake {
            inner: mio::Waker::new(poll.registry(), TASK).io()?,
            failed: AtomicBool::new(false),
        });
        let task_waker = std::task::Waker::from(wake.clone());
        services.register_waker(&task_waker);
        let mut controller = Controller::new(scope.root());
        controller.set_limits(options.max_paste_bytes, options.max_edit_bytes);
        scope.publish();
        services.poll_turn()?;
        check_scene(scope)?;
        let mut size = terminal::size().io()?;
        // Validate the first complete layout before entering alternate screen/raw mode.
        let focus = controller.focus();
        layout::render(
            &scope.root(),
            size,
            focus.as_ref(),
            controller.scrolls_mut(),
            &options.layout,
        )?;
        let mut signals = Signals::new().io()?;
        poll.registry()
            .register(
                &mut SourceFd(&signals.read.as_raw_fd()),
                SIGNAL,
                Interest::READABLE,
            )
            .io()?;
        let mut session = Session::default();
        session.enter(&mut io::stdout()).io()?;
        let mut screen = Terminal::new(CrosstermBackend::new(io::stdout())).io()?;
        let mut decoder = Decoder::new(options.max_paste_bytes);
        let mut events = Events::with_capacity(8);
        let mut ready = false;
        let mut redraw = true;
        let mut bytes = [0; BYTES_PER_TURN];
        'run: loop {
            let signal_pending = signals.drain().io()?;
            if signals.shutdown.swap(false, Ordering::AcqRel) {
                break;
            }
            if signals.suspend.swap(false, Ordering::AcqRel) {
                session.restore().io()?;
                signal_hook::low_level::raise(SIGSTOP).io()?;
                session.enter(&mut io::stdout()).io()?;
                screen.clear().io()?;
                redraw = true;
            }
            if signals.resize.swap(false, Ordering::AcqRel)
                || signals.resume.swap(false, Ordering::AcqRel)
            {
                size = terminal::size().io()?;
                redraw = true;
            }
            if wake.failed.load(Ordering::Acquire) {
                return Err(Error::terminal("task notification failed"));
            }
            services.poll_turn()?;
            check_fault(scope)?;
            if scope.take_dirty() || redraw {
                draw(scope, &mut controller, &mut screen, size, &options.layout)?;
                redraw = false;
                check_fault(scope)?;
            }
            let now = Instant::now();
            decoder.expire(now);
            if ready {
                match rustix::io::read(&input, &mut bytes[..]) {
                    Ok(0) => break,
                    Ok(count) => {
                        for byte in &bytes[..count] {
                            match decoder.feed(*byte, Instant::now())? {
                                Some(Decoded::Input(input)) => {
                                    controller.handle(input)?;
                                    check_fault(scope)?;
                                }
                                Some(Decoded::Shutdown) => {
                                    break 'run;
                                }
                                Some(Decoded::Suspend) => {
                                    signals.suspend.store(true, Ordering::Release);
                                    break;
                                }
                                None => {}
                            }
                        }
                        // Readiness is edge-triggered. Retain it until WouldBlock,
                        // while yielding to shutdown/resize/painting every 256 bytes.
                        continue;
                    }
                    Err(rustix::io::Errno::AGAIN) => ready = false,
                    Err(rustix::io::Errno::INTR) => continue,
                    Err(error) => return Err(io_error(error.into())),
                }
            }
            services.register_waker(&task_waker);
            let timeout = if signal_pending || scope.root.0.scene.dirty.get() || services.is_ready()
            {
                Some(Duration::ZERO)
            } else {
                [decoder.timeout(Instant::now()), services.timeout()]
                    .into_iter()
                    .flatten()
                    .min()
            };
            // Even a self-waking task must give newly ready keyboard/signal events
            // a turn. Polling with zero timeout observes readiness without sleeping.
            match poll.poll(&mut events, timeout) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(io_error(error)),
            }
            for event in &events {
                if event.token() == INPUT {
                    ready = true;
                }
            }
        }
        session.restore().io()?;
        let omitted = scope.root.0.scene.omitted.get();
        if omitted > 0 {
            eprintln!(
                "Limit: {omitted} earlier reactive errors omitted (64-entry diagnostic history)"
            );
        }
        for error in scope.take_errors() {
            eprintln!("{}", describe(&error));
        }
        Ok(())
    }

    fn draw(
        scope: &Scope,
        controller: &mut Controller,
        screen: &mut Terminal<CrosstermBackend<io::Stdout>>,
        size: (u16, u16),
        options: &LayoutOptions,
    ) -> Result<(), Error> {
        let diagnostic = scope.root.0.scene.errors.borrow().last().cloned();
        let omitted = scope.root.0.scene.omitted.get();
        let focus = controller.focus();
        let content_size = (
            size.0,
            size.1.saturating_sub(u16::from(diagnostic.is_some())),
        );
        let mut presentation = layout::render(
            &scope.root(),
            content_size,
            focus.as_ref(),
            controller.scrolls_mut(),
            options,
        )?;
        presentation
            .buffer
            .resize(ratatui::layout::Rect::new(0, 0, size.0, size.1));
        controller.decorate(&mut presentation);
        let (color, rgb) = color_capabilities();
        for cell in &mut presentation.buffer.content {
            for color_value in [&mut cell.fg, &mut cell.bg] {
                if !color || (!rgb && matches!(color_value, Color::Rgb(..))) {
                    *color_value = Color::Reset;
                }
            }
        }
        if let Some(error) = diagnostic {
            if size.1 > 0 {
                let mut message = describe(&error);
                if omitted > 0 {
                    message = format!("Limit: {omitted} earlier errors omitted; {message}");
                }
                presentation.buffer.set_stringn(
                    0,
                    size.1 - 1,
                    message,
                    usize::from(size.0),
                    ratatui::style::Style::default()
                        .add_modifier(ratatui::style::Modifier::REVERSED),
                );
            }
        }
        screen
            .draw(|frame| {
                *frame.buffer_mut() = presentation.buffer.clone();
            })
            .io()?;
        controller.presented(presentation)
    }

    fn check_fault(scope: &Scope) -> Result<(), Error> {
        if scope.is_faulted() {
            Err(scope
                .take_errors()
                .into_iter()
                .last()
                .unwrap_or_else(|| Error::template("coherent publication faulted the scene")))
        } else {
            Ok(())
        }
    }

    fn check_scene(scope: &Scope) -> Result<(), Error> {
        scope.take_errors().into_iter().next().map_or(Ok(()), Err)
    }
    fn describe(error: &Error) -> String {
        format!("{:?}: {}", error.kind, safe_diagnostic(&error.message))
    }

    fn safe_diagnostic(message: &str) -> String {
        let mut safe = String::new();
        for ch in message.chars().take(4096) {
            if ch.is_control() {
                safe.extend(ch.escape_default());
            } else {
                safe.push(ch);
            }
        }
        safe
    }

    fn color_capabilities() -> (bool, bool) {
        let rgb = std::env::var("COLORTERM")
            .is_ok_and(|color| matches!(color.as_str(), "truecolor" | "24bit"));
        let indexed = std::env::var("TERM").is_ok_and(|term| term.contains("256color"));
        let enabled = std::env::var_os("NO_COLOR").is_none();
        (enabled && (rgb || indexed), enabled && rgb)
    }

    #[derive(Default)]
    struct Session {
        raw: bool,
        alternate: bool,
        hidden: bool,
        paste: bool,
    }

    impl Session {
        fn enter(&mut self, output: &mut impl Write) -> io::Result<()> {
            self.raw = true;
            terminal::enable_raw_mode()?;
            self.alternate = true;
            execute!(output, EnterAlternateScreen)?;
            self.hidden = true;
            execute!(output, Hide)?;
            self.paste = true;
            execute!(output, EnableBracketedPaste)
        }

        fn restore(&mut self) -> io::Result<()> {
            let mut first = None;
            let mut output = io::stdout();
            for (enabled, command) in [
                (&mut self.paste, b"\x1b[?2004l".as_slice()),
                (&mut self.hidden, b"\x1b[?25h".as_slice()),
                (&mut self.alternate, b"\x1b[?1049l".as_slice()),
            ] {
                if *enabled {
                    match output.write_all(command).and_then(|()| output.flush()) {
                        Ok(()) => *enabled = false,
                        Err(error) => {
                            first.get_or_insert(error);
                        }
                    }
                }
            }
            if self.raw {
                match terminal::disable_raw_mode() {
                    Ok(()) => self.raw = false,
                    Err(error) => {
                        first.get_or_insert(error);
                    }
                }
            }
            first.map_or(Ok(()), Err)
        }
    }
    impl Drop for Session {
        fn drop(&mut self) {
            let _ = self.restore();
        }
    }

    struct Signals {
        read: UnixStream,
        registrations: Vec<SigId>,
        shutdown: Arc<AtomicBool>,
        suspend: Arc<AtomicBool>,
        resume: Arc<AtomicBool>,
        resize: Arc<AtomicBool>,
    }
    impl Signals {
        fn new() -> io::Result<Self> {
            let (read, write) = UnixStream::pair()?;
            read.set_nonblocking(true)?;
            let mut signals = Self {
                read,
                registrations: Vec::new(),
                shutdown: Arc::new(AtomicBool::new(false)),
                suspend: Arc::new(AtomicBool::new(false)),
                resume: Arc::new(AtomicBool::new(false)),
                resize: Arc::new(AtomicBool::new(false)),
            };
            for (signal, flag) in [
                (SIGINT, &signals.shutdown),
                (SIGTERM, &signals.shutdown),
                (SIGHUP, &signals.shutdown),
                (SIGTSTP, &signals.suspend),
                (SIGCONT, &signals.resume),
                (SIGWINCH, &signals.resize),
            ] {
                signals
                    .registrations
                    .push(signal_hook::flag::register(signal, flag.clone())?);
                signals
                    .registrations
                    .push(signal_hook::low_level::pipe::register(
                        signal,
                        write.try_clone()?,
                    )?);
            }
            Ok(signals)
        }
        fn drain(&mut self) -> io::Result<bool> {
            let mut bytes = [0; 4096];
            match self.read.read(&mut bytes) {
                Ok(count) => Ok(count == bytes.len()),
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) =>
                {
                    Ok(false)
                }
                Err(error) => Err(error),
            }
        }
    }
    impl Drop for Signals {
        fn drop(&mut self) {
            for registration in self.registrations.drain(..) {
                signal_hook::low_level::unregister(registration);
            }
        }
    }
    #[cfg(test)]
    mod tests;

    trait Io<T> {
        fn io(self) -> Result<T, Error>;
    }
    impl<T, E: Into<io::Error>> Io<T> for Result<T, E> {
        fn io(self) -> Result<T, Error> {
            self.map_err(|error| io_error(error.into()))
        }
    }
    fn io_error(error: io::Error) -> Error {
        Error::terminal(format!("terminal I/O: {error}"))
    }
}
