#!/bin/sh
# Installs the hypercmd CLI on macOS and Linux:
#
#   curl -fsSL https://raw.githubusercontent.com/fusor-rs/hypercmd/main/install.sh | sh
#
# Install a specific release with `sh -s v0.1.0`, or HYPERCMD_VERSION=v0.1.0.
# HYPERCMD_INSTALL chooses the directory (default ~/.hypercmd); the binary goes in its bin/.
set -eu

repository="fusor-rs/hypercmd"
# Overridable so the release workflow can test this script against the archives
# it has just built, before they are published.
download_base="${HYPERCMD_DOWNLOAD_BASE:-https://github.com/$repository/releases/download}"
version="${1:-${HYPERCMD_VERSION:-}}"
if [ "$version" = "--archive" ]; then
  version="$2"
fi

fail() {
  echo "error: $*" >&2
  exit 1
}

for tool in curl tar uname; do
  command -v "$tool" >/dev/null 2>&1 || fail "$tool is required to install hypercmd"
done

case "$(uname -s)" in
  Darwin) os="apple-darwin" ;;
  Linux) os="unknown-linux-musl" ;;
  *) fail "unsupported operating system $(uname -s); hypercmd apps run on macOS and Linux" ;;
esac
case "$(uname -m)" in
  x86_64 | amd64) arch="x86_64" ;;
  arm64 | aarch64) arch="aarch64" ;;
  *) fail "unsupported processor $(uname -m)" ;;
esac
target="$arch-$os"

if [ -z "$version" ]; then
  version=$(curl -fsSL "https://api.github.com/repos/$repository/releases/latest" |
    sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -n 1)
  [ -n "$version" ] || fail "could not find the latest hypercmd release"
fi
case "$version" in
  v*) ;;
  *) version="v$version" ;;
esac

archive="hypercmd-${version#v}-$target.tar.gz"
if [ "${1:-}" = "--archive" ]; then
  echo "$archive"
  exit 0
fi

url="$download_base/$version/$archive"
executable="${HYPERCMD_BIN:-${HYPERCMD_INSTALL:-$HOME/.hypercmd}/bin/hypercmd}"
bin_dir=$(dirname "$executable")
mkdir -p "$bin_dir"
temporary=$(mktemp -d "$bin_dir/.hypercmd-XXXXXX")
trap 'rm -rf "$temporary"' EXIT

echo "Downloading hypercmd $version for $target"
curl -fsSL "$url" -o "$temporary/$archive" || fail "could not download $url"
curl -fsSL "$url.sha256" -o "$temporary/$archive.sha256" || fail "could not download $url.sha256"

expected=$(cut -d ' ' -f 1 "$temporary/$archive.sha256")
if command -v sha256sum >/dev/null 2>&1; then
  actual=$(sha256sum "$temporary/$archive" | cut -d ' ' -f 1)
elif command -v shasum >/dev/null 2>&1; then
  actual=$(shasum -a 256 "$temporary/$archive" | cut -d ' ' -f 1)
else
  fail "sha256sum or shasum is required to verify the download"
fi
[ "$expected" = "$actual" ] || fail "checksum mismatch for $archive; the download is corrupt or was tampered with"

member="${archive%.tar.gz}/hypercmd"
tar -xzf "$temporary/$archive" -C "$temporary" "$member"
replacement="$temporary/$member"
[ -f "$replacement" ] && [ ! -L "$replacement" ] ||
  fail "archive has no regular hypercmd executable"
chmod +x "$replacement"
installed_version=$("$replacement" --version) || fail "downloaded hypercmd could not run"
[ "$installed_version" = "hypercmd ${version#v}" ] ||
  fail "downloaded hypercmd has the wrong version"
mv -f "$replacement" "$executable"

echo "Installed $installed_version to $bin_dir"

if [ -n "${HYPERCMD_BIN:-}" ]; then
  exit 0
fi

case ":$PATH:" in
  *":$bin_dir:"*) ;;
  *)
    echo
    echo "Add hypercmd to your PATH, for example in ~/.zshrc or ~/.bashrc:"
    echo "  export PATH=\"$bin_dir:\$PATH\""
    ;;
esac

if ! command -v cargo >/dev/null 2>&1; then
  echo
  echo "hypercmd builds your app with Rust, which is not installed."
  echo "Install it from https://rustup.rs, then run: hypercmd new my-app"
fi
