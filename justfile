default:
    @just --list

check: lint consumer cli packages msrv landing-check docs-check

# A new app from the CLI must check cleanly against this checkout.
cli:
    cargo run -p hypercmd-cli --locked -- new "$(mktemp -d)/starter" --hypercmd-path .
    node scripts/upgrade.mjs

lint:
    cargo fmt --all -- --check
    cargo test --workspace --locked
    cargo clippy --workspace --all-targets --locked -- -D warnings
    cargo doc --workspace --no-deps --locked

fetch:
    cargo fetch --locked
    cargo fetch --manifest-path tests/consumer/Cargo.toml --locked
    cargo fetch --manifest-path tests/browser-consumer/Cargo.toml --locked
    cargo fetch --manifest-path apps/Cargo.toml --locked

consumer:
    cargo test --manifest-path tests/consumer/Cargo.toml --locked
    cargo clippy --manifest-path tests/consumer/Cargo.toml --all-targets --locked -- -D warnings

msrv:
    cargo +1.85 test --workspace --locked
    cargo +1.85 test --manifest-path tests/consumer/Cargo.toml --locked

[positional-arguments]
check-app *args:
    cargo run -p hypercmd-cli -- check "$@"

packages:
    node scripts/packages.mjs

browser:
    cargo fmt --manifest-path tests/browser-consumer/Cargo.toml -- --check
    cargo +1.85 check --manifest-path tests/browser-consumer/Cargo.toml --target wasm32-unknown-unknown --locked
    cargo clippy --manifest-path tests/browser-consumer/Cargo.toml --target wasm32-unknown-unknown --locked -- -D warnings
    cargo clippy -p hypercmd-job-controls --no-default-features --features browser --target wasm32-unknown-unknown --locked -- -D warnings
    node scripts/browser.mjs
    just landing-browser
    just docs-browser
    just site-browser

[working-directory: "apps"]
site:
    {{env_var_or_default("FUSOR_BIN", "fusor")}} build --site --locked

preview port="4187":
    {{env_var_or_default("FUSOR_BIN", "fusor")}} preview apps/dist --port {{port}}

[unix]
deploy environment="production":
    @case "{{environment}}" in production|preview) ;; *) echo "deploy: expected production or preview, got {{environment}}" >&2; exit 2 ;; esac
    vercel build {{ if environment == "production" { "--prod" } else { "" } }} --yes
    find .vercel/output/static -name '.fusor-*.json' -delete
    vercel deploy --prebuilt {{ if environment == "production" { "--prod" } else { "" } }} --yes

site-browser:
    node scripts/site.mjs

docs-check:
    cargo fmt --manifest-path apps/docs/Cargo.toml -- --check
    cargo clippy --manifest-path apps/docs/Cargo.toml --all-targets --locked -- -D warnings
    cargo +1.85 check --manifest-path apps/docs/Cargo.toml --locked

docs:
    {{env_var_or_default("FUSOR_BIN", "fusor")}} build --manifest-path apps/docs/Cargo.toml --locked

docs-dev:
    {{env_var_or_default("FUSOR_BIN", "fusor")}} dev --manifest-path apps/docs/Cargo.toml

docs-browser:
    node scripts/docs.mjs

landing-check:
    cargo fmt --manifest-path apps/landing/Cargo.toml -- --check
    cargo clippy --manifest-path apps/landing/Cargo.toml --all-targets --locked -- -D warnings
    cargo +1.85 check --manifest-path apps/landing/Cargo.toml --locked

landing:
    {{env_var_or_default("FUSOR_BIN", "fusor")}} build --manifest-path apps/landing/Cargo.toml --locked

landing-dev:
    {{env_var_or_default("FUSOR_BIN", "fusor")}} dev --manifest-path apps/landing/Cargo.toml

landing-browser:
    node scripts/landing.mjs

setup-browser:
    rustup target add wasm32-unknown-unknown
    rustup target add --toolchain 1.85 wasm32-unknown-unknown
    cargo install wasm-bindgen-cli --version 0.2.117 --locked
    cargo install fusor-cli --version 0.1.5 --locked
    npm ci
    npx playwright install chromium

[positional-arguments]
demo *args:
    cargo run -p file-browser --locked -- "$@"
