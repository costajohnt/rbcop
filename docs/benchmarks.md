# Benchmarks

How fast rbcop is against RuboCop 1.92.2 in the setting most users care about: a CI step on a
GitHub-hosted runner, which starts with no RuboCop result cache unless the workflow restores one.
Tracking issues: #388, #808.

## CI-shaped runs (Linux container)

rbcop at e2804db7 against RuboCop 1.92.2, each corpus linted with its own `.rubocop.yml` (brew:
default config), pinned to 2 and 4 vCPUs, RuboCop with an empty result cache. Median of 3 runs,
wall seconds. rbcop runs as users get it: `RBCOP_FALLBACK` unset, so cops it does not implement
run in a RuboCop child (README, "Plugins and custom cops"). The rbcop rows of discourse, mastodon,
solidus and fastlane and the one-file rows were rerun at 1a576635, where that child inspects in
parallel (#831); the RuboCop rows are from the e2804db7 run on the same machine. Raw rows:
[benchmarks/ci.tsv](benchmarks/ci.tsv), [benchmarks/boot.tsv](benchmarks/boot.tsv),
[benchmarks/file.tsv](benchmarks/file.tsv).

| corpus | files | RuboCop, 1 process, 2 vCPU | RuboCop `--parallel`, 2 vCPU | rbcop, 2 vCPU | speedup, 2 vCPU | RuboCop `--parallel`, 4 vCPU | rbcop, 4 vCPU | speedup, 4 vCPU |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| rails | 3,476 | 109.6 | 99.0 | 3.78 | 26.2x | 51.2 | 2.02 | 25.4x |
| discourse | 12,441 | 306.8 | 274.9 | 92.34 | 3.0x | 142.4 | 48.67 | 2.9x |
| mastodon | 3,297 | 138.5 | 118.3 | 17.65 | 6.7x | 61.8 | 9.73 | 6.4x |
| brew (default config) | 1,977 | 156.6 | 138.8 | 6.50 | 21.4x | 71.1 | 3.51 | 20.3x |
| solidus | 2,171 | 60.4 | 54.0 | 18.25 | 3.0x | 28.5 | 9.78 | 2.9x |
| fastlane | 1,355 | 66.4 | 58.3 | 24.08 | 2.4x | 30.6 | 12.93 | 2.4x |

Speedup is RuboCop `--parallel` over rbcop at the same vCPU count. Against single-process RuboCop
at 2 vCPUs it is 2.8x (fastlane) to 29.0x (rails). Files are those `rubocop -L` lists.

On rails and brew rbcop implements every enabled cop and starts no RuboCop. discourse, mastodon,
solidus and fastlane enable cops rbcop does not implement (21 rubocop-discourse cops on
discourse; on the other three the cops of the Ruby files their configs `require:`), so rbcop
starts RuboCop for them. That child parses every file one of those cops may inspect, in
RuboCop's parallel worker processes (`--parallel` with a cold result cache of its own, #831),
and it is most of rbcop's time there. At 2 vCPUs its two workers share one physical core, so it
gains up to 7% over the single process it was before #831 (mastodon none); at 4 vCPUs it halves rbcop's wall time
(discourse 90.27 s to 48.67 s, fastlane 24.13 s to 12.93 s). The same runs with
`RBCOP_FALLBACK=0` (those cops skipped, with a notice on stderr):

| corpus | rbcop, 2 vCPU | `RBCOP_FALLBACK=0`, 2 vCPU | speedup, 2 vCPU | rbcop, 4 vCPU | `RBCOP_FALLBACK=0`, 4 vCPU | speedup, 4 vCPU |
|---|---:|---:|---:|---:|---:|---:|
| discourse | 92.34 | 11.92 | 23.1x | 48.67 | 6.80 | 20.9x |
| mastodon | 17.65 | 7.30 | 16.2x | 9.73 | 3.91 | 15.8x |
| solidus | 18.25 | 2.75 | 19.6x | 9.78 | 1.53 | 18.6x |
| fastlane | 24.08 | 2.92 | 20.0x | 12.93 | 1.57 | 19.5x |

The speedup columns here are RuboCop `--parallel` over rbcop with `RBCOP_FALLBACK=0`. On these
trees the skipped cops report no offenses, so the output is the same either way (see Method);
on another tree they may.

CPU time (user) per run, which does not depend on the pinning: rails 194 s RuboCop `--parallel`
vs 7.2 s rbcop, discourse 538 s vs 177 s (22 s with `RBCOP_FALLBACK=0`), mastodon 232 s vs 33 s
(14 s), brew 273 s vs 12 s, solidus 105 s vs 34 s (5.2 s), fastlane 114 s vs 46 s (5.6 s) (2
vCPU rows). The fallback child's parallel workers cost about 65% more CPU time than its single
process did (discourse 107 s, mastodon 24 s, solidus 21 s, fastlane 28 s before #831): each
worker builds its own cop teams, and hyperthread pairs run slower per thread. RuboCop's `--parallel` user time is about 1.8x its single-process time because its
two workers share one physical core; wall time improves 10 to 15% over a single process at 2
vCPUs.

A RuboCop run with a warm cache (the same `--cache-root` again on the unchanged tree, the
`rubocop-par-warm` rows) took 2.1 s (rails), 6.8 s (discourse), 3.5 s (mastodon), 5.5 s (brew),
2.7 s (solidus) and 2.1 s (fastlane) at 4 vCPUs. That is not what the tables above measure (see
Caveats).

Start-up and one file (2 vCPUs, median of 3):

| run | wall s |
|---|---:|
| `rubocop --version` | 0.69 |
| `bundle exec rubocop --version` (Gemfile with rubocop, -performance, -rails, -rspec) | 0.93 |
| `rbcop --version` | 0.07 |
| RuboCop on mastodon's `app/models/account.rb` (mastodon's config, 6 plugins), empty cache | 2.44 |
| rbcop on the same file | 1.17 |
| rbcop on the same file, `RBCOP_FALLBACK=0` | 0.24 |

### Method

- Machine: CT 440, a Linux container on an Intel Core i9-8950HK host (6 cores / 12 threads; the
  container sees CPUs 0-9), Debian 13.7, kernel 7.0.14-30-pve, x86_64. Ruby 4.0.6 with
  RuboCop 1.92.2 (Parser 3.3.12.0, rubocop-ast 1.50.0), rubocop-performance 1.27.0,
  rubocop-rails 2.38.0, rubocop-rspec 3.10.2, rubocop-rspec_rails 2.33.0, rubocop-capybara 3.1.0,
  rubocop-factory_bot 2.28.0, rubocop-minitest 0.41.0, rubocop-i18n 3.3.0, rubocop-packaging
  0.6.0 (the versions docs/plugins.md pins), and for discourse rubocop-discourse 3.20.0 with
  rubocop-discourse-base 1.0.0 (the versions discourse's Gemfile.lock has). rbcop release build
  of e2804db7, built fresh for the run, and of 1a576635 for the #831 rerun (rbcop rows of
  discourse, mastodon, solidus, fastlane and the one-file rows).
- RuboCop invocation: rails, mastodon, brew, solidus and fastlane ran RuboCop from a gem home
  that holds RuboCop 1.92.2 and the plugins above (a wrapper that sets `GEM_HOME`/`GEM_PATH` and
  runs `ruby .../bin/rubocop`). discourse needs rubocop-discourse, which that gem home lacks, so
  it ran `bundle exec rubocop` (Bundler 4.0.16) with a Gemfile that pins the same versions plus
  rubocop-discourse. rbcop's fallback ran the same RuboCop as the corpus (`RBCOP_RUBOCOP`). The
  `--version` rows ran the gem home wrapper and, for `bundle exec`, a Gemfile with rubocop,
  rubocop-performance, rubocop-rails and rubocop-rspec at the versions above.
- `scripts/bench-ci.sh` with `PROJECT_CONFIG=1` (brew without it), 3 reps. Each rep runs every
  corpus and tool in turn, so slow drift spreads over all rows (the script ran once per corpus
  and rep, to give each corpus its RuboCop). 2 vCPUs are CPUs 1,7 (one physical core, both
  hyperthreads), 4 vCPUs are 1,7,2,8 (two cores), on the assumption that a hosted runner's
  vCPUs are hyperthreads. Single-process RuboCop ran at 2 vCPUs only; it uses one thread, so 4
  vCPUs would not change it.
- RuboCop `--parallel` got a fresh `--cache-root` per run (cold cache, as on a runner without a
  restored cache), then a second run on the same root (warm); single-process RuboCop ran with
  `--cache false`. rbcop ran with `--cache false`; its RuboCop child then still inspects in parallel, with a cold
  result cache in a directory of its own. The corpus
  files were in the OS page cache for every run.
- Plugins each config loads: rails: minitest, packaging, performance, rails (`DisabledByDefault`,
  so only the cops it lists run). discourse: rubocop-discourse's `stree-compat.yml`, which loads
  capybara, factory_bot, rspec, rails and rubocop-discourse's own cops (`DisabledByDefault`).
  mastodon: capybara, i18n, performance, rails, rspec, rspec_rails, plus a local cop
  (Style/MiddleDot). solidus: performance, rails, plus a local cop. fastlane: performance, plus
  three local cops. brew ran with the default config (see below).
- The corpora were copied, and three configs edited so that RuboCop loads them on this box:
  rails without rubocop-md (not installed; rbcop does not lint Markdown) and its `Markdown:`
  section; fastlane without rubocop-require_tools (not installed) and its
  `Require/MissingRequireStatement` section; solidus without `storefront/.rubocop.yml`, which RuboCop
  1.92.2 rejects (`Metrics/LineLength` moved to `Layout/LineLength`). discourse and mastodon ran unedited. brew is
  `Library/Homebrew` only, whose config inherits `../.rubocop.yml`, which the corpus does not
  have, so it ran with `--force-default-config`.
- Output was compared before timing: each corpus linted once by RuboCop, by rbcop and by rbcop
  with `RBCOP_FALLBACK=0`, with the same config (`--format emacs`). Offenses were identical in all
  three (rails 0, discourse 57, mastodon 17, brew 113,506, solidus 4,172, fastlane 0) and so were
  the exit statuses; every timed run's exit status also matches between the tools (see the
  TSVs).
