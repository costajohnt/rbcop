#!/bin/sh
# Record `rubocop --mcp` transcripts for tests/mcp/<name>/ (replayed by tests/mcp.rs).
# Each scenario has input.jsonl (one JSON-RPC message per line, sent as is, `$ROOT` standing for
# the project directory) and project/, copied to a temp dir that is the working directory (the
# autocorrection tool writes files). The server's lines are written to expected.jsonl with that
# directory as `$ROOT` again, and its exit status to status.
# RuboCop needs the `mcp` gem (~> 0.6; recorded with 0.25.0), e.g. installed into a scratch
# GEM_HOME and passed with GEM_PATH=<that dir>:$(gem env gempath).
# usage: scripts/mcp.sh [name...]   (RuboCop from $RUBOCOP, see scripts/rubocop.sh)
set -eu
. "$(dirname "$0")/rubocop.sh"
root=$(cd "$(dirname "$0")/.." && pwd)/tests/mcp
names=${*:-$(ls "$root")}
for name in $names; do
  dir="$root/$name"
  [ -d "$dir" ] || continue
  tmp=$(mktemp -d)
  cp -R "$dir/project" "$tmp/project"
  project=$(cd "$tmp/project" && pwd -P)
  status=0
  sed "s#\$ROOT#$project#g" "$dir/input.jsonl" | (cd "$project" && rubocop --mcp 2>/dev/null) > "$tmp/out" || status=$?
  echo "$status" > "$dir/status"
  ruby -e 'File.foreach(ARGV[0]) { |l| puts l.chomp.gsub(ARGV[1], "$ROOT") }' "$tmp/out" "$project" > "$dir/expected.jsonl"
  rm -rf "$tmp"
  echo "$name: $(wc -l < "$dir/expected.jsonl" | tr -d ' ') lines recorded, exit $status"
done
