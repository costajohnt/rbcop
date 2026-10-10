# Known gaps and deliberate differences

Tracked so the review phase can close or document each one. "Deliberate" items are intended.

## Deliberate differences from RuboCop

- `-o/--out` pointing at a file being inspected is refused (RuboCop truncates the file).
- `-o/--out` (or anything that could become it) in a `.rubocop` arguments file is refused.
- HTML formatter escapes `& < >` in messages outside code spans and fully escapes paths; GitHub
  Actions formatter escapes CR/LF in the `file=` property. Output differs only when those appear.
- JUnit escapes cop names in attributes (no effect for real cop names).

## Open gaps

- Target Ruby below 3.3: RuboCop parses with the parser gem for that version and reports syntax newer
  than the target (e.g. `def x(&)` under 2.7) as Lint/Syntax; rbcop parses with Prism's 3.3 grammar and
  does not. Messages for genuine syntax errors also come from Prism, not the parser gem.
- `--stdin` together with `-a/-A` (RuboCop prints the corrected source) is refused.
- `--disable-uncorrectable`, `--auto-gen-config`, `--server`, `--lsp`, `--cache` behaviour (accepted
  and ignored where harmless), custom formatter classes, `require:` of local Ruby files (custom cops).
- ERB in `.rubocop.yml` is not evaluated; remote `inherit_from` URLs are skipped with a warning.
- LineLength `SplitStrings: true` break points (string splitting) are not modelled.
- Duplicate helper ports to consolidate: `helpers/alignment.rs`, `alignment_mixin.rs`,
  `alignment_check.rs`; `helpers/macro_scope.rs` and `macro_scope_nodes.rs`;
  `range_help::with_surrounding_space` and `range_help::range_with_surrounding_space`.
- One spec snippet (`naming/method_name/050.rb`, a `%{}` template) gets a duplicated
  "unexpected write target" syntax error location.
- Cops reported `[Correctable]` without a corrector are not fixed by `-a/-A` until retrofitted.
