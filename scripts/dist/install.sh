#!/bin/sh
# Download the release binary for this machine, check its SHA-256, and put it in DIR/rbcop.
# usage: scripts/dist/install.sh VERSION DIR
# RBCOP_DOWNLOAD_BASE: where the archives are (default: the GitHub release vVERSION); any URL curl
# reads, file:// included. Used by action.yml.
set -eu
version=$1
dir=$2
case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) arch=x86_64 ;;
  Linux-aarch64 | Linux-arm64) arch=aarch64 ;;
  Darwin-arm64) target=aarch64-apple-darwin ;;
  Darwin-x86_64) target=x86_64-apple-darwin ;;
  *) echo "rbcop: no prebuilt binary for $(uname -s) $(uname -m)" >&2; exit 1 ;;
esac
if [ "$(uname -s)" = Linux ]; then
  if ls /lib/ld-musl-* >/dev/null 2>&1; then target=$arch-unknown-linux-musl; else target=$arch-unknown-linux-gnu; fi
fi
base=${RBCOP_DOWNLOAD_BASE:-https://github.com/costajohnt/rbcop/releases/download/v$version}
name=rbcop-$version-$target
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
curl -fsSL --retry 3 "$base/$name.tar.gz" -o "$tmp/$name.tar.gz"
curl -fsSL --retry 3 "$base/$name.tar.gz.sha256" -o "$tmp/sum"
if command -v sha256sum >/dev/null; then sum="sha256sum"; else sum="shasum -a 256"; fi
expected=$(cut -d' ' -f1 "$tmp/sum")
actual=$($sum "$tmp/$name.tar.gz" | cut -d' ' -f1)
if [ "$expected" != "$actual" ]; then
  echo "rbcop: checksum mismatch for $name.tar.gz (expected $expected, got $actual)" >&2
  exit 1
fi
tar -C "$tmp" -xzf "$tmp/$name.tar.gz"
mkdir -p "$dir"
mv "$tmp/$name/rbcop" "$dir/rbcop"
"$dir/rbcop" -v >/dev/null
echo "rbcop $version ($target) installed in $dir"
