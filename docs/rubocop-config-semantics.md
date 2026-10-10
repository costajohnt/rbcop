# RuboCop configuration semantics

Read from RuboCop 1.88.2 source (`lib/rubocop/`); spot-check against 1.91 when porting. This is the spec `src/config.rs` follows.

## 1. Which config applies to a file

- `ConfigStore#for_file(f)` = `for_dir(dirname(f))`. `-c` / `--force-default-config` override everything.
- `ConfigFinder#find_config_path(dir)`: nearest `.rubocop.yml` walking up from **the file's directory**, stopping (inclusive) at `project_root` = dir of the **topmost** `Gemfile` (else `gems.rb`) above cwd; with no Gemfile, walks to `/`.
- Fallbacks in order: `<root>/.config/.rubocop.yml`, `<root>/.config/rubocop/config.yml`, `~/.rubocop.yml`, `$XDG_CONFIG_HOME/rubocop/config.yml` (default `~/.config`), then default.yml.
- `add_excludes_from_files`: the **topmost** `.rubocop.yml` up to project_root (if different from the chosen one) has its `AllCops/Exclude` entries appended (dedup) to the chosen config's AllCops/Exclude before the default merge; relative entries joined with that file's dir. `--ignore-parent-exclusion` disables.

## 2. Loading one file (`load_file`)

1. Read UTF-8, run through **ERB** with cwd = config dir, YAML parse (Psych; only `Regexp` and `Symbol` classes allowed; `!ruby/regexp /x/` becomes a Regexp). Duplicate keys: last wins. Empty file is `{}`.
2. Delete `plugins` and `require` (they load code; never merged).
3. `inherit_gem`: for each gem (hash order), its paths are **unshifted** onto `inherit_from` (so all gem configs come before local inherit_from entries; an earlier gem ends up after a later gem, so the earlier gem wins). Gem dir via Bundler specs, else `Gem::Specification.find_by_name`. `rubocop` itself is an error.
4. `resolve_inheritance`: each `inherit_from` entry is loaded recursively (`File.expand_path(entry, dir_of_this_file)`; glob entries expand relative to cwd; http(s) URLs are remote configs). Bases are processed in **reverse** order, each merged *under* the accumulated hash: the file itself beats every base, and a later inherit_from entry beats an earlier one. Only **Hash-valued** top-level keys are taken from bases.
   - Before merging each base: `override_department_setting_for_cops` (if the department is disabled in base or derived, a derived cop with truthy Enabled gets `'override_department'`), and `override_enabled_for_disabled_departments` (department disabled in derived and base cop `Enabled: true` means derived gets `Enabled: false` for it).
   - If only the base has `Include` for a key and the base file name starts with `.rubocop`, its Include entries are rebased: `relative_path(join(base_dir, inc), derived_dir)`.
5. Delete `inherit_from`; qualify bare cop names (`Debugger` to `Lint/Debugger`).
6. `Config#check`: every key's `Exclude` String entries that are not absolute become `expand_path(join(base_dir, e))` with this file's base dir (§5). **Include is never made absolute.**

## 3. Merge

```ruby
def merge(base, derived, **opts)
  result = base.merge(derived)
  (base.keys & derived.keys).each do |key|
    if opts[:unset_nil] && derived[key].nil? then result.delete(key)
    elsif both Hash then result[key] = merge(base[key], derived[key], **opts)
    elsif should_union?(derived, base, opts[:inherit_mode], key)
      result[key] = Array(base[key]) | Array(derived[key])   # ordered union, base first
    end
  end
  result
end
```

- Hashes deep-merge at every level (so hash-valued params like `DebuggerMethods` merge group by group; `Group: ~` deletes a group in the default merge). Arrays and scalars: derived replaces.
- `unset_nil` only in the final merge with defaults.
- `inherit_mode: {merge: [...], override: [...]}` (keys are parameter names), top level or per section. `should_union?` only for Array values: derived section mode (override beats merge), then base section mode, then root mode. Root mode = cop-local mode else file top-level mode during inheritance; in the default merge it is the user file's top-level `inherit_mode`.
- A base's top-level `inherit_mode` hash is copied/merged into the derived hash like any Hash key.

## 4. Merge with defaults, enabling

