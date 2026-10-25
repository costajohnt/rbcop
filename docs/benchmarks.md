# Benchmarks

How fast rbcop is against RuboCop 1.91 in the setting most users care about: a CI step on a
GitHub-hosted runner, which starts with no RuboCop result cache unless the workflow restores one.
Tracking issue: #388.

## CI-shaped runs (Linux container)

rbcop at 4b08150 against RuboCop 1.91.0, each corpus linted with its own `.rubocop.yml` (brew: default config), pinned to
2 and 4 vCPUs, RuboCop with an empty result cache. Median of 3 runs, wall seconds. Raw rows:
[benchmarks/ci.tsv](benchmarks/ci.tsv), [benchmarks/boot.tsv](benchmarks/boot.tsv),
[benchmarks/file.tsv](benchmarks/file.tsv).

| corpus | files | RuboCop, 1 process, 2 vCPU | RuboCop `--parallel`, 2 vCPU | rbcop, 2 vCPU | speedup, 2 vCPU | RuboCop `--parallel`, 4 vCPU | rbcop, 4 vCPU | speedup, 4 vCPU |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| rails | 3,476 | 109.3 | 98.7 | 4.89 | 20.2x | 50.6 | 2.64 | 19.2x |
| discourse | 12,441 | 303.9 | 273.7 | 19.31 | 14.2x | 140.6 | 10.16 | 13.8x |
| mastodon | 3,297 | 137.2 | 116.0 | 12.11 | 9.6x | 61.2 | 6.10 | 10.0x |
| brew (default config) | 1,977 | 153.5 | 134.0 | 8.26 | 16.2x | 69.0 | 4.39 | 15.7x |
| solidus | 2,171 | 59.9 | 53.4 | 4.05 | 13.2x | 28.3 | 2.12 | 13.3x |
| fastlane | 1,355 | 66.0 | 58.6 | 3.91 | 15.0x | 30.3 | 2.02 | 15.0x |

Speedup is RuboCop `--parallel` over rbcop at the same vCPU count. Against single-process RuboCop
at 2 vCPUs it is 11.3x (mastodon) to 22.3x (rails). Files are those `rubocop -L` lists.

CPU time (user) per run, which does not depend on the pinning: rails 193 s RuboCop `--parallel` vs
9.3 s rbcop, discourse 536 s vs 36 s, mastodon 228 s vs 23 s, brew 264 s vs 16 s, solidus 103 s
vs 7.8 s, fastlane 114 s vs 7.6 s (2 vCPU rows). RuboCop's `--parallel` user time is about 1.8x its
single-process time because its two workers share one physical core; wall time improves 10 to 15%
over a single process at 2 vCPUs.

Start-up and one file (2 vCPUs, median of 3):

| run | wall s |
|---|---:|
| `rubocop --version` | 0.72 |
| `bundle exec rubocop --version` (Gemfile with rubocop, -performance, -rails, -rspec) | 0.85 |
| `rbcop --version` | 0.02 |
| RuboCop on mastodon's `app/models/account.rb` (607 lines, mastodon's config, 6 plugins), empty cache | 2.39 |
| rbcop on the same file | 0.23 |

### Method

- Machine: a Linux container on an Intel Core i9-8950HK host (6 cores / 12 threads), Debian 13.6,
  kernel 7.0.14-22, x86_64. Ruby 4.0.6 with
  RuboCop 1.91.0, rubocop-performance 1.27.0, rubocop-rails 2.38.0, rubocop-rspec 3.10.2,
  rubocop-rspec_rails 2.33.0, rubocop-capybara 3.0.0, rubocop-factory_bot 2.28.0,
  rubocop-minitest 0.41.0, rubocop-i18n 3.3.0, rubocop-packaging 0.6.0, rubocop-discourse 3.20.0.
  rbcop release build of 4b08150.
- `scripts/bench-ci.sh` with `PROJECT_CONFIG=1` (brew without it), 3 reps. Each rep runs every
  corpus and tool in turn, so slow drift spreads over all rows. 2 vCPUs are CPUs 1,7 (one
  physical core, both hyperthreads), 4 vCPUs are 1,7,2,8 (two cores), on the assumption that a
  hosted runner's vCPUs are hyperthreads. Single-process RuboCop ran at 2 vCPUs only; it uses one
  thread, so 4 vCPUs would not change it.
