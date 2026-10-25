#!/bin/sh
# Record `rubocop --lsp` transcripts for tests/lsp/<name>/ (replayed by tests/lsp.rs).
# Each scenario has input.jsonl (one JSON-RPC message per line, `$ROOT` standing for the project
# directory) and project/ (the working directory). The server's messages are written to
# expected.jsonl, one body per line with the project directory as `$ROOT` again, and its exit
# status to status.
# usage: scripts/lsp.sh [name...]   (RuboCop from $RUBOCOP, see scripts/rubocop.sh)
set -eu
. "$(dirname "$0")/rubocop.sh"
root=$(cd "$(dirname "$0")/.." && pwd)/tests/lsp
names=${*:-$(ls "$root")}
for name in $names; do
  dir="$root/$name"
  [ -d "$dir" ] || continue
  project=$(cd "$dir/project" && pwd -P)
  in=$(mktemp)
  out=$(mktemp)
  ruby -e 'File.foreach(ARGV[0]) { |l| m = l.chomp.gsub("$ROOT", ARGV[1]); print "Content-Length: #{m.bytesize}\r\n\r\n#{m}" unless m.empty? }' \
    "$dir/input.jsonl" "$project" > "$in"
  status=0
  (cd "$project" && rubocop --lsp < "$in" 2>/dev/null) > "$out" || status=$?
  echo "$status" > "$dir/status"
  ruby -e '
    s = File.binread(ARGV[0])
    until s.empty?
      head, s = s.split("\r\n\r\n", 2)
      n = head[/Content-Length: (\d+)/, 1].to_i
      puts s.byteslice(0, n).force_encoding("UTF-8").gsub(ARGV[1], "$ROOT")
      s = s.byteslice(n..)
    end' "$out" "$project" > "$dir/expected.jsonl"
  rm -f "$in" "$out"
  echo "$name: $(wc -l < "$dir/expected.jsonl" | tr -d ' ') messages recorded, exit $status"
done
