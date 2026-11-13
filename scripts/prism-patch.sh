#!/bin/sh
# Rebuild vendor/ruby-prism from the pristine crates.io crate and rbcop's patches (#727).
# usage: scripts/prism-patch.sh <version> <out-dir>
# Downloads ruby-prism <version> into <out-dir>/ruby-prism-<version> and applies
# vendor/ruby-prism.patches/*.diff in order, without fuzz. Stops at the first patch that does not
# apply, leaving its rejected hunks in *.rej there to port by hand; re-extract that patch from the
# result before moving on (docs/upgrading-rubocop.md, "Prism and rubydex"). At the vendored
# version, `diff -r <out-dir>/ruby-prism-<version> vendor/ruby-prism` prints nothing.
set -eu
[ $# -eq 2 ] || { echo "usage: $0 <version> <out-dir>" >&2; exit 2; }
version=$1 out=$2
patches=$(cd "$(dirname "$0")/.." && pwd)/vendor/ruby-prism.patches
crate=$out/ruby-prism-$version
[ ! -e "$crate" ] || { echo "$0: $crate exists; use an empty out-dir" >&2; exit 2; }
mkdir -p "$out"
curl -sSfL "https://static.crates.io/crates/ruby-prism/ruby-prism-$version.crate" | tar xzf - -C "$out"
# Cargo's packaging stamp; the vendored copy has none.
rm -f "$crate/.cargo_vcs_info.json"
for p in "$patches"/*.diff; do
  if ! patch -s -F 0 -p 1 -d "$crate" < "$p"; then
    echo "$0: $(basename "$p") does not apply to ruby-prism $version; see the .rej files in $crate" >&2
    exit 1
  fi
done
# patch keeps a .orig of each file a hunk applied to at an offset (the crate's own Cargo.toml.orig
# stays).
find "$crate" -name '*.orig' ! -name Cargo.toml.orig -exec rm {} +
echo "ruby-prism $version with $(ls "$patches"/*.diff | wc -l | tr -d ' ') patches: $crate"
