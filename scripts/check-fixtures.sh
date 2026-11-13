#!/bin/sh
# Re-record the RuboCop fixtures under tests/ into the working tree and fail when any of them
# changes, so a hand-edited fixture, or one recorded from rbcop or another RuboCop, cannot pass
# `cargo test` unnoticed (#728). Leaves the tree as it found it when everything matches; on a
# mismatch the re-recorded files stay in place for review.
# Recorders: scripts/scenarios.sh, scripts/formatters.sh, scripts/show-cops.sh, scripts/lsp.sh,
# scripts/mcp.sh, tests/autocorrect/record.sh, tests/auto_gen/record.sh. Not tests/spec_corpus
# (scripts/record-spec-corpus.sh needs the RuboCop and plugin source checkouts; see
# docs/porting-cops.md).
# usage: scripts/check-fixtures.sh   (RuboCop from $RUBOCOP, see scripts/rubocop.sh; also needs
#   rubydex 0.4.1 and mcp 0.25.0 on the gem path, as the recordings had)
# env: JOBS (parallel recorder runs, default CPU count)
set -eu
root=$(cd "$(dirname "$0")/.." && pwd -P)
cd "$root"
if [ -n "$(git status --porcelain -- tests)" ]; then
  echo "$0: tests/ has uncommitted changes; commit or stash them first" >&2
  exit 1
fi
# The fixtures are recorded on the CI host, where CI is unset; with CI set RuboCop drops its
# extension suggestions.
unset CI
jobs=${JOBS:-$(getconf _NPROCESSORS_ONLN)}
# Recorders that take case names run in batches in parallel; each case writes only its own files.
# shellcheck disable=SC2011 # case names are plain words
batch() { d=$1; shift; ls "$d" | xargs -n 10 -P "$jobs" "$@" > /dev/null; }
batch tests/scenarios scripts/scenarios.sh
# A failing `require` prints the host's Ruby backtrace; the committed stderr.txt keep the message
# only (require_unloadable_after_plugin), so drop the frames.
for f in $(grep -lE "^/.*:[0-9]+:in '" tests/scenarios/*/stderr.txt || true); do
  grep -vE "^/.*:[0-9]+:in '" "$f" > "$f.tmp" || true
  mv "$f.tmp" "$f"
done
batch tests/lsp scripts/lsp.sh
batch tests/mcp scripts/mcp.sh
batch tests/auto_gen sh tests/auto_gen/record.sh
batch tests/autocorrect tests/autocorrect/record.sh
# fix-* cases share a scratch directory under TMPDIR: one per batch.
# shellcheck disable=SC2016 # expanded by the inner sh
cut -d'|' -f1 tests/formatters/cases.txt | grep -v '^#' | xargs -n 10 -P "$jobs" \
  sh -c 't=$(mktemp -d); TMPDIR=$t scripts/formatters.sh "$@" > /dev/null; s=$?; rm -rf "$t"; exit $s' sh
scripts/show-cops.sh > /dev/null
changed=$(git status --porcelain -- tests)
[ -z "$changed" ] && echo "fixtures: ok" && exit 0
echo "$changed"
git --no-pager diff --stat -- tests
version=$(sed -n 's/^pub const RUBOCOP_VERSION: &str = "\(.*\)";$/\1/p' src/config/mod.rs)
echo "$0: the files above are not what RuboCop $version records; they are re-recorded in place." >&2
echo "Review them with 'git diff -- tests' and commit them (each recorder's header says how to" >&2
echo "re-record a single case), or drop them with 'git checkout -- tests && git clean -fd tests'." >&2
exit 1