- Spread: the 3 runs of each corpus row were within 4% of each other, except RuboCop's warm runs
  at 4 vCPUs (solidus 2.59 to 2.93 s, discourse 6.78 to 7.15 s); one-file and `--version` rows
  are below a second and vary more (`rbcop --version` 0.03 to 0.09 s).

### Load and contamination

The box was reserved for this benchmark. No gate (`ci-run.sh`) ran during any run: the
contaminated column is 0 in all 168 rows. Before the runs no other process was busy on the box
(`ps`), and no cargo, RuboCop, RSpec or gate process other than the benchmark's own showed up in
`ps` samples taken every 10 s through the runs. The load average recorded before each run (0.56
to 4.38) is mostly the previous run decaying, since runs follow each other; the container sees
the host's load average. The other containers on the host were not sampled this time.

The #831 rerun ran on the same box, idle inside the container before it started (`ps`; no
`ps` sampling during it), with no gate during any run (contaminated 0 in every row), each run
interleaved with the same run of ae3ca215 (the commit before) as the before. Before timing, both
printed byte-identical stdout, stderr and exit status on the four corpora (`--format emacs`,
cold cache, warm cache and `--cache false`), the same as RuboCop's but for one rbenv warning on
mastodon's stderr from both builds (its `.ruby-version` names a Ruby the box does not have).

