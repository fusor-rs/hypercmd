# Contributing

Use Rust 1.85 or later, edition 2024, and `just`. Hypercmd-owned Rust forbids
unsafe code. Keep native runtime dependencies independent of browser features.

Run `just check` before considering a change complete. The standalone consumer
executes generated HTML; generated-source snapshots are not sufficient evidence.
Add tests only for a required observable contract or a realistic regression that
existing tests do not establish. Prefer extending the consumer.

On a fresh machine, run `just fetch` before the offline archive gate. Browser
component changes also require `just setup-browser` once and `just browser`.
See `docs/consumption.md` for the archive output.

The runtime and build helper consume fusor's supported public backend contracts.
Do not copy fusor's semantic lowering, reactive machinery or route matching.
Preserve token streams through `quote!`. An upstream gap needs a minimal
consumer and a bounded upstream proposal.

Review each cohesive implementation change independently. Check for correctness,
lifecycle/reentrancy, unbounded work, duplicated paths, speculative abstractions,
misleading comments and tests that mirror implementation. Resolve blocking
findings before advancing.

## Releasing

Every crate shares one version: `version` under `[workspace.package]` in the
root `Cargo.toml`. A release publishes `hypercmd`, `hypercmd-build` and
`hypercmd-cli` at that version; the examples are not published.

1. Bump the version (and the `=x.y.z` pins in the README), run `just check` so
   `Cargo.lock` follows, and merge that to `main` with CI passing.
2. On GitHub, publish a release whose tag is the version with a `v`, such as
   `v0.2.0`, on that commit.

The `Release` workflow then builds the CLI for Linux (x86-64 and ARM, statically
linked) and macOS (Apple Silicon and Intel), installs each archive with
`install.sh` and creates a new app with it. Once every platform passes, it
attaches the archives and their `.sha256` checksums to the release, checks that
the tag matches the version, runs `just check` and a dry run, and publishes to
crates.io. Rerunning a stopped release skips crates already published.

To try the binaries without releasing, run the `Release` workflow by hand from
the Actions tab; it keeps the archives as workflow artifacts and publishes
nothing. The `crates-io` environment holds the `CARGO_REGISTRY_TOKEN` secret,
which needs the `publish-new` and `publish-update` scopes.

## Using AI tools

You're welcome to use AI tools, but you're responsible for everything you
submit. Read and understand every line, make sure the tests you add would fail
without your change, and be ready to explain your reasoning in review. Please
don't open pull requests or issues generated without that review.

hypercmd's own development uses AI tools heavily too, mainly Claude Code, and the
maintainer works under the same rules.
Coding agents follow `AGENTS.md`, which is also the code standard for every
change.