- RuboCop `--parallel` got a fresh `--cache-root` per run (cold cache, as on a runner without a
  restored cache); single-process RuboCop ran with `--cache false`. rbcop at 4b08150 had no cache (`scripts/bench-ci.sh` now runs it with `--cache false`). The
  corpus files were in the OS page cache for every run.
- Plugins each config loads: rails: minitest, packaging, performance, rails (`DisabledByDefault`,
  so only the cops it lists run). discourse: rubocop-discourse's `stree-compat.yml`, which loads
  capybara, factory_bot, rspec, rails and rubocop-discourse's own cops (`DisabledByDefault`).
  mastodon: capybara, i18n, performance, rails, rspec, rspec_rails, plus a local cop
  (Style/MiddleDot). solidus: performance, rails, plus a local cop. fastlane: performance, plus
  three local cops. brew ran with the default config (see below).
- The corpora were copied, and four configs edited so that RuboCop loads them on this box:
  rails without rubocop-md (not installed; rbcop does not lint Markdown) and its `Markdown:`
  section; fastlane without rubocop-require_tools (not installed) and its
  `Require/MissingRequireStatement` section; solidus without `storefront/.rubocop.yml`, which
  RuboCop 1.91 rejects (`Metrics/LineLength` moved to `Layout/LineLength`); brew is
  `Library/Homebrew` only, whose config inherits `../.rubocop.yml`, which the corpus does not have,
  so it ran with `--force-default-config`.
- RuboCop ran cops rbcop does not implement, and rbcop skipped them (no `RBCOP_FALLBACK`): 21
  rubocop-discourse cops on discourse, I18n/RailsI18n and Style/MiddleDot on mastodon, the local
  cops on solidus and fastlane. On those four corpora RuboCop does that much more work; the cost
  of `RBCOP_FALLBACK=1` was not measured.
- Output was compared before timing: each corpus linted once by both tools with the same config
  (`--format emacs`). Offenses were identical (rails 0, discourse 57, mastodon 5, brew 113,541,
  solidus 4,172, fastlane 0) and so were the exit statuses; every timed run's exit status also
  matches between the tools (see the TSVs).
- Spread: the 3 runs of each corpus row were within 4% of each other, except fastlane rbcop at 4
  vCPUs (2.02 to 2.18 s, 8%); `bundle exec rubocop --version` ranged 0.85 to 1.00 s.

### Load and contamination

The box was reserved for this benchmark. No gate (`ci-run.sh`) ran during any run: the
contaminated column is 0 in all 105 rows. The load average recorded before each run (0.41 to 5.61)
is mostly the previous run decaying, since runs follow each other; the container sees the host's
load average. To check the rest of the host, CPU counters of the four other containers on the host
were sampled every 5 s through the runs: together they used at most 0.14
CPU-seconds per second during any run, and a `top` on the host during a run showed only the benchmark busy.

### Caveats

- This is not a GitHub runner. It is a container on a 2018 laptop CPU; hosted runners use other
  (server) CPUs, so absolute times there will differ. The ratios are more likely to carry over,
  but that is not verified.
- The 2- and 4-vCPU shapes assume a vCPU is one hyperthread and that a runner's vCPUs pair up on
  physical cores. If a runner's vCPUs are on separate cores, RuboCop `--parallel` gains more from
  its second worker than it does here (10 to 15%), and the 2-vCPU speedups shrink.
- Only lint time is measured: not checkout, Ruby setup or `bundle install`, which a CI job pays
  for RuboCop and its plugins and which the rbcop binary (action or gem) does not need.
- RuboCop's cache is cold, as on a runner without a restored cache. A workflow that restores
  RuboCop's cache from an earlier run on the same files is not what this measures.

## Result cache (#281)

rbcop at 88c23816, each corpus with its own `.rubocop.yml`, pinned to 4 vCPUs (CPUs 1,7,2,8),
median of 3: `--cache false`, then a fresh `--cache-root` (cold), then the same root again on the
unchanged tree (warm). Raw rows: [benchmarks/cache.tsv](benchmarks/cache.tsv).

| corpus | `--cache false` wall s | cold wall s | warm wall s | `--cache false` user s | cold user s | warm user s |
|---|---:|---:|---:|---:|---:|---:|
| rails | 8.94 | 9.57 | 1.09 | 27.1 | 27.7 | 0.86 |
| discourse | 27.66 | 26.65 | 3.72 | 81.8 | 78.0 | 3.03 |

