#!/bin/sh
# Re-record tests/spec_corpus (RuboCop's spec examples for every cop, with RuboCop's offenses and
# -A output) using scripts/record_spec_corpus.rb, whose header describes the format.
# usage: scripts/record-spec-corpus.sh <rubocop-src checkout at v1.92.1>
# env: RUBOCOP (see scripts/rubocop.sh), JOBS (parallel RuboCop runs, default CPU count)
set -eu
. "$(dirname "$0")/rubocop.sh"
src=${1:?usage: $0 <rubocop-src>}
# shellcheck disable=SC2154 # rubocop_cmd and rubocop_root are set by rubocop.sh
RUBOCOP=$rubocop_cmd exec ruby "$rubocop_root/scripts/record_spec_corpus.rb" "$src" "$rubocop_root/tests/spec_corpus"