### Caveats

- This is not a GitHub runner. It is a container on a 2018 laptop CPU; hosted runners use other
  (server) CPUs, so absolute times there will differ. The ratios are more likely to carry over,
  but that is not verified.
- The 2- and 4-vCPU shapes assume a vCPU is one hyperthread and that a runner's vCPUs pair up on
  physical cores. If a runner's vCPUs are on separate cores, RuboCop `--parallel` gains more from
  its second worker than it does here (10 to 15%), and the 2-vCPU speedups shrink.
- Only lint time is measured: not checkout, Ruby setup or `bundle install`, which a CI job pays
  for RuboCop and its plugins and which the rbcop binary (action or gem) does not need unless
  the fallback runs.
- RuboCop's cache is cold, as on a runner without a restored cache. A workflow that restores
  RuboCop's cache from an earlier run on the same files is not what the tables measure: on these
  trees such a run took 2 to 7 s, faster than rbcop with the fallback on discourse, mastodon,
  solidus and fastlane.
- The 1.91.0 numbers this replaces (rbcop at 4b08150, RBCOP_FALLBACK off) are in git history:
  rbcop at 4 vCPUs was 2.64 s (rails), 10.16 s (discourse), 6.10 s (mastodon), 4.39 s (brew),
  2.12 s (solidus) and 2.02 s (fastlane).

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

