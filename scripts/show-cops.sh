#!/bin/sh
# Record RuboCop's `--show-cops` output for tests/show_cops.rs (cases must match CASES there).
# usage: scripts/show-cops.sh   (RuboCop from $RUBOCOP, see scripts/rubocop.sh; the `plugins`
# cases need the plugin gems docs/plugins.md pins)
set -eu
. "$(dirname "$0")/rubocop.sh"
root=$(cd "$(dirname "$0")/.." && pwd -P)
dir=$root/tests/show_cops
mkdir -p "$dir/expected"
record() {
  name=$1 project=$2; shift 2
  (cd "$dir/$project" && rubocop --cache false "$@") | sed "s|$dir/$project|<project>|" > "$dir/expected/$name.txt"
  echo "$name: $(wc -l < "$dir/expected/$name.txt" | tr -d ' ') lines"
}
record all project --show-cops
record patterns project --show-cops 'Lint/*,Style/StringLiterals,Style/InverseMethods,Metrics/MethodLength,Naming/MethodName'
record preview project --preview --show-cops 'Layout/*,Bundler/OrderedGems,Metrics/BlockLength'
record default project --force-default-config --show-cops 'Layout/*,Lint/MissingCopEnableDirective,Lint/UriEscapeUnescape'
record plugins_all plugins --show-cops
record plugins_list_enabled plugins --list-enabled-cops-for spec/a_spec.rb
record plugins_docs_url plugins --show-docs-url 'Capybara/RSpec/HaveContent,Capybara/AssertStyle,Capybara/RSpec,RSpec/Focus,Style/Not'
record junit_order junit_order -f junit a.rb
unset CI
record suggest suggest
record suggest_loaded suggest_loaded
record suggest_installed suggest_installed -f p
record suggest_emacs suggest -f emacs
