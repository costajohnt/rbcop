# `-f json` output and exit codes

`rbcop -f json` (or `--format json`, `-f j`) prints RuboCop 1.92.2's JSON formatter output, byte for
byte: one line of JSON, no trailing newline, on stdout (or the `-o` file). This page documents
that output as rbcop emits it (src/formatter/report.rs, `Json`), so a wrapper can parse it without
reading RuboCop's source.

## Shape

```json
{
  "metadata": {
    "rubocop_version": "1.92.2",
    "ruby_engine": "ruby",
    "ruby_version": "4.0.6",
    "ruby_patchlevel": "0",
    "ruby_platform": "x86_64-linux"
  },
  "files": [
    {
      "path": "lib/a.rb",
      "offenses": [
        {
          "severity": "convention",
          "message": "Style/StringLiterals: Prefer single-quoted strings when you don't need string interpolation or special symbols.",
          "cop_name": "Style/StringLiterals",
          "corrected": false,
          "correctable": true,
          "location": {
            "start_line": 3,
            "start_column": 6,
            "last_line": 3,
            "last_column": 10,
            "length": 5,
            "line": 3,
            "column": 6
          }
        }
      ]
    },
    { "path": "lib/b.rb", "offenses": [] }
  ],
  "summary": { "offense_count": 1, "target_file_count": 2, "inspected_file_count": 2 }
}
```

(Pretty-printed here; the real output is a single line, with members in exactly this order.)

### `metadata`

| Field | Value |
|---|---|
| `rubocop_version` | the RuboCop release rbcop matches (`1.92.2`), not rbcop's own version (`rbcop -V` prints both) |
| `ruby_engine` | always `"ruby"` |
| `ruby_version`, `ruby_patchlevel` | the Ruby RuboCop's recordings were made with (the repository's `.ruby-version`), not a Ruby on the machine: rbcop runs none to lint |
| `ruby_platform` | `RUBY_PLATFORM` for the running OS and architecture, e.g. `x86_64-linux`, `arm64-darwin25` |

### `files`

One entry per file inspected, in the order inspected, which is RuboCop's (paths given on the
command line in that order). A file with
no offenses is listed with `"offenses": []`; a file is never left out for being clean. Files a
run never reached (after `--fail-fast` stops, or a config error ends the run) are not listed.

- `path`: the file's path relative to the working directory when it is under it, else as given
  (RuboCop's `smart_path`).
- `offenses`: as below, in source order.
- `errors`, `warnings`: present only when non-empty: messages of cops that raised on this file
  (an error) or warned. Messages differing only in line and column are listed once.

### An offense

| Field | Type | Meaning |
|---|---|---|
| `severity` | string | `info`, `refactor`, `convention`, `warning`, `error` or `fatal` |
| `message` | string | the message as the text formatters print it, with the `Cop/Name: ` prefix when cop names are displayed (the default; `--no-display-cop-names` and `DisplayCopNames: false` drop it) |
| `cop_name` | string | e.g. `Style/StringLiterals`, `Lint/Syntax` |
| `corrected` | bool | this run corrected it (`-a`/`-A`) |
| `correctable` | bool | the cop can correct it (`[Correctable]` in the text formatters); true for a corrected one too. Whether the correction is safe is `correction.safe`, below |
| `location` | object | see below |
| `suppressed`, `justification` | bool, string or null | only under `--display-suppressed`, for an offense a `# rubocop:disable` comment suppresses: `true` and the comment's text after `--` (null without one) |
| `correction` | object | only when the offense has a correction: `safe` (bool: the cop's `Safe` and `SafeAutoCorrect` are not false) and `edits`, each with `start_line`, `start_column`, `last_line`, `last_column` (as in `location`), `begin_pos`, `end_pos` (0-based character offsets into the source, end exclusive) and `replacement` |

### `location`

Lines are 1-based. `start_column` is 1-based (the first character of a line is column 1).
`last_column` is the column of the offense's last character, 1-based (equivalently, the 0-based
column just past it; 1 when the range ends at the start of a line). Columns and `length` count
characters, not bytes. `length` is the range's length in characters, across lines for a multiline
range. `line` and `column` repeat `start_line` and `start_column`; RuboCop keeps them for
compatibility and recommends the `start_` names.

### `summary`

| Field | Meaning |
|---|---|
| `offense_count` | offenses listed in `files` |
| `target_file_count` | files the run found to inspect (the paths given, expanded through the config's `Include`/`Exclude`) |
| `inspected_file_count` | files actually inspected (result cache hits count as inspected). `0` means nothing was linted: no target matched, so a `0` with `offense_count` 0 is not a clean pass. It is below `target_file_count` when the run stopped early (`--fail-fast`, a config error) |
| `error_count`, `warning_count` | present only when non-zero: the sizes of all files' `errors` and `warnings` |

## `--no-ruby` additions

Under `--no-ruby` (or `RBCOP_NO_RUBY=1`; docs/known-gaps.md, "Security: when rbcop runs Ruby"),
and only then, two members follow `summary`:

```json
{"metadata":{...},"files":[...],"summary":{...},"skipped_cops":["Rails/Foo"],"skipped_ruby_code":true}
```

- `skipped_cops`: the cops the configuration enables that rbcop does not implement and so did not
  run, sorted. Always present under `--no-ruby` (`[]` when none). rbcop implements every cop of
  the plugins it runs natively (docs/plugins.md), and the cops of any other plugin are only known
  by asking Ruby, so today this is `[]` in practice; `skipped_ruby_code` covers the rest.
- `skipped_ruby_code`: true when the configuration also loads Ruby code whose cops rbcop cannot
  name, so they did not run either: custom cops from a `require:`d file, a plugin gem rbcop does not
  implement (under `--no-ruby` its gem is not looked up, so its cops are unknown), a plugin that
  works through Ruby code (rubocop-erb).

A run checked every enabled cop only when `skipped_cops` is empty and `skipped_ruby_code` is false.
The same information is on stderr (`rbcop: not running ...; --no-ruby keeps RuboCop from running
them`). The exit code does not change for skipped cops.

## Exit codes

As RuboCop's:

| Code | Meaning |
|---|---|
| 0 | no offense at or above the fail level (with `-a`/`-A`, corrected offenses do not count) |
| 1 | at least one offense at or above the fail level |
| 2 | the run failed: invalid options, an invalid or unloadable configuration, an error while inspecting, or, under `--no-ruby`, something that needs Ruby |

The fail level is `--fail-level SEVERITY` (`info`, `refactor`, `convention`, `warning`, `error`,
`fatal`, or `A`/`autocorrect`: fail only on offenses left uncorrected), default `refactor`, so by
default any offense but an `info` one exits 1. `--display-only-fail-level-offenses` hides the
offenses below it in every formatter, JSON included.

## Compatibility policy

- The default `-f json` output tracks RuboCop's JSON formatter (of the RuboCop release rbcop
  targets) byte for byte: the same members, order, escaping and values. `tests/formatters` replays
  RuboCop's recorded output to keep it so. When rbcop moves to a newer RuboCop
  (docs/upgrading-rubocop.md), changes RuboCop made to its formatter come along, and the release
  notes say so.
- rbcop-only members appear only under an explicit rbcop option (today: `skipped_cops` and
  `skipped_ruby_code` under `--no-ruby`), always after RuboCop's members, and are never removed or
  renamed without a note in the release notes.
- Exit codes 0, 1 and 2 keep the meanings above.