## Hashers, allocator and release profile (#781)

Linux box of the CI-shaped runs, shared with other jobs (host load average 4 to 10), mastodon
with its own config, `--cache false`, pinned to 4 CPUs (`taskset -c 2-5`; unpinned runs of one
build varied 3x in CPU time). Builds interleaved in alternating order, 20 rounds; user+sys CPU
seconds, min (p25 within 0.3 points of it). Output identical in every run.

| change | CPU s, before | CPU s, after | change |
|---|---:|---:|---:|
| integer keys hashed with one multiply (`config::Fnv`), no double hash in `Fixes.seen` | 11.09 | 11.01 | -0.7% |
| Layout/ExtraSpacing slices a line's tokens (no by-line map), with the rest applied | 10.03 | 10.01 | -0.2% |
| mimalloc on glibc (it was musl only) | 10.91 | 9.99 | -8.4% |
| fat LTO instead of thin (release archives only) | 10.06 | 9.64 | -4.2% |

`--profile` per cop (min of 8 runs): the hashers took Lint/UselessAssignment (VariableForce) from
39.5 to 32.6 ms and Layout/IndentationWidth from 133 to 129 ms; the line slices took
Layout/ExtraSpacing from 442 to 406 ms (-8%).
One file (mastodon's `app/models/account.rb`, unpinned, 30 rounds): mimalloc 0.16 to 0.14 s.
Warm result cache (mastodon, 4 CPUs, 20 rounds), all four changes but fat LTO: 0.33 to 0.31 s.

Release profile, release build of rbcop alone (deps built) on the same box: thin LTO 166 s, thin
with `codegen-units = 1` 706 s and no faster at run time, fat LTO 262 s, fat with
`codegen-units = 1` 411 s and no faster than fat. The x86_64 glibc 2.17 archive built by
`scripts/dist/build.sh` with fat LTO and mimalloc still needs nothing newer than GLIBC_2.17. So `Cargo.toml` keeps thin LTO for gates and
local builds, and `scripts/dist/build.sh` builds the archives with fat LTO. `panic` stays
`unwind`: cop errors unwind to the callback's guard. PGO was not tried.

## Reproducing

CI-shaped runs (Linux, needs `taskset` and RuboCop 1.92.2 with the plugins the corpus loads;
`RUBOCOP` names it, `RBCOP_RUBOCOP` the one rbcop's fallback runs):

    PROJECT_CONFIG=1 scripts/bench-ci.sh ci.tsv 3 rubocop-par,rbcop,rbcop-nofallback,rubocop-single <corpus dir>...
    scripts/bench-ci.sh boot.tsv 3 boot <dir with a Gemfile that has rubocop>
    PROJECT_CONFIG=1 FILE=app/models/account.rb scripts/bench-ci.sh file.tsv 3 file <mastodon>

It times each tool pinned to an emulated runner (`CPUS_2`, `CPUS_4` override the CPU lists), with
a fresh RuboCop cache root per run, and redoes a run when a CI gate ran meanwhile. `rbcop` runs
with `RBCOP_FALLBACK` unset, `rbcop-nofallback` with `RBCOP_FALLBACK=0`. Without
`PROJECT_CONFIG=1` both tools run with `--force-default-config`. Rows are rep, corpus, vCPUs,
tool, wall, user, sys, exit status, load average, contaminated, start time (epoch seconds).

Per-PR comparisons used `/usr/bin/time -l` on macOS (instructions retired, cycles elapsed) with
the builds run round-robin, default config (`--force-default-config`) and all cops
(`--only` with every cop `--show-cops` lists except Lint/RedundantCopDisableDirective).
