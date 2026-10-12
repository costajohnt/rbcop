# Benchmarks

How fast rbcop is against RuboCop 1.91 in the setting most users care about: a CI step on a
GitHub-hosted runner, which starts with no RuboCop result cache unless the workflow restores one.
Tracking issue: #388.

## CI-shaped runs (CT 440)

PENDING_CT

## Where rbcop's time went (#388)

Profiles of rbcop at 64da3c3 (macOS `sample`, release build with line tables), linting discourse
(12,609 files) and rails (3,467 files), with the default config and with every cop (`--only` all).
Shares are of the samples inside the per-file `runner::analyze`.

| hot spot | where | fixed in |
|---|---|---|
| `--only`/`--except` cop selection scanned the 612-name list for each cop of each file | `main.rs` `match_name` | #394 |
| regexps compiled per file or node from config (AllowedPatterns, CommentAnnotation keywords) | `ruby_regex`, `annotation_comment::regex` | #395 |
| `Config#for_cop` scanned ~700 keys twice and cloned two maps per call; SipHash on cop names; glob matching allocated per pattern and path; getcwd per file | `config/mod.rs`, `config/glob.rs` | #398 |
| Lint/DuplicateMethods#humanize_scope: O(n^2) `format!` per `def` (18% of rails samples) | `lint/duplicate_methods.rs` | #400 |
| fixed file chunks per worker: the busiest of 10 workers ran 3.4x longer than the idlest | `main.rs` | #399 |
| variable_force's tree built by 9 cops, token translation by 12, every file parsed 3 times | `source.rs`, `helpers/` | #401 |
| per-node allocation in Lint/Debugger, Naming identifier lists, DuplicateMethods ancestors | cops | #403 |
| no cross-crate inlining | `Cargo.toml` | #404 |

Top cops by samples at 64da3c3, discourse (share of `analyze`):

| default config | | all cops | |
|---|---:|---|---:|
| Lint/DuplicateMethods | 6.9% | Layout/RedundantLineBreak | 15.8% |
| Lint/Debugger | 3.9% | Lint/DuplicateMethods | 3.5% |
| Layout/ExtraSpacing | 3.4% | Lint/Debugger | 2.0% |
| Style/NonNilCheck | 3.0% | Layout/ExtraSpacing | 1.8% |
| Style/CommentAnnotation | 2.9% | Style/NonNilCheck | 1.6% |
| Style/WordArray | 2.7% | Style/CommentAnnotation | 1.5% |
| Naming/HeredocDelimiterNaming | 2.3% | Style/WordArray | 1.4% |
| Layout/IndentationWidth | 2.2% | Layout/IndentationWidth | 1.2% |
| Lint/UnreachableLoop | 1.8% | Naming/HeredocDelimiterNaming | 1.2% |
| Lint/ShadowedArgument | 1.8% | Style/Copyright | 0.9% |

(Cops that share per-file work, such as the variable_force cops, are charged for it when they
run first.)

### Instructions per run, before and after

Load on a shared machine skews wall time but not the instructions a run retires, so each PR was
measured by instructions (macOS `/usr/bin/time -l`) on an M4 MacBook Air, alternating builds, median
of 3. Each PR alone against master (64da3c3):

| PR | rails, default | rails, all cops | discourse, default |
|---|---:|---:|---:|
| master | 234.6G | 440.6G | 600.0G |
| #394 `--only` sets | -0.1% | -47.3% | +0.1% |
| #395 regexp cache | -13.4% | -8.2% | -9.6% |
| #398 config lookups | -30.7% | -5.4% | -40.6% |
| #400 DuplicateMethods | -20.2% | -10.3% | -6.5% |
| #399 scheduling | 0% (wall -17%) | 0% (wall -13%) | 0% (wall -25%) |
| #401 shared analyses | -5.7% | -3.3% | -6.5% |
| #403 cop allocations | -2.8% | -0.6% | -3.9% |
| #394-#403 merged | -73.6% | -78.9% | -69.2% |
| ... plus #404 thin LTO | **-75.5%** | **-80.4%** | **-71.5%** |

Wall time on that (10-core, shared) machine for the last row: rails default 4.74 s to 1.25 s,
rails all cops 7.47 s to 1.89 s, discourse default 11.53 s to 3.55 s.

The standalone savings roughly add up to the merged build's; none of the PRs overlap.

What is left after them (discourse, default config): 70% of the time is the cops themselves,
spread thin (the top cop is 9%); parsing is 7%, Prism's separate lex pass for tokens another 4%.
Further large gains need method-name dispatch (calling a cop only for the call names it handles,
like RuboCop's `RESTRICT_ON_SEND`), which changes the cop trait. Layout/RedundantLineBreak (off
by default) reparses and re-serializes the enclosing scope per offense and dominates all-cops runs.

## Reproducing

CI-shaped runs (Linux, needs `taskset` and RuboCop 1.91 on `PATH`):

    scripts/bench-ci.sh results.tsv 3 rubocop-single,rubocop-par,rbcop <corpus dir>...

It times each tool pinned to an emulated runner (`CPUS_2`, `CPUS_4`, `CPUS_1` override the CPU
lists), with a fresh RuboCop cache root per run, and redoes a run when a CI gate ran meanwhile.
Rows are rep, corpus, vCPUs, tool, wall, user, sys, exit status, load average, contaminated.

Per-PR comparisons used `/usr/bin/time -l` on macOS (instructions retired, cycles elapsed) with
the builds run round-robin, default config (`--force-default-config`) and all cops
(`--only` with every cop `--show-cops` lists except Lint/RedundantCopDisableDirective).