```ruby
disabled_by_default, enabled_by_default = AllCops DisabledByDefault / EnabledByDefault (CLI flags win)
default = transform(default) { |p| p.merge('Enabled' => !disabled_by_default) } if either
config = handle_disabled_by_default(config, default) if disabled_by_default
override_enabled_for_disabled_departments(default, config)
Config.new(merge(default, config, inherit_mode: config['inherit_mode'] || {}, unset_nil: true))
```

- Preview (1.91): preview is on if `--preview`/`--no-preview` says so, else if the hash's own
  `AllCops: Preview == true`. `apply_preview_defaults` runs on the defaults before everything above:
  each top-level entry's `Preview` hash is shallow-merged over it under preview, and the `Preview` key is
  dropped either way (default.yml's `AllCops: Preview` carries `FailLevel: warning`). With no user file
  (or `--force-default-config`) only the CLI flag counts. Under preview, every inherit_mode used in a
  merge (`determine_inherit_mode` during inheritance, the default merge's root mode) gets `Exclude` added
  to `merge` unless it already lists `Exclude` in `merge` or `override`. `Enabled: preview` cops run only
  under preview.
- `handle_disabled_by_default`: for each user department key (no `/`) with truthy `Enabled` (checked before the transform below), restore the real default `Enabled` for that department's cops. Then every user top-level key gets `{'Enabled' => true}.merge(params)`, so any cop key present (inherited ones included) is enabled unless it says otherwise.
- `enable_cop?(name, opts)`: true if `Enabled == true` (or Lint/Syntax); `cop_enabled = opts.fetch('Enabled') { !DisabledByDefault }`; `'override_department'` means true; false if department `Enabled == false`; else `cop_enabled`. Department = everything before the last `/`.
- `Registry#enabled?`: `--only` lists force-enable. Otherwise `Enabled == true`, or `== 'pending'` with `AllCops/NewCops: enable` or `--enable-pending-cops` (`--disable-pending-cops` wins). Any other value is off.
- Cop config seen by the cop = `department_config.merge(cop_config)` (shallow).
- A `# rubocop:enable X` comment in a file opts X in for that file regardless of Enabled.

## 5. Paths

- `base_dir_for_path_parameters`: dir of the loaded file if its basename starts with `.rubocop` and it is not `~/.rubocop.yml`; otherwise **cwd** (default.yml, gem configs, XDG, other names). For the merged config, `loaded_path` is the user's config file.
- `match_path?(pattern, path)`: String equal to path, or (if glob chars `[*{\[?]`) `File.fnmatch?(pattern, path, FNM_PATHNAME | FNM_EXTGLOB)` (`..`-prefixed patterns expanded first), or the hidden-file case: basename starts with `.`, no hidden dir component, and fnmatch with `FNM_DOTMATCH` added. Regexp: unanchored `match?`.
- AllCops `file_to_include?(f)`: rel = path relative to base dir; hidden rel (`.` prefix, not `..`) is skipped unless some Include pattern is a Regexp, starts with `.`, or contains `/.`; pattern matched against the absolute path if the pattern is absolute or starts with `..` or rel starts with `..`, else against rel.
- `file_to_exclude?(f)`: any Exclude pattern matches the absolute path.
- User `AllCops/Exclude` and `Include` **replace** the defaults unless `inherit_mode: merge`.

## 6. File discovery (TargetFinder)

- Directory args: glob everything (dotfiles included); config = the **argument directory's** config only. If the arg dir itself is hidden, only `ruby_file?` applies. Else keep file if not `file_to_exclude?` and (`ruby_file?` and not under a hidden path relative to the arg dir, or `file_to_include?`).
- `ruby_file?`: extension from cwd config Include patterns that start with `**/*.`; filename from other Include patterns (`**/` stripped); or extensionless non-empty file whose first line matches `/#!.*(ruby|rake|jruby)/` (RubyInterpreters from the file's config).
- Explicit file args are **not** Exclude-filtered unless `--force-exclusion` (cwd config, or the file's config with `--ignore-parent-exclusion`).
- Dir pruning by `.../**/*` Excludes is only an optimisation.

## 7. Does a cop run on a file

Per-file config. Skip if cop `excluded_file?`; run if opted in by `# rubocop:enable`; else need `Registry#enabled?`. `relevant_file?`: cop (department-merged) `Include` must match (absent = all) and `Exclude` must not (absent = none). Patterns are tried against the path relative to the config base dir, then: for Include only absolute or `..` patterns against the absolute path; for Exclude, against the path as given (absolute). Non-glob strings match exactly. AllCops Include/Exclude are not consulted here.
