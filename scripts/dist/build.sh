#!/bin/sh
# Build release archives dist/rbcop-<version>-<target>.tar.gz, each with a .sha256 file.
# usage: scripts/dist/build.sh [target...]
#   Default targets: on macOS aarch64-apple-darwin and x86_64-apple-darwin; on Linux the four
#   x86_64/aarch64 gnu/musl targets.
# Linux targets build with cargo-zigbuild (needs `zig` and `cargo-zigbuild` on PATH); gnu targets
# link against glibc 2.17, so the binary runs on any glibc distribution from 2014 on.
# RBCOP_DIST: output directory (default dist).
set -eu
cd "$(dirname "$0")/../.."
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
out=${RBCOP_DIST:-dist}
mkdir -p "$out"
if [ $# -eq 0 ]; then
  case "$(uname -s)" in
    Darwin) set -- aarch64-apple-darwin x86_64-apple-darwin ;;
    Linux) set -- x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu x86_64-unknown-linux-musl aarch64-unknown-linux-musl ;;
    *) echo "build.sh: name the targets on $(uname -s)" >&2; exit 2 ;;
  esac
fi
if command -v sha256sum >/dev/null; then sum="sha256sum"; else sum="shasum -a 256"; fi
export CARGO_PROFILE_RELEASE_STRIP=symbols
for target in "$@"; do
  case "$target" in
    *-linux-gnu) cargo zigbuild --release --locked --target "$target.2.17" ;;
    *-linux-musl) cargo zigbuild --release --locked --target "$target" ;;
    *) cargo build --release --locked --target "$target" ;;
  esac
  name=rbcop-$version-$target
  stage=$(mktemp -d)
  mkdir -p "$stage/$name/licenses"
  cp "target/$target/release/rbcop" LICENSE README.md "$stage/$name/"
  cp config/LICENSE "$stage/$name/licenses/rubocop-LICENSE"
  cp vendor/ruby-prism/LICENSE.md "$stage/$name/licenses/prism-LICENSE.md"
  # No macOS extended attributes (com.apple.provenance) or ._ files in the archive.
  COPYFILE_DISABLE=1 tar --no-xattrs -C "$stage" -czf "$out/$name.tar.gz" "$name"
  rm -rf "$stage"
  (cd "$out" && $sum "$name.tar.gz" > "$name.tar.gz.sha256")
  echo "$out/$name.tar.gz"
done
