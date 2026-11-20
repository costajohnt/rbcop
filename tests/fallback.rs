//! Cops rbcop cannot run: the stderr notice, and RBCOP_FALLBACK=1 (src/fallback.rs) compared with
//! a live RuboCop 1.92.2 when one is on PATH.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const COP: &str = r#"module RuboCop
  module Cop
    module Custom
      class NoFoo < Base
        extend AutoCorrector
        MSG = "Avoid `foo`.	Tab and \\ backslash."
        def on_send(node)
          return unless node.method?(:foo) && node.receiver.nil?
          add_offense(node.loc.selector) { |c| c.replace(node.loc.selector, 'bar') }
        end
      end
    end
  end
end
"#;

const CONFIG: &str = "require:\n  - ./no_foo.rb\nAllCops:\n  NewCops: enable\n  SuggestExtensions: false\n";

/// A test project directory, removed when dropped (also when the test fails).
struct Project(PathBuf);

impl std::ops::Deref for Project {
    type Target = PathBuf;
    fn deref(&self) -> &PathBuf {
        &self.0
    }
}

impl AsRef<Path> for Project {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<std::ffi::OsStr> for Project {
    fn as_ref(&self) -> &std::ffi::OsStr {
        self.0.as_os_str()
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn project(name: &str) -> Project {
    let dir = std::env::temp_dir().join(format!("rbcop-fallback-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("Gemfile"), "").unwrap();
    fs::write(dir.join("no_foo.rb"), COP).unwrap();
    fs::write(dir.join(".rubocop.yml"), CONFIG).unwrap();
    fs::write(dir.join("a.rb"), "# frozen_string_literal: true\n\nfoo(1)\nx = \"é\"; foo 2\nputs x\n").unwrap();
    fs::write(dir.join("b.rb"), "# frozen_string_literal: true\n\nfoo(3)\n").unwrap();
    Project(dir)
}

fn rbcop(dir: &Path, fallback: bool, args: &[&str]) -> Output {
    let mut cmd = common::command(env!("CARGO_BIN_EXE_rbcop"));
    // RuboCop needs an existing HOME (CacheConfig.root_dir); the project has no user config.
    cmd.env("HOME", dir);
    if fallback {
        cmd.env("RBCOP_FALLBACK", "1");
    }
    cmd.args(args).current_dir(dir).output().unwrap()
}

fn rubocop(dir: &Path, args: &[&str]) -> Output {
    common::command("rubocop").env("HOME", dir).args(args).current_dir(dir).output().unwrap()
}

fn have_rubocop() -> bool {
    Command::new("rubocop").arg("-v").output().is_ok_and(|o| o.stdout == b"1.92.2\n")
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

#[test]
fn required_ruby_code_is_named_on_stderr() {
    let dir = project("notice");
    let out = rbcop(&dir, false, &["-f", "e"]);
    assert_eq!(
        text(&out.stderr),
        "rbcop: not running cops from Ruby code the config requires; set RBCOP_FALLBACK=1 to run them with RuboCop\n"
    );
    assert!(!text(&out.stdout).contains("Avoid"), "{}", text(&out.stdout));
    // Required code may define any cop, so --only does not silence it.
    let out = rbcop(&dir, false, &["-f", "e", "--only", "Style/StringLiterals"]);
    assert!(text(&out.stderr).contains("cops from Ruby code"), "{}", text(&out.stderr));
    // A plugin's cops are counted by department: a fake plugin gem found through GEM_PATH.
    let gems = dir.join("gems");
    fs::create_dir_all(gems.join("specifications")).unwrap();
    fs::create_dir_all(gems.join("gems/rubocop-fake-0.1.0/config")).unwrap();
    // The feature `plugins:` requires.
    fs::create_dir_all(gems.join("gems/rubocop-fake-0.1.0/lib")).unwrap();
    fs::write(gems.join("gems/rubocop-fake-0.1.0/lib/rubocop-fake.rb"), "").unwrap();
    fs::write(
        gems.join("specifications/rubocop-fake-0.1.0.gemspec"),
        "Gem::Specification.new { |s| s.name = 'rubocop-fake'; s.version = '0.1.0'; \
         s.metadata = { 'default_lint_roller_plugin' => 'RuboCop::Fake::Plugin' } }\n",
    )
    .unwrap();
    fs::write(
        gems.join("gems/rubocop-fake-0.1.0/config/default.yml"),
        "Fake/One:\n  Enabled: true\nFake/Two:\n  Enabled: true\nFake/Off:\n  Enabled: false\nFake/Sub:\n  Enabled: \
         false\nFake/Sub/Three:\n  Enabled: true\n",
    )
    .unwrap();
    // A nested department (rubocop-i18n's I18n/GetText) is not a cop, enabled or not (#423).
    fs::write(
        dir.join(".rubocop.yml"),
        "plugins:\n  - rubocop-fake\nAllCops:\n  NewCops: enable\nFake/Sub:\n  Enabled: true\n",
    )
    .unwrap();
    let run = |args: &[&str]| {
        let mut cmd = common::command(env!("CARGO_BIN_EXE_rbcop"));
        cmd.env("GEM_PATH", &gems).args(args).current_dir(&dir).output().unwrap()
    };
    let out = run(&["-f", "e", "a.rb"]);
    // Without a ruby on PATH the gem cannot be found.
    if !text(&out.stderr).contains("cannot run ruby") {
        assert_eq!(
            text(&out.stderr),
            "rbcop: not running 3 cops it does not implement (Fake: 2, Fake/Sub: 1); set RBCOP_FALLBACK=1 to run \
             them with RuboCop\n"
        );
        // ...and --only/--except leave out what they deselect.
        let out = run(&["-f", "e", "--except", "Fake/One", "a.rb"]);
        assert!(text(&out.stderr).contains("(Fake: 1, Fake/Sub: 1)"), "{}", text(&out.stderr));
        let out = run(&["-f", "e", "--only", "Style/StringLiterals", "a.rb"]);
        assert_eq!(text(&out.stderr), "");
    }
    let _ = fs::remove_dir_all(&dir);
}

/// A `require:` entry RuboCop cannot load (FeatureLoader's LoadError) stops the run with exit 2
/// before anything is inspected, whether a gem, a `./` path or a bare relative path (a $LOAD_PATH
/// feature, not a project file).
#[test]
fn unloadable_requires_abort() {
    let dir = project("unloadable");
    for (entry, message) in [
        ("./nope.rb", format!("{}/./nope.rb", dir.canonicalize().unwrap().display())),
        ("no_foo.rb", "no_foo.rb".to_string()),
        ("rbcop_no_such_gem_x", "rbcop_no_such_gem_x".to_string()),
    ] {
        fs::write(dir.join(".rubocop.yml"), format!("require:\n  - {entry}\n")).unwrap();
        let out = rbcop(&dir, false, &["-f", "e"]);
        if text(&out.stderr).contains("cannot run ruby") {
            continue;
        }
        assert_eq!(text(&out.stderr), format!("cannot load such file -- {message}\n"), "{entry}");
        assert_eq!(text(&out.stdout), "", "{entry}");
        assert_eq!(out.status.code(), Some(2), "{entry}");
    }
    let _ = fs::remove_dir_all(&dir);
}

/// `require:` of a feature path inside a gem (`rubocop/require_tools`) loads that gem, whose cop
/// classes register even without a config/default.yml.
#[test]
fn required_feature_path_registers_its_gems_cops() {
    let dir = project("feature-path");
    let gems = dir.join("gems");
    let gem = gems.join("gems/rubocop-fake_tools-0.1.0");
    fs::create_dir_all(gems.join("specifications")).unwrap();
    fs::create_dir_all(gem.join("lib/rubocop/cop/fake_tools")).unwrap();
    fs::write(
        gems.join("specifications/rubocop-fake_tools-0.1.0.gemspec"),
        "Gem::Specification.new { |s| s.name = 'rubocop-fake_tools'; s.version = '0.1.0' }\n",
    )
    .unwrap();
    fs::write(gem.join("lib/rubocop/fake_tools.rb"), "require_relative 'cop/fake_tools/one'\n").unwrap();
    fs::write(
        gem.join("lib/rubocop/cop/fake_tools/one.rb"),
        "module RuboCop\n  module Cop\n    module FakeTools\n      class One < Base\n      end\n    end\n  end\nend\n",
    )
    .unwrap();
    fs::write(dir.join(".rubocop.yml"), "require:\n  - rubocop/fake_tools\nAllCops:\n  NewCops: enable\n").unwrap();
    fs::write(dir.join("a.rb"), "# frozen_string_literal: true\n\nputs 1 # rubocop:disable FakeTools/Nope\n").unwrap();
    let run = |args: &[&str]| {
        let mut cmd = common::command(env!("CARGO_BIN_EXE_rbcop"));
        cmd.env("GEM_PATH", &gems).args(args).current_dir(&dir).output().unwrap()
    };
    let out = run(&["-f", "e", "a.rb"]);
    if !text(&out.stderr).contains("cannot run ruby") {
        assert_eq!(
            text(&out.stderr),
            "rbcop: not running 1 cop it does not implement (FakeTools: 1); set RBCOP_FALLBACK=1 to run them with \
             RuboCop\n"
        );
        assert!(text(&out.stdout).contains("Unknown cop name `FakeTools/Nope`"), "{}", text(&out.stdout));
    }
    let _ = fs::remove_dir_all(&dir);
}

/// A plain `require:` (a gem that is not a lint_roller plugin) loads only the files the feature
/// reaches: a namespace-only file registers no cops and merges no config (#499), the gem's main
/// file registers its cops and, through its Inject.defaults!, its config. Under `plugins:` such a
/// gem fails to load as a plugin, exit 2 (#498).
#[test]
fn plain_require_loads_what_the_feature_requires() {
    let dir = project("plain-require");
    let gems = dir.with_extension("gems");
    let _ = fs::remove_dir_all(&gems);
    let gem = gems.join("gems/rubocop-fake_ns-0.1.0");
    fs::create_dir_all(gems.join("specifications")).unwrap();
    fs::create_dir_all(gem.join("config")).unwrap();
    fs::create_dir_all(gem.join("lib/rubocop/fake_ns")).unwrap();
    fs::create_dir_all(gem.join("lib/rubocop/cop/fake_ns")).unwrap();
    fs::write(
        gems.join("specifications/rubocop-fake_ns-0.1.0.gemspec"),
        "Gem::Specification.new { |s| s.name = 'rubocop-fake_ns'; s.version = '0.1.0' }\n",
    )
    .unwrap();
    fs::write(
        gem.join("config/default.yml"),
        "FakeNs/One:\n  Enabled: true\nStyle/StringLiterals:\n  Enabled: false\n",
    )
    .unwrap();
    fs::write(gem.join("lib/rubocop/fake_ns.rb"), "module RuboCop\n  module FakeNs\n  end\nend\n").unwrap();
    fs::write(
        gem.join("lib/rubocop/fake_ns/plugin.rb"),
        "module RuboCop\n  module FakeNs\n    class Plugin < LintRoller::Plugin\n    end\n  end\nend\n",
    )
    .unwrap();
    // Sets the default configuration only from a method RuboCop would call later (standard's
    // plugin, #518): requiring it merges nothing.
    fs::write(
        gem.join("lib/rubocop/fake_ns/later.rb"),
        "require_relative 'inject'\nmodule RuboCop\n  module FakeNs\n    def self.later\n      \
         ConfigLoader.instance_variable_set(:@default_configuration, nil)\n    end\n  end\nend\n",
    )
    .unwrap();
    fs::write(
        gem.join("lib/rubocop-fake_ns.rb"),
        "require 'rubocop'\nrequire_relative 'rubocop/fake_ns'\nrequire_relative 'rubocop/fake_ns/plugin'\n\
         require_relative 'rubocop/fake_ns/inject'\n\
         RuboCop::FakeNs::Inject.defaults!\nrequire 'rubocop/cop/fake_ns/one'\n",
    )
    .unwrap();
    fs::write(
        gem.join("lib/rubocop/fake_ns/inject.rb"),
        "module RuboCop\n  module FakeNs\n    module Inject\n      def self.defaults!\n        \
         ConfigLoader.instance_variable_set(:@default_configuration, nil)\n      end\n    end\n  end\nend\n",
    )
    .unwrap();
    fs::write(
        gem.join("lib/rubocop/cop/fake_ns/one.rb"),
        "module RuboCop\n  module Cop\n    module FakeNs\n      class One < Base\n      end\n    end\n  end\nend\n",
    )
    .unwrap();
    fs::write(dir.join("a.rb"), "# frozen_string_literal: true\n\nputs 1 # rubocop:disable FakeNs/One\nputs \"x\"\n")
        .unwrap();
    let run = |config: &str| {
        fs::write(dir.join(".rubocop.yml"), format!("{config}AllCops:\n  NewCops: enable\n")).unwrap();
        let mut cmd = common::command(env!("CARGO_BIN_EXE_rbcop"));
        cmd.env("GEM_PATH", &gems).args(["-f", "e", "a.rb"]).current_dir(&dir).output().unwrap()
    };
    let out = run("require:\n  - rubocop/fake_ns\n");
    // Without a ruby on PATH the gem cannot be found.
    if !text(&out.stderr).contains("cannot run ruby") {
        assert_eq!(text(&out.stderr), "");
        assert!(text(&out.stdout).contains("Unknown cop name `FakeNs/One`"), "{}", text(&out.stdout));
        assert!(text(&out.stdout).contains("Style/StringLiterals"), "{}", text(&out.stdout));
        let out = run("require:\n  - rubocop/fake_ns/later\n");
        assert_eq!(text(&out.stderr), "");
        assert!(text(&out.stdout).contains("Style/StringLiterals"), "{}", text(&out.stdout));
        let out = run("require:\n  - rubocop-fake_ns\n");
        assert_eq!(
            text(&out.stderr),
            "rbcop: not running 1 cop it does not implement (FakeNs: 1); set RBCOP_FALLBACK=1 to run them with \
             RuboCop\n"
        );
        assert!(!text(&out.stdout).contains("Unknown cop name"), "{}", text(&out.stdout));
        assert!(!text(&out.stdout).contains("Style/StringLiterals"), "{}", text(&out.stdout));
        let out = run("plugins:\n  - rubocop-fake_ns\n");
        assert_eq!(
            text(&out.stderr),
            "Error: Failed to load plugin `rubocop-fake_ns` because the corresponding plugin class could not be \
             determined for instantiation.\nTry upgrading it first (e.g., `bundle update rubocop-fake_ns`).\nIf \
             `rubocop-fake_ns` is not yet a plugin, use `require: rubocop-fake_ns` instead of `plugins: \
             rubocop-fake_ns` in your configuration.\n\nFor further assistance, check with the developer regarding \
             the following points:\nhttps://docs.rubocop.org/rubocop/plugin_migration_guide.html\n"
        );
        assert_eq!(text(&out.stdout), "");
        assert_eq!(out.status.code(), Some(2));
        // Named with its plugin class, it loads.
        let named = |class: &str| run(&format!("plugins:\n  - rubocop-fake_ns:\n      plugin_class_name: {class}\n"));
        let out = named("RuboCop::FakeNs::Plugin");
        assert!(text(&out.stderr).contains("not running 1 cop it does not implement"), "{}", text(&out.stderr));
        // A name that is no constant, or a module, ends the run (#530). A class in a file the gem's
        // require does not reach is no constant either (#768).
        fs::write(
            gem.join("lib/rubocop/fake_ns/unrequired.rb"),
            "module RuboCop\n  module FakeNs\n    class Unrequired < LintRoller::Plugin\n    end\n  end\nend\n",
        )
        .unwrap();
        for (class, want) in [
            (
                "Nope::Plugin",
                "Failed while configuring plugin `rubocop-fake_ns': no constant with name `Nope::Plugin' was found.\n",
            ),
            (
                "RuboCop::FakeNs::Unrequired",
                "Failed while configuring plugin `rubocop-fake_ns': no constant with name \
                 `RuboCop::FakeNs::Unrequired' was found.\n",
            ),
            ("RuboCop::FakeNs", "undefined method 'new' for module RuboCop::FakeNs\n"),
        ] {
            let out = named(class);
            assert_eq!(text(&out.stderr), want);
            assert_eq!(out.status.code(), Some(2));
        }
        // Inject.defaults! however the main file spells the call, but not in a heredoc (#531).
        for (call, merged) in [
            ("::RuboCop::FakeNs::Inject.defaults!\n", true),
            ("module RuboCop\n  module FakeNs\n    Inject.defaults!\n  end\nend\n", true),
            ("x = 1; RuboCop::FakeNs::Inject.defaults!\n", true),
            ("USAGE = <<~TXT\nRuboCop::FakeNs::Inject.defaults!\nTXT\n", false),
        ] {
            fs::write(
                gem.join("lib/rubocop-fake_ns.rb"),
                format!(
                    "require 'rubocop'\nrequire_relative 'rubocop/fake_ns'\nrequire_relative 'rubocop/fake_ns/plugin'\n\
                     require_relative 'rubocop/fake_ns/inject'\n\
                     {call}require 'rubocop/cop/fake_ns/one'\n"
                ),
            )
            .unwrap();
            let out = run("require:\n  - rubocop-fake_ns\n");
            assert_eq!(text(&out.stdout).contains("Style/StringLiterals"), !merged, "{call:?}: {}", text(&out.stdout));
        }
        // Any Inject method, merging the file its constant names (rubocop-github's
        // default_defaults! and config/default_cops.yml, #558).
        fs::write(
            gem.join("config/default_cops.yml"),
            "FakeNs/One:\n  Enabled: true\nStyle/StringLiterals:\n  Enabled: false\n",
        )
        .unwrap();
        fs::write(gem.join("config/default.yml"), "FakeNs/One:\n  Enabled: true\n").unwrap();
        fs::write(
            gem.join("lib/rubocop/fake_ns/inject.rb"),
            "module RuboCop\n  module FakeNs\n    CONFIG_DEFAULT = PROJECT_ROOT.join(\"config\", \"default_cops.yml\").freeze\n    \
             module Inject\n      def self.default_defaults!\n        _load_config(CONFIG_DEFAULT)\n      end\n\n      \
             def self._load_config(path)\n        ConfigLoader.instance_variable_set(:@default_configuration, path)\n      \
             end\n    end\n  end\nend\n",
        )
        .unwrap();
        fs::write(
            gem.join("lib/rubocop-fake_ns.rb"),
            "require 'rubocop'\nrequire_relative 'rubocop/fake_ns'\nrequire_relative 'rubocop/fake_ns/plugin'\n\
             require_relative 'rubocop/fake_ns/inject'\n\
             RuboCop::FakeNs::Inject.default_defaults!\nrequire 'rubocop/cop/fake_ns/one'\n",
        )
        .unwrap();
        let out = run("require:\n  - rubocop-fake_ns\n");
        assert!(!text(&out.stdout).contains("Style/StringLiterals"), "{}", text(&out.stdout));
        // A native plugin's plugin_class_name is looked up too (#556, #568).
        let perf = gems.join("gems/rubocop-performance-99.0.0");
        fs::create_dir_all(perf.join("lib/rubocop/performance")).unwrap();
        fs::write(
            gems.join("specifications/rubocop-performance-99.0.0.gemspec"),
            "Gem::Specification.new { |s| s.name = 'rubocop-performance'; s.version = '99.0.0'; \
             s.metadata = { 'default_lint_roller_plugin' => 'RuboCop::Performance::Plugin' } }\n",
        )
        .unwrap();
        fs::write(perf.join("lib/rubocop-performance.rb"), "require_relative 'rubocop/performance/plugin'\n").unwrap();
        fs::write(
            perf.join("lib/rubocop/performance/plugin.rb"),
            "module RuboCop\n  module Performance\n    class Plugin < LintRoller::Plugin\n    end\n  end\nend\n",
        )
        .unwrap();
        let native =
            |class: &str| run(&format!("plugins:\n  - rubocop-performance:\n      plugin_class_name: {class}\n"));
        assert_ne!(native("RuboCop::Performance::Plugin").status.code(), Some(2));
        for (class, want) in [
            (
                "Nope",
                "Failed while configuring plugin `rubocop-performance': no constant with name `Nope' was found.\n",
            ),
            ("RuboCop::Performance", "undefined method 'new' for module RuboCop::Performance\n"),
        ] {
            let out = native(class);
            assert_eq!(text(&out.stderr), want);
            assert_eq!(out.status.code(), Some(2));
        }
        // Kernel.const_get also sees the constants of the plugins loaded before (#590): a plugin
        // class loads, another class is no plugin.
        let after_fake = |class: &str| {
            run(&format!(
                "plugins:\n  - rubocop-fake_ns:\n      plugin_class_name: RuboCop::FakeNs::Plugin\n  - rubocop-performance:\n      \
                 plugin_class_name: {class}\n"
            ))
        };
        assert_ne!(after_fake("RuboCop::FakeNs::Plugin").status.code(), Some(2));
        for (class, want) in [
            (
                "RuboCop::Cop::FakeNs::One",
                "undefined method 'supported?' for an instance of RuboCop::Cop::FakeNs::One\n",
            ),
            ("LintRoller::Rules", "unknown keywords: enabled, require_path, plugin_class_name\n"),
        ] {
            let out = after_fake(class);
            assert_eq!(text(&out.stderr), want);
            assert_eq!(out.status.code(), Some(2));
        }
    }
    let _ = fs::remove_dir_all(&dir);
    let _ = fs::remove_dir_all(&gems);
}

/// rbcop under RBCOP_FALLBACK=1 and RuboCop print the same stdout and stderr and exit alike.
fn same(dir: &Path, env: &[(&str, &str)], args: &[&str]) {
    same_with(dir, env, args, false);
}

/// `same` with RuboCop's result cache on too (both caches under HOME, `dir`).
fn same_cached(dir: &Path, env: &[(&str, &str)], args: &[&str]) {
    same_with(dir, env, args, true);
}

fn same_with(dir: &Path, env: &[(&str, &str)], args: &[&str], cache: bool) {
    let mut ours = common::command(env!("CARGO_BIN_EXE_rbcop"));
    ours.env("HOME", dir).env("RBCOP_FALLBACK", "1");
    let mut theirs = common::command("rubocop");
    theirs.env("HOME", dir);
    if !cache {
        theirs.args(["--cache", "false"]);
    }
    for (k, v) in env {
        ours.env(k, v);
        theirs.env(k, v);
    }
    let ours = ours.args(args).current_dir(dir).output().unwrap();
    let theirs = theirs.args(args).current_dir(dir).output().unwrap();
    // The JSON formatter's metadata names the Ruby RuboCop runs on; rbcop names the one its
    // recordings use (docs/known-gaps.md), so a RuboCop on another Ruby differs only there.
    let ruby = regex::Regex::new(r#""ruby_version":"[^"]*","ruby_patchlevel":"[^"]*""#).unwrap();
    let stdout = |o: &Output| ruby.replace_all(&text(&o.stdout), "RUBY").into_owned();
    assert_eq!(stdout(&ours), stdout(&theirs), "{env:?} {args:?}");
    assert_eq!(text(&ours.stderr), text(&theirs.stderr), "{env:?} {args:?}");
    assert_eq!(ours.status.code(), theirs.status.code(), "{env:?} {args:?}");
}

#[test]
fn fallback_forwards_the_command_line() {
    if !have_rubocop() {
        eprintln!("skipped: RuboCop 1.92.2 is not on PATH");
        return;
    }
    let dir = project("flags");
    fs::write(dir.join("c.rb"), "# frozen_string_literal: true\n\nfoo(1) # rubocop:disable Custom/NoFoo\n").unwrap();
    // A pending custom cop runs under `NewCops: enable`, of AllCops or of its department (#413).
    let dept_new_cops = "require:\n  - ./no_foo.rb\nAllCops:\n  SuggestExtensions: false\nCustom:\n  NewCops: enable\n";
    for config in [CONFIG, dept_new_cops] {
        fs::write(dir.join(".rubocop.yml"), format!("{config}Custom/NoFoo:\n  Enabled: pending\n")).unwrap();
        for flags in [&[][..], &["--disable-pending-cops"], &["--enable-pending-cops"]] {
            same(&dir, &[], &[&["-f", "e", "b.rb"], flags].concat());
        }
    }
    // Without NewCops the pending-cops banner is printed once.
    fs::write(dir.join(".rubocop.yml"), "require:\n  - ./no_foo.rb\nCustom/NoFoo:\n  Enabled: pending\n").unwrap();
    same(&dir, &[], &["-f", "e", "b.rb"]);
    // #415, and --editor-mode (`AutoCorrect: contextual` is not correctable under it).
    fs::write(dir.join(".rubocop.yml"), format!("{CONFIG}Custom/NoFoo:\n  AutoCorrect: contextual\n")).unwrap();
    for flag in ["--ignore-disable-comments", "--editor-mode"] {
        same(&dir, &[], &["-f", "e", flag, "b.rb", "c.rb"]);
    }
    // The child reads neither `.rubocop` nor RUBOCOP_OPTS a second time (#414).
    for opts in ["--format progress", "--format json", "--stderr", "-f s --stderr"] {
        fs::write(dir.join(".rubocop"), format!("{opts}\n")).unwrap();
        same(&dir, &[], &["a.rb"]);
        fs::remove_file(dir.join(".rubocop")).unwrap();
        same(&dir, &[("RUBOCOP_OPTS", opts)], &["a.rb"]);
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn fallback_prints_config_warnings_once() {
    if !have_rubocop() {
        eprintln!("skipped: RuboCop 1.92.2 is not on PATH");
        return;
    }
    // #416: a config warning, and the missing-department warnings for directive names (of a cop
    // rbcop implements, and of a custom one: the child warns about that one as its offense on the
    // line consults the directive).
    let dir = project("warnings");
    fs::write(dir.join(".rubocop.yml"), format!("{CONFIG}Style/StringLiterals:\n  Foo: 1\n")).unwrap();
    fs::write(dir.join("c.rb"), "# frozen_string_literal: true\n\nfoo(1) # rubocop:disable NoFoo, LineLength\n")
        .unwrap();
    // The custom cop's class is registered, so Migration/DepartmentName qualifies `NoFoo` too.
    let ours = rbcop(&dir, true, &["-f", "e", "a.rb", "c.rb"]);
    let theirs = rubocop(&dir, &["--cache", "false", "-f", "e", "a.rb", "c.rb"]);
    assert_eq!(text(&ours.stderr), text(&theirs.stderr));
    assert_eq!(text(&ours.stdout), text(&theirs.stdout));
    assert!(text(&theirs.stderr).contains("no department given for NoFoo"), "{}", text(&theirs.stderr));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn fallback_fail_fast_autocorrect_stops_correcting() {
    if !have_rubocop() {
        eprintln!("skipped: RuboCop 1.92.2 is not on PATH");
        return;
    }
    // -A -F: an uncorrectable offense in a.rb stops the run, so the fallback corrects no b.rb.
    let ours = project("ff-ours");
    let theirs = project("ff-theirs");
    for d in [&ours, &theirs] {
        fs::write(d.join("a.rb"), "# frozen_string_literal: true\n\neval('1')\n").unwrap();
    }
    let a = rbcop(&ours, true, &["-A", "-F", "-f", "e", "a.rb", "b.rb"]);
    let b = rubocop(&theirs, &["--cache", "false", "-A", "-F", "-f", "e", "a.rb", "b.rb"]);
    let rel = |s: String, d: &Path| s.replace(&d.display().to_string(), "DIR");
    assert_eq!(rel(text(&a.stdout), &ours), rel(text(&b.stdout), &theirs));
    assert_eq!(a.status.code(), b.status.code());
    for f in ["a.rb", "b.rb"] {
        assert_eq!(fs::read_to_string(ours.join(f)).unwrap(), fs::read_to_string(theirs.join(f)).unwrap(), "{f}");
    }
    let _ = fs::remove_dir_all(&ours);
    let _ = fs::remove_dir_all(&theirs);
}

/// RuboCop's registry holds a plugin's cop classes: one without a default.yml entry is a cop
/// (enabled), a default.yml entry without a class is not (an unknown name), and --show-cops lists
/// them with `# Supports --autocorrect` for the cops that extend AutoCorrector.
#[test]
fn plugin_cops_come_from_lib_classes() {
    let dir = project("lib-classes");
    let gems = dir.join("gems");
    let gem = gems.join("gems/rubocop-fake-0.1.0");
    fs::create_dir_all(gems.join("specifications")).unwrap();
    fs::create_dir_all(gem.join("config")).unwrap();
    fs::create_dir_all(gem.join("lib/rubocop/cop/fake")).unwrap();
    fs::write(gem.join("lib/rubocop-fake.rb"), "").unwrap();
    fs::write(
        gems.join("specifications/rubocop-fake-0.1.0.gemspec"),
        "Gem::Specification.new { |s| s.name = 'rubocop-fake'; s.version = '0.1.0'; \
         s.metadata = { 'default_lint_roller_plugin' => 'RuboCop::Fake::Plugin' } }\n",
    )
    .unwrap();
    fs::write(gem.join("config/default.yml"), "Fake/One:\n  Enabled: true\nFake/Phantom:\n  Enabled: true\n").unwrap();
    fs::write(
        gem.join("lib/rubocop/cop/fake/base.rb"),
        "module RuboCop # :nodoc:\n  module Cop\n    module Fake\n      class Base < ::RuboCop::Cop::Base\n        \
         exclude_from_registry\n        extend AutoCorrector\n      end\n    end\n  end\nend\n",
    )
    .unwrap();
    fs::write(
        gem.join("lib/rubocop/cop/fake/one.rb"),
        "module RuboCop\n  module Cop\n    module Fake\n      class One < Base # :nodoc:\n        def on_send(node)\n        \
         end\n      end\n\n      class Helper < Struct.new(:a); end\n    end\n  end\nend\n",
    )
    .unwrap();
    fs::write(
        gem.join("lib/rubocop/cop/fake/extra.rb"),
        // a qualified AutoCorrector counts too (#611)
        "module RuboCop::Cop::Fake\n  class Extra < RuboCop::Cop::Base\n    extend ::RuboCop::Cop::AutoCorrector\n  end\nend\n",
    )
    .unwrap();
    fs::write(dir.join(".rubocop.yml"), "plugins:\n  - rubocop-fake\nAllCops:\n  NewCops: enable\n").unwrap();
    fs::write(dir.join("a.rb"), "# frozen_string_literal: true\n\nputs 1 # rubocop:disable Fake/Phantom\n").unwrap();
    let run = |args: &[&str]| {
        let mut cmd = common::command(env!("CARGO_BIN_EXE_rbcop"));
        cmd.env("GEM_PATH", &gems).args(args).current_dir(&dir).output().unwrap()
    };
    let out = run(&["-f", "e", "a.rb"]);
    // Without a ruby on PATH the gem cannot be found.
    if !text(&out.stderr).contains("cannot run ruby") {
        assert!(
            text(&out.stderr).contains("not running 2 cops it does not implement (Fake: 2)"),
            "{}",
            text(&out.stderr)
        );
        assert!(text(&out.stdout).contains("Unknown cop name `Fake/Phantom`"), "{}", text(&out.stdout));
        let out = run(&["--show-cops", "Fake/*"]);
        assert_eq!(
            text(&out.stdout),
            "# Supports --autocorrect\nFake/Extra:\n  Enabled: true\n\n# Supports --autocorrect\nFake/One:\n  Enabled: true\n\n"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn fallback_output_matches_rubocop() {
    if !have_rubocop() {
        eprintln!("skipped: RuboCop 1.92.2 is not on PATH");
        return;
    }
    let dir = project("report");
    // ruby-progressbar skips a redraw less than 10 ms after the previous one, so whether fuubar
    // prints an intermediate `Progress: |===` before a file's offenses depends on how fast RuboCop
    // inspected the file before it; rbcop never throttles (src/formatter/text.rs). Those redraws are
    // compared as empty lines; the offenses, the line breaks and the final full bar stay exact.
    let unthrottled = |s: String| {
        s.split_inclusive('\n')
            .map(|l| {
                let bar = l.trim_end_matches('\n').strip_prefix("Progress: |");
                if bar.is_some_and(|b| b.bytes().all(|c| c == b'=')) { "\n" } else { l }
            })
            .collect::<String>()
    };
    for format in ["simple", "emacs", "clang", "progress", "fuubar"] {
        let ours = rbcop(&dir, true, &["-f", format]);
        let theirs = rubocop(&dir, &["--cache", "false", "-f", format]);
        let (ours_out, theirs_out) = if format == "fuubar" {
            (unthrottled(text(&ours.stdout)), unthrottled(text(&theirs.stdout)))
        } else {
            (text(&ours.stdout), text(&theirs.stdout))
        };
        assert_eq!(ours_out, theirs_out, "{format}");
        assert_eq!(ours.status.code(), theirs.status.code(), "{format}");
        assert!(ours.stderr.is_empty(), "{}", text(&ours.stderr));
    }
    let _ = fs::remove_dir_all(&dir);
}

/// A plugin rbcop does not implement that defines no cops (rubocop-erb): rbcop honors its
/// default.yml (its `AllCops: Include` brings in the .erb files, its Lint/Syntax Exclude applies),
/// runs no cops on the files it brings in, names it on stderr, and under RBCOP_FALLBACK=1 hands
/// those files whole to RuboCop (#484).
#[test]
fn copless_plugin_files_go_to_rubocop() {
    let dir = project("copless");
    // Outside the project, so its lib/ is not inspected.
    let gems = dir.with_extension("gems");
    let _ = fs::remove_dir_all(&gems);
    let gem = gems.join("gems/rubocop-fakeerb-0.1.0");
    fs::create_dir_all(gems.join("specifications")).unwrap();
    fs::create_dir_all(gem.join("config")).unwrap();
    fs::create_dir_all(gem.join("lib/rubocop/fakeerb")).unwrap();
    fs::write(
        gems.join("specifications/rubocop-fakeerb-0.1.0.gemspec"),
        "Gem::Specification.new { |s| s.name = 'rubocop-fakeerb'; s.version = '0.1.0'; \
         s.metadata = { 'default_lint_roller_plugin' => 'RuboCop::Fakeerb::Plugin' } }\n",
    )
    .unwrap();
    fs::write(
        gem.join("config/default.yml"),
        "inherit_mode:\n  merge:\n    - Exclude\n    - Include\n\nAllCops:\n  Include:\n    - '**/*.erb'\n\n\
         Lint/Syntax:\n  Exclude:\n    - '**/*.erb'\n    - 'skip/*.rb'\n",
    )
    .unwrap();
    fs::write(gem.join("lib/rubocop-fakeerb.rb"), "").unwrap();
    fs::write(
        gem.join("lib/rubocop/fakeerb/plugin.rb"),
        "module RuboCop\n  module Fakeerb\n    class Plugin\n    end\n  end\nend\n",
    )
    .unwrap();
    fs::write(dir.join(".rubocop.yml"), "plugins:\n  - rubocop-fakeerb\nAllCops:\n  NewCops: enable\n").unwrap();
    fs::remove_file(dir.join("a.rb")).unwrap();
    fs::remove_file(dir.join("no_foo.rb")).unwrap();
    fs::write(dir.join("b.rb"), "# frozen_string_literal: true\n\nx = 1\n").unwrap();
    fs::write(dir.join("v.erb"), "<p><%= foo(1,2) %></p>\nhello world\n").unwrap();
    fs::create_dir_all(dir.join("skip")).unwrap();
    fs::write(dir.join("skip/bad.rb"), "def (\n").unwrap();
    // A stand-in RuboCop: records how it was run and reports one offense on the last file.
    let child = dir.join("child.sh");
    fs::write(
        &child,
        "#!/bin/sh\necho \"$RBCOP_FALLBACK_SYNTAX $*\" >> \"$(dirname \"$0\")/child.log\"\n\
         while [ $# -gt 0 ]; do [ \"$1\" = --out ] && o=$2; f=$1; shift; done\n\
         printf '%s\\t3\\t4\\tconvention\\t0\\t0\\t0\\tFake/Cop\\tFrom RuboCop.\\t0\\t\\nrbcop-fallback-done\\n' \"$f\" > \"$o\"\n",
    )
    .unwrap();
    Command::new("chmod").arg("755").arg(&child).status().unwrap();
    let run = |fallback: bool| {
        let mut cmd = common::command(env!("CARGO_BIN_EXE_rbcop"));
        cmd.env("GEM_PATH", &gems).env("RBCOP_RUBOCOP", &child);
        if fallback {
            cmd.env("RBCOP_FALLBACK", "1");
        }
        cmd.args(["-f", "e"]).current_dir(&dir).output().unwrap()
    };
    let out = run(false);
    // Without a ruby on PATH the gem cannot be found.
    if !text(&out.stderr).contains("cannot run ruby") {
        assert_eq!(
            text(&out.stderr),
            "rbcop: not running the Ruby code of plugin rubocop-fakeerb; set RBCOP_FALLBACK=1 to run them with \
             RuboCop\n"
        );
        // b.rb's offenses only: v.erb runs no cops, skip/bad.rb's Lint/Syntax is excluded.
        let stdout = text(&out.stdout);
        assert!(stdout.contains("b.rb:3:1"), "{stdout}");
        assert!(!stdout.contains("v.erb") && !stdout.contains("bad.rb"), "{stdout}");
        let out = run(true);
        assert_eq!(text(&out.stderr), "");
        let stdout = text(&out.stdout);
        assert!(stdout.contains("v.erb:1:4: C: Fake/Cop: From RuboCop."), "{stdout}");
        // One child, for v.erb alone, with every cop (no --except) and Lint/Syntax kept.
        let log = fs::read_to_string(dir.join("child.log")).unwrap();
        assert_eq!(log.lines().count(), 1, "{log}");
        assert!(log.starts_with("1 ") && !log.contains("--except") && log.trim_end().ends_with("v.erb"), "{log}");
    }
    let _ = fs::remove_dir_all(&dir);
    let _ = fs::remove_dir_all(&gems);
}

#[test]
fn fallback_autocorrect_matches_rubocop() {
    if !have_rubocop() {
        eprintln!("skipped: RuboCop 1.92.2 is not on PATH");
        return;
    }
    let ours = project("ac-ours");
    let theirs = project("ac-theirs");
    let a = rbcop(&ours, true, &["-A", "-f", "e"]);
    let b = rubocop(&theirs, &["--cache", "false", "-A", "-f", "e"]);
    let rel = |s: String, d: &Path| s.replace(&d.display().to_string(), "DIR");
    assert_eq!(rel(text(&a.stdout), &ours), rel(text(&b.stdout), &theirs));
    for f in ["a.rb", "b.rb"] {
        assert_eq!(fs::read_to_string(ours.join(f)).unwrap(), fs::read_to_string(theirs.join(f)).unwrap(), "{f}");
    }
    let _ = fs::remove_dir_all(&ours);
    let _ = fs::remove_dir_all(&theirs);
}

const MISSING: &str = "rbcop: RuboCop runs them when one is found: add the rubocop gem to the bundle, put rubocop \
                       on PATH, or set RBCOP_RUBOCOP\n";

/// #287: without RBCOP_FALLBACK the fallback runs whenever a RuboCop is found (`bundle exec rubocop`
/// when the bundle locks the gem, else `rubocop` on PATH), and `-r`, `--plugin` and custom formatter
/// classes hand the whole run to it. With none found, or under RBCOP_FALLBACK=0, rbcop skips or
/// refuses as before.
#[test]
fn fallback_is_the_default() {
    let dir = project("default");
    // Each run starts RuboCop: none reuses the child's cached offenses (#805).
    fs::write(dir.join(".rubocop.yml"), format!("{CONFIG}  UseCache: false\n")).unwrap();
    let bin = dir.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let run = |env: &[(&str, &str)], args: &[&str]| {
        let mut cmd = common::command(env!("CARGO_BIN_EXE_rbcop"));
        cmd.env_remove("RBCOP_FALLBACK").env("HOME", &dir).env("PATH", &bin);
        for (k, v) in env {
            cmd.env(k, v);
        }
        cmd.args(args).current_dir(&dir).output().unwrap()
    };
    let notice = "rbcop: not running cops from Ruby code the config requires; set RBCOP_FALLBACK=1 to run them \
                  with RuboCop\n";
    // No RuboCop: nothing on PATH, no Gemfile.lock.
    let out = run(&[], &["-f", "e"]);
    assert_eq!(text(&out.stderr), format!("{notice}{MISSING}"));
    assert_eq!(text(&run(&[("RBCOP_FALLBACK", "0")], &["-f", "e"]).stderr), notice);
    for (args, refusal) in [
        (&["-r", "./no_foo.rb", "-f", "e"][..], "--require loads Ruby code, which rbcop cannot run"),
        (&["--plugin", "rubocop-sorbet"], "--plugin loads Ruby code, which rbcop cannot run"),
        (&["-f", "MyFormatter"], "custom formatter classes are not supported by rbcop: MyFormatter"),
    ] {
        let out = run(&[], args);
        assert_eq!(text(&out.stderr), format!("rbcop: {refusal}\n{MISSING}"), "{args:?}");
        assert_eq!(out.status.code(), Some(2));
        let out = run(&[("RBCOP_FALLBACK", "0")], args);
        assert_eq!(text(&out.stderr), format!("rbcop: {refusal}\n"), "{args:?}");
    }
    // A bundle that locks rubocop, and a stand-in `bundle` that records how it was run.
    fs::write(dir.join("Gemfile.lock"), "GEM\n  specs:\n    rubocop (1.92.2)\n").unwrap();
    // What a cop prints itself passes through to stdout.
    fs::write(
        bin.join("bundle"),
        "#!/bin/sh\necho \"$*\" >> \"${0%/*}/log\"\necho from-a-cop\no=/dev/null\n\
         while [ $# -gt 0 ]; do [ \"$1\" = --out ] && o=$2; shift; done\necho rbcop-fallback-done > \"$o\"\n",
    )
    .unwrap();
    Command::new("chmod").arg("755").arg(bin.join("bundle")).status().unwrap();
    // Each run names the RuboCop it starts on stderr (stdout stays RuboCop's).
    let running =
        |what: &str| format!("rbcop: running bundle exec rubocop for {what}; set RBCOP_FALLBACK=0 to skip RuboCop\n");
    let out = run(&[], &["-f", "e"]);
    assert_eq!(text(&out.stderr), running("cops from Ruby code the config requires"));
    assert!(text(&out.stdout).starts_with("from-a-cop\n"), "{}", text(&out.stdout));
    let out = run(&[], &["-r", "./no_foo.rb", "-f", "e", "a.rb"]);
    assert_eq!(
        (text(&out.stderr), out.status.code()),
        (running("the whole run (--require loads Ruby code, which rbcop cannot run)"), Some(0))
    );
    let log = fs::read_to_string(bin.join("log")).unwrap();
    let lines: Vec<&str> = log.lines().collect();
    assert!(lines[0].starts_with("exec rubocop --cache false --require ") && lines[0].contains("--except"), "{log}");
    assert_eq!(lines[1..], ["exec rubocop -r ./no_foo.rb -f e a.rb"], "{log}");
    // An editor or server context (the server options, --lsp, --mcp) runs no RuboCop on its own.
    let out = run(&[], &["--server", "-f", "e"]);
    assert_eq!(text(&out.stderr), notice);
    let refused = "rbcop: --require loads Ruby code, which rbcop cannot run\n";
    // The language server lints the project natively and never starts RuboCop.
    let mut lsp = Vec::new();
    for m in [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"processId":null,"capabilities":{}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#.to_string(),
        format!(
            r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"file://{}/a.rb","languageId":"ruby","version":1,"text":"foo(1)\n"}}}}}}"#,
            dir.display()
        ),
        r#"{"jsonrpc":"2.0","id":2,"method":"shutdown"}"#.to_string(),
        r#"{"jsonrpc":"2.0","method":"exit"}"#.to_string(),
    ] {
        lsp.extend(format!("Content-Length: {}\r\n\r\n{m}", m.len()).into_bytes());
    }
    let mut child = common::command(env!("CARGO_BIN_EXE_rbcop"))
        .env_remove("RBCOP_FALLBACK")
        .env("HOME", &dir)
        .env("PATH", &bin)
        .arg("--lsp")
        .current_dir(&dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let _ = std::io::Write::write_all(&mut child.stdin.take().unwrap(), &lsp);
    let out = child.wait_with_output().unwrap();
    assert!(text(&out.stdout).contains("publishDiagnostics"), "{}", text(&out.stdout));
    assert!(!text(&out.stderr).contains("rbcop: running"), "{}", text(&out.stderr));
    // So does the MCP server, for both tools.
    let logged = fs::read_to_string(bin.join("log")).unwrap();
    let mcp = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"rubocop_inspection\",\
               \"arguments\":{\"path\":\"a.rb\"}}}\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":\
               {\"name\":\"rubocop_autocorrection\",\"arguments\":{\"source_code\":\"foo(1)\\n\",\"safety\":true}}}\n";
    let mut child = common::command(env!("CARGO_BIN_EXE_rbcop"))
        .env_remove("RBCOP_FALLBACK")
        .env("HOME", &dir)
        .env("PATH", &bin)
        .arg("--mcp")
        .current_dir(&dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let _ = std::io::Write::write_all(&mut child.stdin.take().unwrap(), mcp.as_bytes());
    let out = child.wait_with_output().unwrap();
    assert_eq!(text(&out.stdout).matches("\"isError\":false").count(), 2, "{}", text(&out.stdout));
    assert!(!text(&out.stderr).contains("rbcop: running"), "{}", text(&out.stderr));
    assert_eq!(fs::read_to_string(bin.join("log")).unwrap(), logged);
    let out = run(&[], &["--mcp", "-r", "./no_foo.rb"]);
    assert_eq!((text(&out.stderr), out.status.code()), (refused.to_string(), Some(2)));
    // Options parse to the end, abbreviated too: `-r` with `--ls` (--lsp) is not handed to RuboCop.
    for args in [&["-r", "./no_foo.rb", "--ls"][..], &["--lsp", "-r", "./no_foo.rb"]] {
        let out = run(&[], args);
        assert_eq!((text(&out.stderr), out.status.code()), (refused.to_string(), Some(2)), "{args:?}");
    }
    for (env, args) in [
        (&[("RUBOCOP_OPTS", "--lsp")][..], &["-r", "./no_foo.rb"][..]),
        (&[], &["--server", "-r", "./no_foo.rb", "-f", "e"]),
        // Abbreviated, and from RUBOCOP_OPTS.
        (&[], &["-r", "./no_foo.rb", "--editor-m", "-f", "e"]),
        (&[("RUBOCOP_OPTS", "--server")], &["-r", "./no_foo.rb", "-f", "e"]),
        (&[("RUBOCOP_OPTS", "--editor-mode")], &["-r", "./no_foo.rb", "-f", "e"]),
        (&[], &["-r", "./no_foo.rb", "--stdin", "a.rb"]),
    ] {
        let out = run(env, args);
        assert_eq!((text(&out.stderr), out.status.code()), (refused.to_string(), Some(2)), "{env:?} {args:?}");
    }
    let out = run(&[("RUBOCOP_OPTS", "--editor-mode")], &["-f", "e"]);
    assert_eq!(text(&out.stderr), notice);
    assert_eq!(fs::read_to_string(bin.join("log")).unwrap(), log);
    // RBCOP_FALLBACK=1 runs it there too, and asked for, is not announced.
    let out = run(&[("RBCOP_FALLBACK", "1")], &["--server", "-f", "e"]);
    assert_eq!(text(&out.stderr), "");
    assert_eq!(fs::read_to_string(bin.join("log")).unwrap().lines().count(), 3);
    // A RuboCop that exits 1 without running (an rbenv shim missing the project's Ruby) is an error.
    fs::write(dir.join("bin/bundle"), "#!/bin/sh\nexit 1\n").unwrap();
    let out = run(&[], &["-f", "e"]);
    assert!(text(&out.stderr).contains("set RBCOP_FALLBACK=0 to skip"), "{}", text(&out.stderr));
    assert_eq!(out.status.code(), Some(2));
    let _ = fs::remove_dir_all(&dir);
}

/// #287 with a live RuboCop on PATH: the default run, `-r` of a custom cop and a custom formatter
/// class print what RuboCop prints.
#[test]
fn default_fallback_matches_rubocop() {
    if !have_rubocop() {
        eprintln!("skipped: RuboCop 1.92.2 is not on PATH");
        return;
    }
    let dir = project("default-live");
    fs::write(
        dir.join("my_formatter.rb"),
        "class MyFormatter < RuboCop::Formatter::BaseFormatter\n  def file_finished(file, offenses)\n    \
         offenses.each { |o| output.puts \"#{File.basename(file)} #{o.cop_name}\" }\n  end\nend\n",
    )
    .unwrap();
    let check = |args: &[&str]| {
        let mut ours = common::command(env!("CARGO_BIN_EXE_rbcop"));
        ours.env_remove("RBCOP_FALLBACK").env("HOME", &dir);
        let ours = ours.args(args).current_dir(&dir).output().unwrap();
        let theirs = rubocop(&dir, args);
        assert_eq!(text(&ours.stdout), text(&theirs.stdout), "{args:?}");
        // rbcop's own first line names the RuboCop it runs.
        let stderr = text(&ours.stderr);
        let (first, rest) = stderr.split_once('\n').unwrap();
        assert!(first.starts_with("rbcop: running rubocop for "), "{stderr}");
        assert_eq!(rest, text(&theirs.stderr), "{args:?}");
        assert_eq!(ours.status.code(), theirs.status.code(), "{args:?}");
        assert!(!theirs.stdout.is_empty());
    };
    check(&["--cache", "false", "-f", "e"]);
    fs::write(dir.join(".rubocop.yml"), "AllCops:\n  NewCops: enable\n  SuggestExtensions: false\n").unwrap();
    check(&["--cache", "false", "-r", "./no_foo.rb", "-f", "e"]);
    check(&["--cache", "false", "-r", "./no_foo.rb", "-r", "./my_formatter.rb", "-f", "MyFormatter"]);
    let _ = fs::remove_dir_all(&dir);
}

/// What a `require:`d file prints while it loads reaches stderr as from RuboCop: Ruby's warnings
/// when it redefines a cop's constant, though the child hides its config output (#804).
#[test]
fn required_code_warnings_reach_stderr() {
    if !have_rubocop() {
        eprintln!("skipped: RuboCop 1.92.2 is not on PATH");
        return;
    }
    let dir = project("warn");
    let config = "require:\n  - ./msg_cop.rb\nAllCops:\n  DisabledByDefault: true\n  NewCops: disable\n  \
                  SuggestExtensions: false\n  TargetRubyVersion: 3.4\nStyle/OneClassPerFile:\n  Enabled: true\n";
    fs::write(dir.join(".rubocop.yml"), config).unwrap();
    fs::write(dir.join("a.rb"), "# frozen_string_literal: true\n\nclass A; end\nclass B; end\n").unwrap();
    let cop = "class RuboCop::Cop::Style::OneClassPerFile < RuboCop::Cop::Base\n  MSG = 'Changed.'\nend\n";
    fs::write(dir.join("msg_cop.rb"), cop).unwrap();
    same(&dir, &[], &["-f", "e", "a.rb"]);
    let _ = fs::remove_dir_all(&dir);
}

/// A `require:`d local file that defines or reopens the class of a cop rbcop implements changes
/// that cop in RuboCop: rbcop leaves it to RuboCop as required code, and under RBCOP_FALLBACK=0
/// neither runs its own port nor judges a directive naming it redundant (#801).
#[test]
fn required_code_reopening_a_builtin_cop_goes_to_rubocop() {
    let dir = project("reopen");
    let config = "require:\n  - ./quiet_cop.rb\nAllCops:\n  DisabledByDefault: true\n  NewCops: disable\n  \
                  SuggestExtensions: false\n  TargetRubyVersion: 3.4\nStyle/OneClassPerFile:\n  Enabled: true\n\
                  Lint/RedundantCopDisableDirective:\n  Enabled: true\n";
    fs::write(dir.join(".rubocop.yml"), config).unwrap();
    fs::write(dir.join("a.rb"), "# frozen_string_literal: true\n\nclass A; end\nclass B; end\n").unwrap();
    fs::write(
        dir.join("b.rb"),
        "# frozen_string_literal: true\n\nclass C; end\nclass D; end # rubocop:disable Style/OneClassPerFile\n",
    )
    .unwrap();
    let patch = "  private\n\n  def allowed_class?(_node) = true\nend\n";
    let forms = [
        // Compact, as a new cop would be declared.
        format!("class RuboCop::Cop::Style::OneClassPerFile < RuboCop::Cop::Base\n{patch}"),
        // Nested modules.
        "module RuboCop\n  module Cop\n    module Style\n      class OneClassPerFile < Base\n        private\n\n        \
         def allowed_class?(_node) = true\n      end\n    end\n  end\nend\n"
            .to_string(),
        // Reopened without a superclass.
        format!("class RuboCop::Cop::Style::OneClassPerFile\n{patch}"),
    ];
    for form in &forms {
        fs::write(dir.join("quiet_cop.rb"), form).unwrap();
        let mut cmd = common::command(env!("CARGO_BIN_EXE_rbcop"));
        let out =
            cmd.env("HOME", &dir).env("RBCOP_FALLBACK", "0").args(["-f", "e"]).current_dir(&dir).output().unwrap();
        assert_eq!(text(&out.stdout), "", "{form}");
        assert_eq!(
            text(&out.stderr),
            "rbcop: not running cops from Ruby code the config requires; set RBCOP_FALLBACK=1 to run them with \
             RuboCop\n",
            "{form}"
        );
        assert_eq!(out.status.code(), Some(0), "{form}");
        // RuboCop judges b.rb's directive redundant: the deliberate difference above.
        if have_rubocop() {
            same(&dir, &[], &["-f", "e", "a.rb"]);
        }
    }
    let _ = fs::remove_dir_all(&dir);
}

const NO_BAR: &str = "module CustomCops\n  class NoBar < RuboCop::Cop::Base\n    def on_send(node)\n      \
                      add_offense(node, message: 'bar') if node.method?(:bar)\n    end\n  end\nend\n";
const BOTH: &str = "require:\n  - ./no_foo.rb\n  - ./no_bar.rb\nAllCops:\n  DisabledByDefault: true\n  \
                    SuggestExtensions: false\n";

/// The RuboCop child parses only the files one of its cops may inspect (enabled, Include/Exclude,
/// or opted in by a directive), which is most of its time saved (#805); the output is RuboCop's
/// either way. A cop class outside RuboCop::Cop is named as Badge.for names it (`CustomCops/NoBar`).
#[test]
fn fallback_file_selection_matches_rubocop() {
    if !have_rubocop() {
        eprintln!("skipped: RuboCop 1.92.2 is not on PATH");
        return;
    }
    let dir = project("files");
    fs::write(dir.join("c.rb"), "# rubocop:enable Custom/NoFoo\nfoo\n").unwrap();
    for config in [
        "",
        "Custom/NoFoo:\n  Include:\n    - b.rb\n",
        "Custom:\n  Exclude:\n    - b.rb\n",
        "Custom/NoFoo:\n  Enabled: false\n",
    ] {
        fs::write(dir.join(".rubocop.yml"), format!("{CONFIG}{config}")).unwrap();
        same(&dir, &[], &["-f", "e", "a.rb", "b.rb", "c.rb"]);
        same_cached(&dir, &[], &["-f", "e", "a.rb", "b.rb", "c.rb"]);
        same_cached(&dir, &[], &["-f", "e", "a.rb", "b.rb", "c.rb"]);
    }
    fs::write(dir.join("no_bar.rb"), NO_BAR).unwrap();
    fs::write(dir.join("c.rb"), "# rubocop:enable CustomCops/NoBar\nbar\n").unwrap();
    fs::write(dir.join(".rubocop.yml"), BOTH).unwrap();
    for args in [&["-f", "e", "a.rb", "c.rb"][..], &["-f", "e", "--only", "CustomCops/NoBar", "a.rb", "c.rb"]] {
        same(&dir, &[], args);
    }
    let _ = fs::remove_dir_all(&dir);
}

/// The child's offenses are kept in the result cache (#805): a warm run starts RuboCop (it loads
/// the required code every run) but it skips the files rbcop has offenses for; a changed file, a
/// changed loaded Ruby file, or a file a cop raised an error or a warning on is inspected again.
#[test]
fn fallback_results_are_cached() {
    let dir = project("cached");
    let rc = dir.join("rc");
    // A stand-in RuboCop that takes rbcop's handshake (no_foo.rb is what it loaded), logs the
    // files it inspects, reports one offense in a.rb, marks b.rb uncached while `warn` exists, and
    // writes to stderr while `noisy` exists.
    fs::write(
        &rc,
        "#!/bin/sh\no=/dev/null\nwhile [ $# -gt 0 ]; do case $1 in --out) o=$2; shift;; --) shift; break;; esac; \
         shift; done\nd=${0%/*}\nskip=\nif [ -n \"$RBCOP_FALLBACK_HANDSHAKE\" ]; then\n  \
         printf '%s\\n' \"$d/no_foo.rb\" > \"$RBCOP_FALLBACK_HANDSHAKE/loaded\"\n  \
         printf '\\036rbcop-fallback-loaded\\n' >&2\n  skip=$(cat <&3)\nfi\n: > \"$o\"\nfor f; do\n  \
         case \"\n$skip\n\" in *\"\n$f\n\"*) continue;; esac\n  printf '%s ' \"${f##*/}\" >> \"$d/log\"\n  \
         case $f in */a.rb) printf '%s\\t2\\t5\\tconvention\\t0\\t0\\t0\\tCustom/NoFoo\\tAvoid.\\t0\\t\\n' \"$f\" >> \
         \"$o\";; */b.rb) [ -e \"$d/warn\" ] && printf 'rbcop-fallback-uncached\\t%s\\n' \"$f\" >> \"$o\";; esac\n\
         done\necho >> \"$d/log\"\n[ -e \"$d/noisy\" ] && echo 'warning: loud' >&2\necho rbcop-fallback-done >> \"$o\"\n",
    )
    .unwrap();
    Command::new("chmod").arg("755").arg(&rc).status().unwrap();
    let run = || {
        let _ = fs::remove_file(dir.join("log"));
        let mut cmd = common::command(env!("CARGO_BIN_EXE_rbcop"));
        let out = cmd
            .env("HOME", &dir)
            .env("RBCOP_FALLBACK", "1")
            .env("RBCOP_RUBOCOP", &rc)
            .env("RUBOCOP_CACHE_ROOT", dir.join("cache"))
            .args(["-f", "e", "a.rb", "b.rb"])
            .current_dir(&dir)
            .output()
            .unwrap();
        assert!(text(&out.stdout).contains("a.rb:1:3: C: Custom/NoFoo: Avoid."), "{}", text(&out.stdout));
        fs::write(dir.join("stderr"), &out.stderr).unwrap();
        fs::read_to_string(dir.join("log")).unwrap_or_default()
    };
    assert_eq!(run(), "a.rb b.rb \n");
    assert_eq!(run(), "\n");
    fs::write(dir.join("b.rb"), "# frozen_string_literal: true\n\nfoo(4)\n").unwrap();
    assert_eq!(run(), "b.rb \n");
    assert_eq!(run(), "\n");
    // A loaded file's contents, also edited to the same size.
    fs::write(dir.join("no_foo.rb"), COP.replace("foo", "fox")).unwrap();
    assert_eq!(run(), "a.rb b.rb \n");
    assert_eq!(run(), "\n");
    fs::write(dir.join("warn"), "").unwrap();
    fs::write(dir.join("b.rb"), "# frozen_string_literal: true\n\nfoo(5)\n").unwrap();
    assert_eq!(run(), "b.rb \n");
    assert_eq!(run(), "b.rb \n");
    fs::remove_file(dir.join("warn")).unwrap();
    // What it writes to stderr (but the handshake's line) comes on every run, cached or not.
    fs::write(dir.join("noisy"), "").unwrap();
    for log in ["b.rb \n", "\n"] {
        assert_eq!(run(), log);
        assert_eq!(fs::read_to_string(dir.join("stderr")).unwrap(), "warning: loud\n");
    }
    let _ = fs::remove_dir_all(&dir);
}

const FOO_COP: &str = "  def on_send(node)\n    add_offense(node, message: 'foo') if node.method?(:foo)\n  end\nend\n";
const REQUIRE_ONLY: &str = "AllCops:\n  NewCops: disable\n  SuggestExtensions: false\n";

/// #805 review F1: a cop class rbcop cannot read in the required code (top level, `Class.new`)
/// keeps every file, though another cop class there is read and restricts its own.
#[test]
fn unread_cop_classes_get_every_file() {
    if !have_rubocop() {
        eprintln!("skipped: RuboCop 1.92.2 is not on PATH");
        return;
    }
    let dir = project("unread");
    fs::write(
        dir.join(".rubocop.yml"),
        format!("require:\n  - ./cops.rb\n{REQUIRE_ONLY}CustomCops/NoBar:\n  Include:\n    - b.rb\n"),
    )
    .unwrap();
    fs::write(dir.join("a.rb"), "# frozen_string_literal: true\n\nfoo\n").unwrap();
    fs::write(dir.join("b.rb"), "# frozen_string_literal: true\n\nbar\nfoo\n").unwrap();
    for unread in [
        format!("class NoFoo < RuboCop::Cop::Base\n{FOO_COP}"),
        format!("module Other\nend\nOther::NoFoo = Class.new(RuboCop::Cop::Base) do\n{FOO_COP}"),
    ] {
        fs::write(dir.join("cops.rb"), format!("{NO_BAR}\n{unread}")).unwrap();
        same(&dir, &[], &["-f", "e", "a.rb", "b.rb"]);
        same_cached(&dir, &[], &["-f", "e", "a.rb", "b.rb"]);
        same_cached(&dir, &[], &["-f", "e", "a.rb", "b.rb"]);
    }
    let _ = fs::remove_dir_all(&dir);
}

/// #805 review F2: a file the required code loads through $LOAD_PATH is part of what a warm run's
/// cached offenses depend on.
#[test]
fn cached_fallback_offenses_follow_loaded_files() {
    if !have_rubocop() {
        eprintln!("skipped: RuboCop 1.92.2 is not on PATH");
        return;
    }
    let dir = project("loaded");
    fs::create_dir_all(dir.join("tools/rubocop/lib/acme")).unwrap();
    fs::write(dir.join(".rubocop.yml"), format!("require:\n  - ./tools/rubocop/loader.rb\n{REQUIRE_ONLY}")).unwrap();
    fs::write(
        dir.join("tools/rubocop/loader.rb"),
        "$LOAD_PATH.unshift(File.expand_path('lib', __dir__))\nrequire 'acme/messages'\nmodule RuboCop\n  \
         module Cop\n    module Custom\n      class NoFoo < Base\n        def on_send(node)\n          \
         add_offense(node, message: Acme::MSG) if node.method?(:foo)\n        end\n      end\n    end\n  end\nend\n",
    )
    .unwrap();
    let messages = dir.join("tools/rubocop/lib/acme/messages.rb");
    fs::write(&messages, "module Acme\n  MSG = 'old message'\nend\n").unwrap();
    fs::write(dir.join("a.rb"), "# frozen_string_literal: true\n\nfoo\n").unwrap();
    for _ in 0..2 {
        same_cached(&dir, &[], &["-f", "e", "a.rb"]);
    }
    fs::write(&messages, "module Acme\n  MSG = 'new message'\nend\n").unwrap();
    same_cached(&dir, &[], &["-f", "e", "a.rb"]);
    let _ = fs::remove_dir_all(&dir);
}

/// #805 review F3: a custom cop's external dependency (Base#external_dependency_checksum) is
/// something rbcop cannot key on, so its offenses are not cached.
#[test]
fn external_dependencies_are_not_cached() {
    if !have_rubocop() {
        eprintln!("skipped: RuboCop 1.92.2 is not on PATH");
        return;
    }
    let dir = project("external");
    fs::write(dir.join(".rubocop.yml"), format!("require:\n  - ./cops.rb\n{REQUIRE_ONLY}")).unwrap();
    fs::write(
        dir.join("cops.rb"),
        "require 'digest'\nmodule RuboCop\n  module Cop\n    module Custom\n      class Banned < Base\n        \
         def external_dependency_checksum\n          Digest::SHA1.hexdigest(File.read('banned.txt'))\n        \
         end\n\n        def on_send(node)\n          add_offense(node, message: 'banned') if \
         File.read('banned.txt').split.include?(node.method_name.to_s)\n        end\n      end\n    end\n  \
         end\nend\n",
    )
    .unwrap();
    fs::write(dir.join("banned.txt"), "foo\n").unwrap();
    fs::write(dir.join("a.rb"), "# frozen_string_literal: true\n\nfoo\nbar\n").unwrap();
    same_cached(&dir, &[], &["-f", "e", "a.rb"]);
    fs::write(dir.join("banned.txt"), "bar\n").unwrap();
    same_cached(&dir, &[], &["-f", "e", "a.rb"]);
    let _ = fs::remove_dir_all(&dir);
}

/// #805 review F4: what the required code prints on stdout while it loads comes on every run,
/// warm too.
#[test]
fn required_code_output_comes_every_run() {
    if !have_rubocop() {
        eprintln!("skipped: RuboCop 1.92.2 is not on PATH");
        return;
    }
    let dir = project("loud");
    let cop = format!(
        "module RuboCop\n  module Cop\n    module Custom\n      class NoFoo < Base\n{FOO_COP}    end\n  end\nend\n"
    );
    fs::write(dir.join("cops.rb"), format!("puts 'acme cops loaded'\n{cop}")).unwrap();
    fs::write(dir.join(".rubocop.yml"), format!("require:\n  - ./cops.rb\n{REQUIRE_ONLY}")).unwrap();
    fs::write(dir.join("a.rb"), "# frozen_string_literal: true\n\nfoo\n").unwrap();
    fs::write(dir.join("b.rb"), "# frozen_string_literal: true\n\nfoo\n").unwrap();
    for _ in 0..2 {
        same_cached(&dir, &[], &["-f", "e", "a.rb"]);
    }
    let _ = fs::remove_dir_all(&dir);
}

/// #805 review F5: what the required code prints on stderr while it loads, depending on anything
/// (an environment variable here), comes on a cached run with no file for its cops (#804).
#[test]
fn required_code_stderr_comes_without_files() {
    if !have_rubocop() {
        eprintln!("skipped: RuboCop 1.92.2 is not on PATH");
        return;
    }
    let dir = project("stderr");
    let cop = format!(
        "module RuboCop\n  module Cop\n    module Custom\n      class NoFoo < Base\n{FOO_COP}    end\n  end\nend\n"
    );
    fs::write(dir.join("b.rb"), "# frozen_string_literal: true\n\nfoo\n").unwrap();
    fs::write(dir.join("cops.rb"), &cop).unwrap();
    fs::write(dir.join("consts.rb"), "warn 'acme cops: STRICT mode is deprecated' if ENV['ACME_STRICT']\n").unwrap();
    fs::write(
        dir.join(".rubocop.yml"),
        format!("require:\n  - ./consts.rb\n  - ./cops.rb\n{REQUIRE_ONLY}Custom/NoFoo:\n  Include:\n    - a.rb\n"),
    )
    .unwrap();
    same_cached(&dir, &[], &["-f", "e", "b.rb"]);
    same_cached(&dir, &[("ACME_STRICT", "1")], &["-f", "e", "b.rb"]);
    let _ = fs::remove_dir_all(&dir);
}

/// #805 review N1: required code's load-time output comes once, though rbcop cannot read one of
/// the cop classes.
#[test]
fn child_output_comes_once() {
    if !have_rubocop() {
        eprintln!("skipped: RuboCop 1.92.2 is not on PATH");
        return;
    }
    let dir = project("once");
    fs::write(
        dir.join(".rubocop.yml"),
        format!("require:\n  - ./cops.rb\n{REQUIRE_ONLY}CustomCops/NoBar:\n  Include:\n    - b.rb\n"),
    )
    .unwrap();
    fs::write(dir.join("a.rb"), "# frozen_string_literal: true\n\nfoo\n").unwrap();
    fs::write(dir.join("b.rb"), "# frozen_string_literal: true\n\nbar\nfoo\n").unwrap();
    for unread in [
        format!("class NoFoo < RuboCop::Cop::Base\n{FOO_COP}"),
        format!("module Other\nend\nOther::NoFoo = Class.new(RuboCop::Cop::Base) do\n{FOO_COP}"),
    ] {
        fs::write(dir.join("cops.rb"), format!("puts 'acme banner'\nwarn 'acme warning'\n{NO_BAR}\n{unread}")).unwrap();
        same(&dir, &[], &["-f", "e", "a.rb", "b.rb"]);
        same_cached(&dir, &[], &["-f", "e", "a.rb", "b.rb"]);
    }
    let _ = fs::remove_dir_all(&dir);
}

/// #805 review N2: under -a the corrections of every fallback cop are reported, though rbcop
/// cannot read one of the cop classes.
#[test]
fn fallback_corrections_are_reported() {
    if !have_rubocop() {
        eprintln!("skipped: RuboCop 1.92.2 is not on PATH");
        return;
    }
    let cops = "module CustomCops\n  class NoBar < RuboCop::Cop::Base\n    extend RuboCop::Cop::AutoCorrector\n    \
                def on_send(node)\n      return unless node.method?(:bar)\n\n      add_offense(node, message: 'bar') \
                { |c| c.replace(node, 'baz') }\n    end\n  end\nend\n\nclass NoFoo < RuboCop::Cop::Base\n  extend \
                RuboCop::Cop::AutoCorrector\n  def on_send(node)\n    return unless node.method?(:foo)\n\n    \
                add_offense(node, message: 'foo') { |c| c.replace(node, 'qux') }\n  end\nend\n";
    let setup = |name: &str| {
        let dir = project(name);
        fs::write(
            dir.join(".rubocop.yml"),
            format!("require:\n  - ./cops.rb\n{REQUIRE_ONLY}CustomCops/NoBar:\n  Include:\n    - b.rb\n"),
        )
        .unwrap();
        fs::write(dir.join("cops.rb"), cops).unwrap();
        fs::write(dir.join("a.rb"), "# frozen_string_literal: true\n\nfoo\n").unwrap();
        fs::write(dir.join("b.rb"), "# frozen_string_literal: true\n\nbar\nfoo\n").unwrap();
        dir
    };
    let (ours, theirs) = (setup("correct-ours"), setup("correct-theirs"));
    let args = ["-f", "s", "-a", "a.rb", "b.rb"];
    let o = common::command(env!("CARGO_BIN_EXE_rbcop"))
        .env("HOME", &ours)
        .env("RBCOP_FALLBACK", "1")
        .args(args)
        .current_dir(&ours)
        .output()
        .unwrap();
    let t = common::command("rubocop").env("HOME", &theirs).args(args).current_dir(&theirs).output().unwrap();
    assert_eq!(text(&o.stdout), text(&t.stdout));
    assert_eq!(o.status.code(), t.status.code());
    for f in ["a.rb", "b.rb"] {
        assert_eq!(fs::read_to_string(ours.join(f)).unwrap(), fs::read_to_string(theirs.join(f)).unwrap(), "{f}");
    }
    let _ = fs::remove_dir_all(&ours);
    let _ = fs::remove_dir_all(&theirs);
}

/// #805 review N3: a loaded Ruby file edited to the same size with its modification time kept is
/// seen by the cache (its contents are hashed, as ResultCache.source_checksum does).
#[test]
fn cached_fallback_offenses_follow_loaded_file_contents() {
    if !have_rubocop() {
        eprintln!("skipped: RuboCop 1.92.2 is not on PATH");
        return;
    }
    let dir = project("contents");
    fs::create_dir_all(dir.join("tools/rubocop/lib/acme")).unwrap();
    fs::write(dir.join(".rubocop.yml"), format!("require:\n  - ./tools/rubocop/loader.rb\n{REQUIRE_ONLY}")).unwrap();
    fs::write(
        dir.join("tools/rubocop/loader.rb"),
        "$LOAD_PATH.unshift(File.expand_path('lib', __dir__))\nrequire 'acme/messages'\nmodule RuboCop\n  \
         module Cop\n    module Custom\n      class NoFoo < Base\n        def on_send(node)\n          \
         add_offense(node, message: Acme::MSG) if node.method?(:foo)\n        end\n      end\n    end\n  end\nend\n",
    )
    .unwrap();
    let messages = dir.join("tools/rubocop/lib/acme/messages.rb");
    fs::write(&messages, "module Acme\n  MSG = 'old message'\nend\n").unwrap();
    fs::write(dir.join("a.rb"), "# frozen_string_literal: true\n\nfoo\n").unwrap();
    fs::write(dir.join("b.rb"), "# frozen_string_literal: true\n\nfoo\n").unwrap();
    for _ in 0..2 {
        same_cached(&dir, &[], &["-f", "e", "a.rb", "b.rb"]);
    }
    let mtime = fs::metadata(&messages).unwrap().modified().unwrap();
    fs::write(&messages, "module Acme\n  MSG = 'new message'\nend\n").unwrap();
    fs::File::options().write(true).open(&messages).unwrap().set_modified(mtime).unwrap();
    same_cached(&dir, &[], &["-f", "e", "a.rb", "b.rb"]);
    let _ = fs::remove_dir_all(&dir);
}

/// #805 review N4: a warm run with every file cached reports every file's cached offenses, as
/// RuboCop's does (even when something neither cache sees has changed).
#[test]
fn warm_runs_use_every_cached_file() {
    if !have_rubocop() {
        eprintln!("skipped: RuboCop 1.92.2 is not on PATH");
        return;
    }
    let dir = project("allcached");
    fs::write(dir.join(".rubocop.yml"), format!("require:\n  - ./cops.rb\n{REQUIRE_ONLY}")).unwrap();
    fs::write(
        dir.join("cops.rb"),
        "module RuboCop\n  module Cop\n    module Custom\n      class Banned < Base\n        def on_send(node)\n          \
         add_offense(node, message: 'banned') if File.read('banned.txt').split.include?(node.method_name.to_s)\n        \
         end\n      end\n    end\n  end\nend\n",
    )
    .unwrap();
    fs::write(dir.join("banned.txt"), "foo\n").unwrap();
    fs::write(dir.join("a.rb"), "# frozen_string_literal: true\n\nfoo\nbar\n").unwrap();
    fs::write(dir.join("b.rb"), "# frozen_string_literal: true\n\nfoo\nbar\n").unwrap();
    same_cached(&dir, &[], &["-f", "e", "a.rb", "b.rb"]);
    fs::write(dir.join("banned.txt"), "bar\n").unwrap();
    same_cached(&dir, &[], &["-f", "e", "a.rb", "b.rb"]);
    let _ = fs::remove_dir_all(&dir);
}

/// A file a fallback cop raised an error on is not cached, as Runner#save_in_cache, also when the
/// child inspects its files in parallel worker processes (RuboCop's default): a warm run reports
/// the error again.
#[test]
fn cop_errors_are_not_cached() {
    if !have_rubocop() {
        eprintln!("skipped: RuboCop 1.92.2 is not on PATH");
        return;
    }
    let dir = project("errors");
    fs::write(dir.join(".rubocop.yml"), format!("require:\n  - ./cops.rb\n{REQUIRE_ONLY}")).unwrap();
    fs::write(
        dir.join("cops.rb"),
        "module RuboCop\n  module Cop\n    module Custom\n      class Boom < Base\n        def on_send(node)\n          \
         raise 'boom' if node.method?(:foo)\n        end\n      end\n    end\n  end\nend\n",
    )
    .unwrap();
    fs::write(dir.join("a.rb"), "# frozen_string_literal: true\n\nfoo\n").unwrap();
    fs::write(dir.join("b.rb"), "# frozen_string_literal: true\n\nbar\n").unwrap();
    // (Exit status not compared: RuboCop exits 1 after a cop error, rbcop 0, also without the
    // cache.)
    let args = ["-f", "e", "a.rb", "b.rb"];
    for _ in 0..2 {
        let ours = common::command(env!("CARGO_BIN_EXE_rbcop"))
            .env("HOME", &dir)
            .env("RBCOP_FALLBACK", "1")
            .args(args)
            .current_dir(&dir)
            .output()
            .unwrap();
        let theirs = common::command("rubocop").env("HOME", &dir).args(args).current_dir(&dir).output().unwrap();
        assert_eq!(text(&ours.stdout), text(&theirs.stdout));
        assert_eq!(text(&ours.stderr), text(&theirs.stderr));
        assert!(text(&theirs.stderr).contains("1 error occurred"), "{}", text(&theirs.stderr));
    }
    let _ = fs::remove_dir_all(&dir);
}

const NO_FOO_CUSTOM: &str = "module RuboCop\n  module Cop\n    module Custom\n      class NoFoo < Base\n        \
                             def on_send(node)\n          add_offense(node, message: 'foo') if node.method?(:foo)\n        \
                             end\n      end\n    end\n  end\nend\n";

/// #805 review R1: a directive in any mode can opt a disabled fallback cop in (`next +Cop`,
/// `next +Dept`, `push`, `enable-next`), so the child parses those files.
#[test]
fn directives_in_any_mode_opt_fallback_cops_in() {
    if !have_rubocop() {
        eprintln!("skipped: RuboCop 1.92.2 is not on PATH");
        return;
    }
    let dir = project("next");
    fs::write(dir.join("cops.rb"), NO_FOO_CUSTOM).unwrap();
    fs::write(
        dir.join(".rubocop.yml"),
        format!("require:\n  - ./cops.rb\n{REQUIRE_ONLY}Custom/NoFoo:\n  Enabled: false\n"),
    )
    .unwrap();
    let head = "# frozen_string_literal: true\n\n";
    fs::write(dir.join("next.rb"), format!("{head}# rubocop:next +Custom/NoFoo\nfoo\n")).unwrap();
    fs::write(dir.join("nextdep.rb"), format!("{head}# rubocop:next +Custom\nfoo\n")).unwrap();
    fs::write(dir.join("push.rb"), format!("{head}# rubocop:push +Custom/NoFoo\nfoo\n# rubocop:pop\n")).unwrap();
    fs::write(dir.join("enext.rb"), format!("{head}# rubocop:enable-next Custom/NoFoo\nfoo\n")).unwrap();
    let args = ["-f", "e", "next.rb", "push.rb", "enext.rb", "nextdep.rb"];
    same(&dir, &[], &args);
    same_cached(&dir, &[], &args);
    same_cached(&dir, &[], &args);
    let _ = fs::remove_dir_all(&dir);
}

/// #805 review R2: the child's stdin is RuboCop's (empty here), also with the result cache on,
/// so required code that reads it while loading goes on (rather than waiting for rbcop).
#[test]
fn required_code_reading_stdin_goes_on() {
    if !have_rubocop() {
        eprintln!("skipped: RuboCop 1.92.2 is not on PATH");
        return;
    }
    let dir = project("stdin");
    fs::write(
        dir.join("cops.rb"),
        format!("ACME_INPUT = $stdin.read\nwarn \"tty: #{{$stdin.tty?}}\"\n{NO_FOO_CUSTOM}"),
    )
    .unwrap();
    fs::write(dir.join(".rubocop.yml"), format!("require:\n  - ./cops.rb\n{REQUIRE_ONLY}")).unwrap();
    fs::write(dir.join("a.rb"), "# frozen_string_literal: true\n\nfoo\n").unwrap();
    for _ in 0..2 {
        // A child that hangs fails the test instead of hanging it.
        let mut child = common::command(env!("CARGO_BIN_EXE_rbcop"))
            .env("HOME", &dir)
            .env("RBCOP_FALLBACK", "1")
            .args(["-f", "e", "a.rb"])
            .current_dir(&dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let start = std::time::Instant::now();
        while child.try_wait().unwrap().is_none() {
            if start.elapsed() > std::time::Duration::from_secs(60) {
                let _ = child.kill();
                panic!("rbcop did not finish in 60 s");
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        same_cached(&dir, &[], &["-f", "e", "a.rb"]);
    }
    let _ = fs::remove_dir_all(&dir);
}

/// #805 review R3: what a fallback cop's constructor prints comes as often as from RuboCop, warm
/// too (RuboCop builds the cops' standby team for its cache key even when every file is cached).
#[test]
fn cop_constructor_output_matches_rubocop() {
    if !have_rubocop() {
        eprintln!("skipped: RuboCop 1.92.2 is not on PATH");
        return;
    }
    let dir = project("init");
    let init = "RuboCop::Cop::Base.prepend(Module.new { def initialize(*a) = super.tap { warn \"init #{self.class.name}\" \
                if self.class.name == 'RuboCop::Cop::Custom::NoFoo' } })\n";
    fs::write(dir.join("cops.rb"), format!("{init}{NO_FOO_CUSTOM}")).unwrap();
    fs::write(dir.join(".rubocop.yml"), format!("require:\n  - ./cops.rb\n{REQUIRE_ONLY}")).unwrap();
    fs::write(dir.join("a.rb"), "# frozen_string_literal: true\n\nfoo\n").unwrap();
    for _ in 0..2 {
        same_cached(&dir, &[], &["-f", "e", "a.rb"]);
    }
    same(&dir, &[], &["-f", "e", "--cache", "false", "a.rb"]);
    // Several files inspected one after another (in parallel, RuboCop's count depends on its
    // worker processes: docs/known-gaps.md).
    fs::write(dir.join("b.rb"), "# frozen_string_literal: true\n\nfoo\n").unwrap();
    fs::write(dir.join("c.rb"), "# frozen_string_literal: true\n\nbar\n").unwrap();
    for _ in 0..2 {
        same_cached(&dir, &[], &["-f", "e", "--no-parallel", "a.rb", "b.rb", "c.rb"]);
    }
    let _ = fs::remove_dir_all(&dir);
}

/// #805 review R4: the child's skip asks the cops RuboCop builds (constructed, as
/// Team#roundup_relevant_cops does) whether they take a file, so a `relevant_file?` that reads
/// state its constructor set still sees it.
#[test]
fn relevance_uses_constructed_cops() {
    if !have_rubocop() {
        eprintln!("skipped: RuboCop 1.92.2 is not on PATH");
        return;
    }
    let dir = project("constructed");
    fs::write(dir.join("a.rb"), "# frozen_string_literal: true\n\nfoo\n").unwrap();
    let on_send =
        "    def on_send(node)\n      add_offense(node, message: 'foo') if node.method?(:foo)\n    end\n  end\nend\n";
    for (state, config) in [
        (
            "    def initialize(config = nil, options = nil)\n      super\n      @active = true\n    end\n\n    \
             def relevant_file?(file)\n      @active == true && super\n    end\n\n",
            "",
        ),
        (
            "    def initialize(config = nil, options = nil)\n      super\n      @globs = cop_config.fetch('Paths', [])\n    \
             end\n\n    def relevant_file?(file)\n      Array(@globs).any? { |g| File.fnmatch?(g, file, File::FNM_PATHNAME \
             | File::FNM_EXTGLOB) }\n    end\n\n",
            "Custom/NoFoo:\n  Paths:\n    - '**/*.rb'\n",
        ),
    ] {
        fs::write(
            dir.join("cops.rb"),
            format!("module RuboCop::Cop::Custom\n  class NoFoo < RuboCop::Cop::Base\n{state}{on_send}"),
        )
        .unwrap();
        fs::write(dir.join(".rubocop.yml"), format!("require:\n  - ./cops.rb\n{REQUIRE_ONLY}{config}")).unwrap();
        same(&dir, &[], &["-f", "e", "--cache", "false", "a.rb"]);
        same_cached(&dir, &[], &["-f", "e", "a.rb"]);
        same_cached(&dir, &[], &["-f", "e", "a.rb"]);
    }
    let _ = fs::remove_dir_all(&dir);
}

/// #805 review R5: the child asks whether a cop takes a file of the cop instances RuboCop would
/// ask (fresh for each file, Team#investigate_with_corrector's be_ready), so a `relevant_file?`
/// that keeps state per instance answers for that file only.
#[test]
fn relevance_is_asked_of_fresh_cops_per_file() {
    if !have_rubocop() {
        eprintln!("skipped: RuboCop 1.92.2 is not on PATH");
        return;
    }
    let dir = project("fresh");
    fs::write(
        dir.join("cops.rb"),
        "module RuboCop::Cop::Custom\n  class NoFoo < RuboCop::Cop::Base\n    def relevant_file?(file)\n      \
         return @relevant if defined?(@relevant)\n\n      @relevant = super\n    end\n\n    def on_send(node)\n      \
         add_offense(node, message: 'foo') if node.method?(:foo)\n    end\n  end\nend\n",
    )
    .unwrap();
    fs::write(
        dir.join(".rubocop.yml"),
        format!("require:\n  - ./cops.rb\n{REQUIRE_ONLY}Custom/NoFoo:\n  Exclude:\n    - a.rb\n"),
    )
    .unwrap();
    for f in ["a.rb", "b.rb", "c.rb"] {
        fs::write(dir.join(f), "# frozen_string_literal: true\n\nfoo\n").unwrap();
    }
    for files in [["a.rb", "b.rb", "c.rb"], ["b.rb", "a.rb", "c.rb"]] {
        for parallel in [&[][..], &["--no-parallel"]] {
            let args: Vec<&str> = ["-f", "e"].iter().chain(parallel).chain(&files).copied().collect();
            let cold: Vec<&str> = args.iter().copied().chain(["--cache", "false"]).collect();
            same(&dir, &[], &cold);
            same_cached(&dir, &[], &args);
            same_cached(&dir, &[], &args);
            let _ = fs::remove_dir_all(dir.join(".cache"));
        }
    }
    let _ = fs::remove_dir_all(&dir);
}
