#!/bin/sh
# Build the gems from release archives (scripts/dist/build.sh): one platform gem per archive,
# dist/gems/rbcop-<version>-<gem platform>.gem, and dist/gems/rbcop-<version>.gem (platform
# `ruby`) with every archive's binary, for lockfiles that list only the `ruby` platform.
# usage: scripts/dist/gem.sh [archive...]   (default: every archive in $RBCOP_DIST, default dist)
# Needs `gem` (RubyGems 3.3.22 or newer understands the -gnu/-musl platforms).
set -eu
cd "$(dirname "$0")/../.."
root=$(pwd)
out=${RBCOP_DIST:-dist}
mkdir -p "$out/gems"
[ $# -gt 0 ] || set -- "$out"/rbcop-*.tar.gz
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# stage DIR: the gemspec, launcher and license files.
stage() {
  mkdir -p "$1/libexec" "$1/exe"
  cp packaging/gem/rbcop.gemspec "$1/"
  cp packaging/gem/exe/rbcop "$1/exe/"
  cp LICENSE README.md "$1/"
  mkdir -p "$1/licenses"
  cp config/LICENSE "$1/licenses/rubocop-LICENSE"
  cp vendor/ruby-prism/LICENSE.md "$1/licenses/prism-LICENSE.md"
  chmod 755 "$1/exe/rbcop"
}

# build DIR VERSION PLATFORM
build() {
  (cd "$1" && RBCOP_GEM_VERSION=$2 RBCOP_GEM_PLATFORM=$3 gem build --quiet rbcop.gemspec >/dev/null)
  mv "$1"/rbcop-*.gem "$root/$out/gems/"
  ls "$out/gems/rbcop-$2$( [ "$3" = ruby ] || echo "-$3").gem"
}

stage "$work/ruby"
for archive in "$@"; do
  name=$(basename "$archive" .tar.gz)          # rbcop-<version>-<target>
  rest=${name#rbcop-}
  version=${rest%%-*}
  target=${rest#*-}
  case "$target" in
    x86_64-unknown-linux-gnu) platform=x86_64-linux-gnu ;;
    aarch64-unknown-linux-gnu) platform=aarch64-linux-gnu ;;
    x86_64-unknown-linux-musl) platform=x86_64-linux-musl ;;
    aarch64-unknown-linux-musl) platform=aarch64-linux-musl ;;
    x86_64-apple-darwin) platform=x86_64-darwin ;;
    aarch64-apple-darwin) platform=arm64-darwin ;;
    *) echo "gem.sh: no gem platform for $target" >&2; exit 2 ;;
  esac
  tar -C "$work" -xzf "$archive"
  chmod 755 "$work/$name/rbcop"
  stage "$work/$platform"
  cp "$work/$name/rbcop" "$work/$platform/libexec/rbcop"
  build "$work/$platform" "$version" "$platform"
  mkdir -p "$work/ruby/libexec/$target"
  cp "$work/$name/rbcop" "$work/ruby/libexec/$target/rbcop"
done
build "$work/ruby" "$version" ruby
