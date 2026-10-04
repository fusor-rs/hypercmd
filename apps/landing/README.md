# Hypercmd landing page

A Fusor browser app at `/`, in the shared `apps/` Cargo workspace. From the
repository root, run `just site` to build the landing page and docs into
`apps/dist/`, then `just preview` to serve them together on port 4187.
`just landing-dev` serves this app alone; `just landing` builds `apps/landing/dist/`.
`FUSOR_BIN` can select a local Fusor CLI. `just setup-browser` installs the tools.

The five templates in `ui/` are both the editor's displayed source and the input
compiled by Hypercmd. Counter is selected initially; the example buttons switch
between counter, filesystem search, tables, boxes and menu demonstrations.
Switching examples starts a fresh component. The preview paints its terminal cells
and exposes the generated button and text-input handlers as accessible
browser controls, without native terminal dependencies.

The full HTML and interactive terminal appear together. The example card enters
in 240ms; switching examples fades in both panes over 180ms. Reduced motion
disables these animations.

Search filters a build-time snapshot of this app's `src/` and `ui/` directories;
the preview shows the first five matches. The table displays the actual sizes of
the HTML files and sorts them on demand. Neither example accesses a visitor's
filesystem. Edit each example's Rust state in `src/examples/` and its terminal
styles in `ui/terminal.css`. The example catalog and terminal viewport heights
live in `build.rs`.

Brand assets come from `assets/brand/`. The build copies those assets and each
example's downloadable source into `public/`; edit the originals. The page links
to the sibling documentation app at `/docs/` and its CLI reference at `/docs/cli`.
The quick start shows how to create and run an app after installing the CLI.
The combined site mounts both applications; see the root
[deployment guide](../../CONTRIBUTING.md#deploying-the-site) for Vercel hosting.

`just landing-check` checks formatting, Clippy and Rust 1.85. `just landing-browser`
builds the app and exercises entrance and switching animations, all five terminal
interactions, installation clipboard, and responsive layout in Chromium.
