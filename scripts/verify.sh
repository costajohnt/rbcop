#!/bin/sh
# Kept for old instructions: the pre-merge gate is scripts/ci.sh.
exec sh "$(dirname "$0")/ci.sh" "$@"
