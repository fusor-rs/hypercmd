# CLI reference

The `hypercmd` CLI creates, checks, runs and builds terminal applications. It
requires Rust 1.85 or newer; native applications run on macOS and Linux.

## Install and get started

The CLI is a Cargo package. Cargo is Rust's build tool and package manager; it comes
with the Rust toolchain and downloads dependencies, called crates, from crates.io.

```sh
cargo install hypercmd-cli --locked
hypercmd new my-app
cd my-app
hypercmd run
```

`hypercmd new` checks the new package, and that first check downloads and compiles
dependencies. Edit `ui/app.html` for the interface, `ui/terminal.css` for its layout and
`src/main.rs` for state and behavior. Hypercmd doesn't watch files, so stop the app and
run `hypercmd run` again after editing.

## hypercmd new

```sh
hypercmd new <PATH> [--hypercmd-path <CHECKOUT>]
```

Creates a Cargo package (a directory with a `Cargo.toml` manifest) and checks that it
compiles. The destination must not exist. The directory name becomes the package and
executable name: start with a lowercase letter and use lowercase letters, digits,
hyphens or underscores.

The starter includes a counter, its HTML and stylesheet, a `build.rs` (Cargo's build
script, which here compiles the templates) and the required Cargo dependencies. By
default those dependencies come from crates.io. To use an unreleased local checkout:

```sh
hypercmd new my-app --hypercmd-path /path/to/hypercmd
```

`--hypercmd-path` points at the repository root containing `crates/hypercmd` and
`crates/hypercmd-build`.

## hypercmd run

```sh
hypercmd run [ARGS]...
```

Builds the application in the current directory with Cargo's development profile (the
unoptimized build Cargo uses by default), then runs its executable in the terminal.
Arguments go to the application, not to Cargo. Use `--` to separate application
arguments from CLI options.

Run it from a package with one executable. For projects with several binaries, use Cargo
directly to select one. See [native execution](native.md) for terminal sessions and
shutdown behavior.

## hypercmd check

```sh
hypercmd check [CARGO_ARGS]...
```

Runs `cargo check` and reports compiler diagnostics at the authored HTML locations
alongside Rust diagnostics. Extra arguments are forwarded to Cargo:

```sh
hypercmd check --locked --offline
hypercmd check --manifest-path my-app/Cargo.toml
```

Hypercmd controls Cargo's diagnostic format; do not pass `--message-format`.

## hypercmd build

```sh
hypercmd build [--debug]
```

Builds an optimized native executable and copies it into `dist/` beside the
application's `Cargo.toml`. For a package named `my-app`, the output is `dist/my-app`.
By default, the executable targets the current host.

`--debug` uses Cargo's development profile for a faster, unoptimized build. Run this
command from the application directory. Projects with several binaries must use Cargo
directly to choose which executable to build.

## hypercmd profile

```sh
hypercmd profile
```

Prints the supported HTML elements and CSS properties with accepted values. It doesn't
build an application. The [HTML and CSS reference](profile.md) explains layout behavior and
unsupported features.

## hypercmd upgrade

```sh
hypercmd upgrade
```

Upgrades the installed CLI to the latest stable release. An equal or older release
leaves the installation unchanged. Cargo installations upgrade through Cargo in their
existing installation root. Other installations download the matching macOS or Linux
archive, verify its SHA-256 checksum and version, and replace the running executable.
The replacement is staged in the same directory so a failed download or verification
leaves the installed CLI intact. Symlinks continue to point to the upgraded executable.

The command uses the same `curl`, `tar` and checksum tools as `install.sh`. Its
installation directory must be writable. Upgrading the CLI does not change existing
projects' dependency versions.

Interactive commands print a warning when a newer release is available:

```text
⚠️ New version available: 0.1.2 → 0.1.3. Run hypercmd upgrade
```

Release checks run at most once per 24 hours with a two-second network timeout.
Results are cached under `$XDG_CACHE_HOME/hypercmd`, or `~/.cache/hypercmd` when that
variable is unset. Failed checks preserve the last known release and do not fail the
command. Automatic checks are skipped in CI, when stdout or stderr is redirected,
for help and version output, and for `check --offline` or `check --frozen`.
`upgrade` always checks for a release directly. A release is offered after its
platform archive, checksum and final installer asset are present.

## Help and version

```sh
hypercmd --help
hypercmd new --help
hypercmd --version
```

Every command accepts `-h` or `--help`. `-V` or `--version` prints the CLI version.
Invalid commands, build failures and other CLI errors return a nonzero exit status.
