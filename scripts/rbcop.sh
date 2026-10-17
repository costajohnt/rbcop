# shellcheck shell=sh
# Sourced by every script that runs the rbcop binary. Builds it (a stale binary reports phantom
# diffs, e.g. right after a merge), sets $bin to the release binary (under $CARGO_TARGET_DIR when
# set) and exits loudly when it does not run, so a missing binary never turns into an empty cop
# list that filters both sides of a comparison to nothing and a false "all corpora match" (#163).
rbcop_root=$(cd "$(dirname "$0")/.." && pwd)
bin=${CARGO_TARGET_DIR:-$rbcop_root/target}/release/rbcop
(cd "$rbcop_root" && cargo build --release -q) || { echo "$0: cargo build --release failed" >&2; exit 1; }
if ! "$bin" --version > /dev/null 2>&1; then
  echo "$0: rbcop does not run as '$bin'" >&2
  exit 1
fi
# Every cop the binary implements, comma separated: --show-cops prints RuboCop's YAML and the cop
# names are its unindented keys (with $CONFIG, as that config lists them: a plugin config adds the
# plugin's cops). `rbcop_only_cops` leaves out Lint/RedundantCopDisableDirective,
# which RuboCop's --only rejects. Both fail (non-zero, nothing printed) rather than print an empty
# list; call them in a plain assignment (`cops=$(rbcop_cops)`), where set -e sees the failure.
rbcop_cops() {
  rbcop_cops_list=$("$bin" --show-cops ${CONFIG:+--config "$CONFIG"} | sed -n 's/^\([A-Z][A-Za-z]*\/[A-Za-z0-9]*\):$/\1/p' | grep -vx "${1:-}" | paste -sd, -)
  if [ -z "$rbcop_cops_list" ]; then
    echo "$0: no cops from '$bin --show-cops'" >&2
    return 1
  fi
  echo "$rbcop_cops_list"
}
rbcop_only_cops() { rbcop_cops Lint/RedundantCopDisableDirective; }