The machine was shared with other jobs (load average about 20 on 10 cores for every run), so the
wall times are about 3x the quiet-machine numbers in the table above; user time does not depend
on that. A cold run costs what an uncached one does (within the noise); a warm run reads each
file, hashes it and loads its entry, about 3% of the CPU time of inspecting it.

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

### Second round: each corpus with its own config

Profiled on the Linux box of the CI-shaped runs (no `perf` in the container, so an in-process
sampler, pprof-rs, in a scratch build), rbcop at 64b4a66f linting rails, discourse and mastodon
with their own `.rubocop.yml`. With a project config, choosing each file's cops cost more than
any cop: every one of the ~800 cops matched its Include/Exclude globs against the file, though
most share their department's lists. Shares of the profile samples, before and after:

| hot spot | rails | discourse | mastodon |
|---|---:|---:|---:|
| glob miss falling through to the hidden-file test (`hidden_file_in_not_hidden_dir`) | 5.4% to 0.4% | 9.9% to 1.7% | 6.9% to 0.7% |
| `Resolved::relevant` per cop (now a per-file memo over shared lists) | 8.9% to 1.0% | 12.0% to 5.5% | 16.1% to 2.4% |
| per-file scan of every cop's config and plugin, `disabled_names` per file | 12.3% to 0.7% | 7.8% to 0.6% | 4.1% to 0.4% |
| VariableForce's `lvasgn` keys built for files without a loop | 6.2% to 0.9% | 5.1% to 0.4% | 0.4% to 0.1% |
| depth guard walking the tree up to four times | 1.8% to 0.6% | 1.5% to 0.7% | 0.5% to 0.2% |
| `Source::comment_on_line` scanning every comment | 1.3% to 0.03% | 0.06% to 0% | 0.06% to 0% |
| fancy-regex compiling an unused anchored copy (RSpec/SpecFilePathFormat, per file) | | | 2.9 to 2.0 per parse sample |

Each corpus with its own config, all 10 CPUs of the container, `--cache false --format
quiet`, master 64b4a66f against the branch (release builds), alternating, median of 3. Load
average before the runs 4.1 to 8.1, mostly the previous run decaying; two single-threaded
RuboCop processes of another job ran throughout. Output was identical (`--format emacs` and
exit status, also on jekyll), and the gates below are unchanged.

| corpus | wall s, before | wall s, after | user s, before | user s, after |
|---|---:|---:|---:|---:|
| rails | 1.66 | 1.40 | 9.46 | 7.37 |
| discourse | 5.59 | 4.25 | 34.2 | 23.9 |
| mastodon | 3.08 | 2.54 | 21.6 | 18.0 |

What is left: on discourse `relevant` still takes about 5% (a list no other cop of the config
has is matched once per file and cop); the rest is the cops and their shared
analyses (token translation, charged to Style/Semicolon or Layout/ExtraSpacing as the first to
ask; VariableForce, charged to Lint/ShadowedArgument or Lint/UselessAssignment), parsing, and
the walker's dispatch.

## Reproducing

CI-shaped runs (Linux, needs `taskset` and RuboCop 1.91 on `PATH`):

    PROJECT_CONFIG=1 scripts/bench-ci.sh ci.tsv 3 rubocop-par,rbcop,rubocop-single <corpus dir>...
    scripts/bench-ci.sh boot.tsv 3 boot <dir with a Gemfile that has rubocop>
    PROJECT_CONFIG=1 FILE=app/models/account.rb scripts/bench-ci.sh file.tsv 3 file <mastodon>

It times each tool pinned to an emulated runner (`CPUS_2`, `CPUS_4` override the CPU lists), with
a fresh RuboCop cache root per run, and redoes a run when a CI gate ran meanwhile. Without
`PROJECT_CONFIG=1` both tools run with `--force-default-config`. Rows are rep, corpus, vCPUs,
tool, wall, user, sys, exit status, load average, contaminated, start time (epoch seconds).

Per-PR comparisons used `/usr/bin/time -l` on macOS (instructions retired, cycles elapsed) with
the builds run round-robin, default config (`--force-default-config`) and all cops
(`--only` with every cop `--show-cops` lists except Lint/RedundantCopDisableDirective).
