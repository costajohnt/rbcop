#!/bin/sh
# Render the Homebrew formula from packaging/homebrew/rbcop.rb.in and the .sha256 files that
# scripts/dist/build.sh wrote (all four of the darwin and linux-gnu archives must be there).
# usage: scripts/dist/homebrew.sh > rbcop.rb
# RBCOP_DIST: archive directory (default dist). RBCOP_DOWNLOAD_BASE: URL prefix of the archives
# (default: the GitHub release for the Cargo.toml version).
set -eu
cd "$(dirname "$0")/../.."
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
out=${RBCOP_DIST:-dist}
base=${RBCOP_DOWNLOAD_BASE:-https://github.com/costajohnt/rbcop/releases/download/v$version}
script="s|@VERSION@|$version|g; s|@BASE@|$base|g"
for target in aarch64-apple-darwin x86_64-apple-darwin aarch64-unknown-linux-gnu x86_64-unknown-linux-gnu; do
  sum=$(cut -d' ' -f1 "$out/rbcop-$version-$target.tar.gz.sha256")
  script="$script; s|@SHA_$target@|$sum|g"
done
sed "$script" packaging/homebrew/rbcop.rb.in
