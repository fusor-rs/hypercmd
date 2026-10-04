# Hypercmd documentation site

This browser application consumes the shared
[`docs-base`](https://github.com/fusor-rs/docs-base) compiler and Fusor components.
Guides remain in `../../docs/`, readable on GitHub. `navigation.json` sets groups
and reading order; `public/brand.css` supplies Hypercmd colors. The web apps share
the `apps/` Cargo workspace, separate from the native terminal crates.

From the Hypercmd repository root, run `just setup-browser` once, then `just site`
to build the combined site into `apps/dist/`. `just preview` serves the landing
page and docs on port 4187; open `/docs/` for the guides.
`just docs-dev` serves the docs alone, and `just docs` builds
`apps/docs/dist/`, including original Markdown at `/docs/content/`. Configure a static
host to serve `apps/docs/dist/index.html` for `/docs/*` routes that are not files.
The site requires JavaScript/Wasm and does not prerender per-page HTML.
The **Deploy site** workflow hosts the combined site on Vercel; see
[deployment setup](../../CONTRIBUTING.md#deploying-the-site).
Markdown lives outside the app directory watched by Fusor; rebuild with
`just site` or restart `just docs-dev` after editing files in `docs/`.

`just docs-check` checks formatting, Clippy and Rust 1.85. `just docs-browser`
builds and tests every guide in Chromium, including source downloads, links,
history, mobile layout and theme persistence. They are included in the existing
`check` and `browser` gates. `FUSOR_BIN` can select a local Fusor CLI.

To add a guide, write a Markdown file in `docs/` starting with `# Title` and add
its slug to `apps/docs/navigation.json`. Start with one short introduction, then
use `##` sections for the body. Text before the first `##` uses the larger lead
typography. `##` headings also form the table of contents;
explicit IDs (`## Heading {#stable-id}`) preserve published anchors. Relative
links to registered Markdown files become site routes. Other relative file
links point to Hypercmd repository source.

Header links and branding belong here. The article shell, navigation, search
and Markdown renderer belong in `docs-base`; update the pinned revision of both
shared crates together when consuming an upstream change.
