//! Cops rbcop cannot run: the stderr notice, and RBCOP_FALLBACK=1 (src/fallback.rs) compared with
//! a live RuboCop 1.91 when one is on PATH.

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

fn project(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rbcop-fallback-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("Gemfile"), "").unwrap();
    fs::write(dir.join("no_foo.rb"), COP).unwrap();
    fs::write(dir.join(".rubocop.yml"), CONFIG).unwrap();
    fs::write(dir.join("a.rb"), "# frozen_string_literal: true\n\nfoo(1)\nx = \"é\"; foo 2\nputs x\n").unwrap();
    fs::write(dir.join("b.rb"), "# frozen_string_literal: true\n\nfoo(3)\n").unwrap();
    dir
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
    Command::new("rubocop").arg("-v").output().is_ok_and(|o| o.stdout == b"1.91.0\n")
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
        "require 'rubocop'\nrequire_relative 'rubocop/fake_ns'\nrequire_relative 'rubocop/fake_ns/inject'\n\
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
        // A name that is no constant, or a module, ends the run (#530).
        for (class, want) in [
            (
                "Nope::Plugin",
                "Failed while configuring plugin `rubocop-fake_ns': no constant with name `Nope::Plugin' was found.\n",
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
                    "require 'rubocop'\nrequire_relative 'rubocop/fake_ns'\nrequire_relative 'rubocop/fake_ns/inject'\n\
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
            "require 'rubocop'\nrequire_relative 'rubocop/fake_ns'\nrequire_relative 'rubocop/fake_ns/inject'\n\
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
    let mut ours = common::command(env!("CARGO_BIN_EXE_rbcop"));
    ours.env("HOME", dir).env("RBCOP_FALLBACK", "1");
    let mut theirs = common::command("rubocop");
    theirs.env("HOME", dir).args(["--cache", "false"]);
    for (k, v) in env {
        ours.env(k, v);
        theirs.env(k, v);
    }
    let ours = ours.args(args).current_dir(dir).output().unwrap();
    let theirs = theirs.args(args).current_dir(dir).output().unwrap();
    assert_eq!(text(&ours.stdout), text(&theirs.stdout), "{env:?} {args:?}");
    assert_eq!(text(&ours.stderr), text(&theirs.stderr), "{env:?} {args:?}");
    assert_eq!(ours.status.code(), theirs.status.code(), "{env:?} {args:?}");
}

#[test]
fn fallback_forwards_the_command_line() {
    if !have_rubocop() {
        eprintln!("skipped: RuboCop 1.91.0 is not on PATH");
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
        eprintln!("skipped: RuboCop 1.91.0 is not on PATH");
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
        eprintln!("skipped: RuboCop 1.91.0 is not on PATH");
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
        eprintln!("skipped: RuboCop 1.91.0 is not on PATH");
        return;
    }
    let dir = project("report");
    for format in ["simple", "emacs", "clang", "progress", "fuubar"] {
        let ours = rbcop(&dir, true, &["-f", format]);
        let theirs = rubocop(&dir, &["--cache", "false", "-f", format]);
        assert_eq!(text(&ours.stdout), text(&theirs.stdout), "{format}");
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
        eprintln!("skipped: RuboCop 1.91.0 is not on PATH");
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
    fs::write(dir.join("Gemfile.lock"), "GEM\n  specs:\n    rubocop (1.91.0)\n").unwrap();
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
        |what: &str| format!("rbcop: running bundle exec rubocop for {what}; set RBCOP_FALLBACK=0 to never run Ruby\n");
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
        eprintln!("skipped: RuboCop 1.91.0 is not on PATH");
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
