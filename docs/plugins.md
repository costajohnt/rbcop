# Plugins

rbcop implements some RuboCop plugin gems natively. A project that lists one under `plugins:` (or
the older `require:`) gets that plugin's cops, with the same offenses and `-a`/`-A` output as RuboCop
running the pinned plugin version below.

| Gem | Version | Departments | Cops ported | Issue |
|---|---|---|---|---|
| rubocop-performance | 1.27.0 | Performance | all 52 | #283 |
| rubocop-rails | 2.38.0 | Rails | all 138 | #284 |
| rubocop-minitest | 0.41.0 | Minitest | all 55 | #286 |
| rubocop-rspec | 3.10.2 | RSpec | all 115 | #285 |
| rubocop-rspec_rails | 2.33.0 | RSpecRails | all 10 | #285 |
| rubocop-factory_bot | 2.28.0 | FactoryBot | all 11 | #285 |
| rubocop-capybara | 3.0.0 | Capybara, Capybara/RSpec | all 15 | #285 |

Each version is the newest release that supports RuboCop 1.91.0 (rubocop-performance 1.27.0
requires `rubocop >= 1.89.0, < 2.0`, released 2026-08-16; rubocop-rails 2.38.0 requires
`rubocop >= 1.89.0, < 2.0`, released 2026-09-20;
rubocop-minitest 0.41.0 requires `rubocop >= 1.89.0, < 2.0`, released 2026-10-02;
rubocop-rspec 3.10.2 requires `rubocop ~> 1.86, >= 1.86.2`; rubocop-rspec_rails 2.33.0 requires
`rubocop ~> 1.89` and `rubocop-rspec ~> 3.5`, released 2026-10-07; rubocop-factory_bot 2.28.0
requires `rubocop ~> 1.72, >= 1.72.1`; rubocop-capybara 3.0.0 requires `rubocop ~> 1.81`, released
2026-06-22). The gem's `config/default.yml` is vendored
as `config/plugins/<gem>.yml` (MIT, `config/plugins/LICENSE-<gem>`), so the gem need not be
installed; a different installed version is ignored.

## How it works

- `src/helpers/cop_registry.rs` `NATIVE_PLUGINS` lists each native plugin: gem name, departments and
  vendored default.yml.
- When a config's `plugins:` / `require:` names one, `Store::integrate_plugins` registers its cops
  (`register_plugin`) and merges its default.yml as RuboCop's ConfigurationIntegrator does.
- Its cops are ordinary cops in `src/cops/<department>/` (same `Cop` trait, dispatch and helpers).
  `--show-cops`, `--only`/`--except` and the junit formatter skip a native plugin's cops until it is
  loaded (`cop_registry::is_registered`), as RuboCop's registry only holds them then. The runner
  goes by each config's team (`Resolved::team_has`): Runner caches a config's cops when its first
  file is inspected, so a plugin that only a later config loads (`plugins:` in a subdirectory's
  `.rubocop.yml`) adds no cops to the configs whose files came before, while a config mobilized
  after it runs the plugin's cops, enabled by default where that config has no entry for them.
- `REGISTRY` (directives, the validator, `--only`, `--list-enabled-cops-for`, `--show-docs-url`)
  holds RuboCop's cops, then each loaded plugin's. Plugins rbcop does not implement still add the
  cop classes their gem's `lib/` defines (`register_foreign`, `ruby_cops` in `src/config/mod.rs`),
  so directives naming them are not "unknown" and `--show-cops` lists them; since rbcop
  cannot run those cops (nor custom cops from a `require:`d Ruby file), a directive naming one is
  never reported as redundant, and neither is a `disable all` while such cops are loaded.
- `--only` names are qualified once configs have loaded (plugins add departments and cop names);
  `Store::set_only` then re-resolves the configs already loaded.
- Autocorrect order and `autocorrect_incompatible_with` come from `src/autocorrect/registry.tsv`
  (`scripts/autocorrect_registry.rb` requires each native plugin): RuboCop's cops, then each native
  plugin's in the order the plugins were loaded (`plugins:` order), as the registry (and the junit
  formatter's testcases) list them.

## rubocop-rails specifics

- `TargetRailsVersion` comes from `AllCops: TargetRailsVersion`, else the `railties` version in the
  `Gemfile.lock` / `gems.locked` found upward from the config's directory (as Bundler resolves it,
  so a lockfile with a `PATH` section and no Gemfile reachable from the working directory is
  ignored), else 5.0. Cops with a minimum Rails version are skipped below it.
- `Include`/`Exclude` defaults and per-cop settings come from the vendored default.yml; obsolete
  parameters come from `config/plugins/rubocop-rails-obsoletion.yml`.
