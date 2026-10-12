#!/bin/sh
# Install the release binary for this machine as DIR/rbcop, after checking the archive's SHA-256.
# usage: scripts/dist/install.sh VERSION DIR
#
# The expected checksum comes from packaging/checksums/VERSION.sha256 next to this script (pinned in
# the repository, so a tampered download cannot bring its own checksum). RBCOP_CHECKSUMS names
# another file of `sha256sum` lines. No entry for this archive, a mismatch, or a failed download
# exits non-zero before the binary is extracted or run; there is no way to skip the check.
#
# RBCOP_ARCHIVE_CACHE: keep the downloaded archive in this directory and reuse it next time (it is
#   checked again on every run). RBCOP_DOWNLOAD_BASE: where the archives are (default: the GitHub
#   release vVERSION); any URL curl reads, file:// included. Used by action.yml.
set -eu
version=$1
dir=$2
here=$(cd "$(dirname "$0")" && pwd)
checksums=${RBCOP_CHECKSUMS:-$here/../../packaging/checksums/$version.sha256}
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
name=rbcop-$version-$target
if [ ! -f "$checksums" ]; then
  echo "rbcop: no pinned checksums for $version ($checksums)" >&2
  exit 1
fi
expected=$(awk -v f="$name.tar.gz" '$2 == f || $2 == "*" f { print $1 }' "$checksums")
# Exactly one 64-hex-digit entry, or fail.
if [ "$(printf '%s\n' "$expected" | wc -l)" -ne 1 ] || ! printf '%s' "$expected" | grep -Eqx '[0-9a-f]{64}'; then
  echo "rbcop: no single pinned checksum for $name.tar.gz in $checksums" >&2
  exit 1
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
cache=${RBCOP_ARCHIVE_CACHE:-$tmp}
mkdir -p "$cache"
archive=$cache/$name.tar.gz
if [ ! -f "$archive" ]; then
  base=${RBCOP_DOWNLOAD_BASE:-https://github.com/costajohnt/rbcop/releases/download/v$version}
  curl -fsSL --retry 3 "$base/$name.tar.gz" -o "$tmp/download"
  mv "$tmp/download" "$archive"
fi
if command -v sha256sum >/dev/null; then sum="sha256sum"; else sum="shasum -a 256"; fi
actual=$($sum "$archive" | cut -d' ' -f1)
if [ "$expected" != "$actual" ]; then
  rm -f "$archive"
  echo "rbcop: checksum mismatch for $name.tar.gz (pinned $expected, got $actual)" >&2
  exit 1
fi
tar -C "$tmp" -xzf "$archive"
mkdir -p "$dir"
mv "$tmp/$name/rbcop" "$dir/rbcop"
echo "rbcop $version ($target) installed in $dir"
