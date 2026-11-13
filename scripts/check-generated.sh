#!/bin/sh
# Generated-file gate (#730): reruns every generator and every vendored copy and fails if any
# committed file differs from what it produces now. A stale file is rewritten in the tree (commit
# it, see `git diff`); a clean tree stays untouched, mtimes included.
#   src/cop_docs.txt               scripts/dump-cop-docs.rb
#   src/autocorrect/registry.tsv   scripts/autocorrect_registry.rb
#   docs/cop-inventory.tsv         scripts/cop_inventory.rb
#   src/helpers/alignment.rs       WIDTHS rows from scripts/display_widths.rb
#   src/encodings.bin              scripts/encodings.rb
#   src/node_kind.rs               scripts/gen_node_kinds.py on the release build's ruby-prism bindings
#   src/help.txt                   `rubocop -h`
#   src/helpers/ruby_exceptions/<linux|darwin>.rs, the one for this host's platform:
#                                  scripts/dump-ruby-exceptions.rb (Errno codes and aliases differ)
#   config/default.yml, config/obsoletion.yml, config/plugins/<gem>.yml, <gem>-obsoletion.yml:
#                                  verbatim copies of the pinned gems' config files
# Not checked: the other platform's ruby_exceptions table; src/helpers/nil_methods.rs is written by hand.
# usage: scripts/check-generated.sh
# env: RUBOCOP as for every RuboCop script, and a `ruby` that loads the same pinned gems (on the CI
#   host GEM_HOME=GEM_PATH=~/rubocop-1.92.2/gems, RBENV_VERSION=4.0.6, as ci-run.sh exports).
set -eu
. "$(dirname "$0")/rubocop.sh"
root=$(cd "$(dirname "$0")/.." && pwd)
. "$root/scripts/rbcop.sh"
cd "$root"
# dump-cop-docs.rb reads cop sources as US-ASCII (and stops at the first non-ASCII byte) under C.
export LC_ALL=C.UTF-8 LANG=C.UTF-8
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# The generators require the gems themselves: `ruby` must see the versions $RUBOCOP runs on.
want=$(printf '%s\n' "rubocop:$rubocop_expected" $rubocop_pins | sort)
# shellcheck disable=SC2016,SC2086
found=$(ruby -e 'ARGV.each { |d| n = d.split(":")[0]; puts "#{n}:#{n == "ruby" ? RUBY_VERSION : Gem::Specification.find_by_name(n).version}" }' $want | sort) || {
  echo "check-generated: ruby cannot load the pinned gems" >&2
  exit 1
}
if [ "$found" != "$want" ]; then
  echo "check-generated: ruby does not load the pinned gems (set GEM_HOME/GEM_PATH to the gem home \$RUBOCOP uses)" >&2
  echo "expected: $(echo $want)" >&2
  echo "found:    $(echo $found)" >&2
  exit 1
fi

changed=
# gen FILE COMMAND...: FILE must be exactly COMMAND's stdout.
gen() {
  gen_f=$1
  shift
  if ! "$@" > "$tmp/new" 2> "$tmp/err"; then
    cat "$tmp/err" >&2
    echo "check-generated: '$*' failed (for $gen_f)" >&2
    exit 1
  fi
  if ! cmp -s "$tmp/new" "$gen_f"; then
    echo "check-generated: $gen_f is stale: $(diff "$gen_f" "$tmp/new" | grep -c '^[<>]' || true) lines differ"
    cp "$tmp/new" "$gen_f"
    changed="$changed $gen_f"
  fi
}
gem_dir() { ruby -e 'print Gem::Specification.find_by_name(ARGV[0]).gem_dir' "$1"; }

gen src/cop_docs.txt ruby scripts/dump-cop-docs.rb
gen src/autocorrect/registry.tsv ruby scripts/autocorrect_registry.rb
gen docs/cop-inventory.tsv ruby scripts/cop_inventory.rb
gen src/encodings.bin ruby scripts/encodings.rb
ruby scripts/display_widths.rb > "$tmp/widths" 2> "$tmp/err" || { cat "$tmp/err" >&2; exit 1; }
gen src/helpers/alignment.rs awk -v t="$tmp/widths" \
  'f && /^\];/ { f = 0 } !f { print } /^static WIDTHS: / { f = 1; while ((getline l < t) > 0) print l }' \
  src/helpers/alignment.rs
# ponytail: newest bindings.rs; a target dir holding several ruby-prism builds picks the last built.
bindings=$(ls -t "${CARGO_TARGET_DIR:-$root/target}"/release/build/ruby-prism-*/out/bindings.rs | head -n 1)
gen src/node_kind.rs python3 -I scripts/gen_node_kinds.py "$bindings"
gen src/help.txt rubocop -h
gen "src/helpers/ruby_exceptions/$(uname -s | tr '[:upper:]' '[:lower:]').rs" ruby scripts/dump-ruby-exceptions.rb -
d=$(gem_dir rubocop)
gen config/default.yml cat "$d/config/default.yml"
gen config/obsoletion.yml cat "$d/config/obsoletion.yml"
for f in config/plugins/*.yml; do
  g=$(basename "$f" .yml)
  case $g in
    *-obsoletion) gen "$f" cat "$(gem_dir "${g%-obsoletion}")/config/obsoletion.yml" ;;
    *) gen "$f" cat "$(gem_dir "$g")/config/default.yml" ;;
  esac
done

if [ -n "$changed" ]; then
  # Outside a checkout (the CI host syncs the tree without .git) `git diff` would compare the files
  # with each other (--no-index), so only inside one.
  # shellcheck disable=SC2086
  if git rev-parse --git-dir > /dev/null 2>&1; then git diff --stat -- $changed; fi
  echo "check-generated: stale generated files rewritten in the tree:$changed"
  exit 1
fi
echo "generated files match"