- Inflection (`pluralize`, `singularize`, `underscore`, `camelize`, `tableize`) follows
  ActiveSupport's default English inflections (`src/helpers/inflector.rs`), checked against
  activesupport 8.1.4.
- `db/schema.rb` is found upward from the working directory and read once per run
  (`src/helpers/schema.rs`, rubocop-rails' SchemaLoader), for Rails/UniqueValidationWithoutIndex and
  Rails/UnusedIgnoredColumns. `config/database.yml` (from the working directory) and `DATABASE_URL`
  set the database for Rails/BulkChangeTable when its `Database` is unset.
- Some Rails corrections that splice string content into new code are withheld when that content is
  not inert there, where RuboCop writes it unescaped; see the deliberate differences in
  docs/known-gaps.md.

## rubocop-rspec family specifics

- rubocop-rspec, rubocop-rspec_rails, rubocop-factory_bot and rubocop-capybara are four native
  plugins, each loaded by its own `plugins:` entry. Shared code lives in `src/helpers/rspec.rs`
  (Language, the ExampleGroup/Example/Hook concepts and the mixins), `src/helpers/factory_bot.rs`
  and `src/helpers/capybara.rs` (CssSelector, CssAttributesParser, CapybaraHelp).
- `RSpec: Language` (the DSL names: example groups, examples, hooks, helpers, includes, shared
  groups, subjects, expectations) is read from the resolved config of each file, once per config,
  so a project's own aliases (`RSpec: Language: Examples: Regular: [scenario]`, inherited or merged
  like any department setting) apply to every RSpec and RSpecRails cop.
- A cop's config is merged with its department's (`RSpec:`, `FactoryBot:`, `Capybara:`,
  `Capybara/RSpec:`), as RuboCop's `Config#for_badge`: the department `Include` (for RSpec,
  `**/*_spec.rb` and `**/spec/**/*`) applies to each cop that has none of its own, and department
  and cop `Exclude` lists are unioned.
- `Capybara/RSpec` is a nested department: `Capybara/RSpec/HaveSelector` and so on. `--only
  Capybara` does not select them (RuboCop's department of such a cop is `Capybara/RSpec`).
- rubocop-rspec_rails requires rubocop-rspec (its cops include rubocop-rspec mixins). RuboCop fails
  to load it alone; rbcop then runs its cops with rubocop-rspec's default `RSpec/Language`
  (docs/known-gaps.md).
- RSpec/SpecFilePathFormat's `EnforcedInflector: active_support` uses ActiveSupport's default
  inflections (`src/helpers/inflector.rs`); an `InflectorPath` file is not evaluated.
- rubocop-rspec's obsoletion file is vendored as `config/plugins/rubocop-rspec-obsoletion.yml`.
- Corrections that would write string or symbol content into new literals are withheld when that
  content is not inert; see docs/known-gaps.md.
- The four plugins share one oracle set (`oracle-rspec`, recorded with `config/all-cops-rspec.yml`)
  and one `scripts/check-plugins.sh` iteration (`rspec`), which also checks Metrics/BlockLength
  (rubocop-rspec's config adds spec files to its Exclude) and mastodon under its own `.rubocop.yml`.

## Porting a plugin

1. Pin the version: the newest release whose gemspec accepts RuboCop 1.91.0. Install it with the
   rbenv Ruby on every machine that runs the oracle (same version on all) and add it to the table above.
2. Vendor its `config/default.yml` as `config/plugins/<gem>.yml`, add an entry to `NATIVE_PLUGINS`,
   regenerate `src/autocorrect/registry.tsv`, and add `config/all-cops-<plugin>.yml` (oracle config)
   and `config/plugin-<plugin>.yml` (autocorrect gate config).
3. Record oracles with the plugin on every corpus:
   `CONFIG=$PWD/config/all-cops-<plugin>.yml scripts/oracle.sh $RBCOP_CORPUS/<c> $ORACLES/oracle-<plugin>/<c>.txt`
   for the nine corpora and the corpus2 projects (oracle dir next to `$RBCOP_ORACLE`).
4. Record the gem's spec examples into `tests/spec_corpus` from the gem's source at the pinned tag:
   `PLUGIN=<gem> scripts/record-spec-corpus.sh <gem-src>` (only that plugin's cop dirs are replaced).
   A gem tested with Minitest (`test/rubocop/cop`, rubocop-minitest) is extracted by running its
   tests with the installed gem (scripts/extract_minitest_corpus.rb).
5. Port the cops as in docs/porting-cops.md (Ruby source in the installed gem). Quick loop:
   `SPEC_CORPUS_ONLY=<Dept>_ cargo test --release --test spec_corpus`.
6. Add the plugin to the loop in `scripts/check-plugins.sh`, which `scripts/ci.sh` runs: offenses on
   all corpora must match the plugin oracles exactly, and `-A` must match live RuboCop.
