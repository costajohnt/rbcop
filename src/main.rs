//! The `rubocop` command line: options, formatters and exit status as RuboCop 1.91.0 has them
//! (lib/rubocop/cli.rb, options.rb, runner.rb, cli/command/*.rb).

use rbcop::config::{Options, RUBOCOP_VERSION, Resolved, Store, expand_path};
use rbcop::cop::Severity;
use rbcop::formatter::{self, Annotate, Ctx, FailLevel, FileOut, Opts};
use rbcop::runner::{self, FileReport};
use rbcop::{cops, targets};
use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;
use std::{env, fs, thread};

const USAGE: &str = "Usage: rbcop [options] [file1, file2, ...]

Accepts RuboCop's command-line options. Supported:
  -c, --config FILE, --force-default-config, --only/--except COPS, --force-exclusion,
  --ignore-parent-exclusion, --enable-pending-cops, --disable-pending-cops, -s/--stdin FILE,
  -F/--fail-fast, --fail-level SEVERITY, -f/--format FORMATTER, -o/--out FILE,
  -D/--[no-]display-cop-names, -E/--extra-details, -S/--display-style-guide, --display-time,
  --display-only-fail-level-offenses, --display-only-correctable, --display-only-safe-correctable,
  --display-only-failed, --stderr, --[no-]color, -L/--list-target-files, --show-cops [COPS],
  -v/--version, -V/--verbose-version.
Ignored: -C/--cache, --cache-root, -P/--[no-]parallel, --no-server, --editor-mode, --raise-cop-error.

Formatters: [p]rogress (default), [s]imple, [c]lang, [e]macs, [fi]les, [fu]ubar, [g]ithub, [h]tml,
  [j]son, [ju]nit, [m]arkdown, [o]ffenses, [pa]cman, [q]uiet, [sa]rif, [t]ap, [w]orst.";

/// RuboCop's exit statuses.
const OFFENSES: u8 = 1;
const ERROR: u8 = 2;

#[derive(Clone, Copy, PartialEq)]
enum Arg {
    None,
    Required,
    Optional,
}

/// Every option RuboCop 1.91.0 defines: (long name, short, argument, what rbcop does with it).
/// Long names with `[no-]` also accept `--no-NAME`.
#[derive(Clone, Copy, PartialEq)]
enum Use {
    Handle,
    Ignore,
    Unsupported,
}

const SPECS: &[(&str, Option<char>, Arg, Use)] = &[
    ("lint", Some('l'), Arg::None, Use::Unsupported),
    ("fix-layout", Some('x'), Arg::None, Use::Unsupported),
    ("safe", None, Arg::None, Use::Unsupported),
    ("except", None, Arg::Optional, Use::Handle),
    ("only", None, Arg::Optional, Use::Handle),
    ("only-guide-cops", None, Arg::None, Use::Unsupported),
    ("fail-fast", Some('F'), Arg::None, Use::Handle),
    ("disable-pending-cops", None, Arg::None, Use::Handle),
    ("enable-pending-cops", None, Arg::None, Use::Handle),
    ("disable-all-cops", None, Arg::None, Use::Unsupported),
    ("enable-all-cops", None, Arg::None, Use::Unsupported),
    ("[no-]preview", None, Arg::None, Use::Unsupported),
    ("ignore-disable-comments", None, Arg::None, Use::Unsupported),
    ("force-exclusion", None, Arg::None, Use::Handle),
    ("only-recognized-file-types", None, Arg::None, Use::Unsupported),
    ("ignore-parent-exclusion", None, Arg::None, Use::Handle),
    ("ignore-unrecognized-cops", None, Arg::None, Use::Unsupported),
    ("force-default-config", None, Arg::None, Use::Handle),
    ("changed", None, Arg::Optional, Use::Unsupported),
    ("stdin", Some('s'), Arg::Required, Use::Handle),
    ("editor-mode", None, Arg::None, Use::Ignore),
    ("[no-]parallel", Some('P'), Arg::None, Use::Handle),
    ("raise-cop-error", None, Arg::None, Use::Ignore),
    ("fail-level", None, Arg::Required, Use::Handle),
    ("cache", Some('C'), Arg::Required, Use::Handle),
    ("cache-root", None, Arg::Required, Use::Handle),
    ("lsp", None, Arg::None, Use::Unsupported),
    ("mcp", None, Arg::None, Use::Unsupported),
    ("[no-]server", None, Arg::None, Use::Handle),
    ("restart-server", None, Arg::None, Use::Unsupported),
    ("start-server", None, Arg::None, Use::Unsupported),
    ("stop-server", None, Arg::None, Use::Unsupported),
    ("server-status", None, Arg::None, Use::Unsupported),
    ("no-detach", None, Arg::None, Use::Unsupported),
    ("format", Some('f'), Arg::Required, Use::Handle),
    ("[no-]display-cop-names", Some('D'), Arg::None, Use::Handle),
    ("extra-details", Some('E'), Arg::None, Use::Handle),
    ("display-style-guide", Some('S'), Arg::None, Use::Handle),
    ("out", Some('o'), Arg::Required, Use::Handle),
    ("stderr", None, Arg::None, Use::Handle),
    ("display-time", None, Arg::None, Use::Handle),
    ("display-only-failed", None, Arg::None, Use::Handle),
    ("display-only-fail-level-offenses", None, Arg::None, Use::Handle),
    ("display-only-correctable", None, Arg::None, Use::Handle),
    ("display-only-safe-correctable", None, Arg::None, Use::Handle),
    ("display-suppressed", None, Arg::None, Use::Unsupported),
    ("autocorrect", Some('a'), Arg::None, Use::Handle),
    ("auto-correct", None, Arg::None, Use::Unsupported),
    ("safe-auto-correct", None, Arg::None, Use::Unsupported),
    ("autocorrect-all", Some('A'), Arg::None, Use::Handle),
    ("auto-correct-all", None, Arg::None, Use::Unsupported),
    ("disable-uncorrectable", None, Arg::None, Use::Unsupported),
    ("diff", None, Arg::None, Use::Unsupported),
    ("auto-gen-config", None, Arg::None, Use::Unsupported),
    ("regenerate-todo", None, Arg::None, Use::Unsupported),
    ("report-unused-todo-entries", None, Arg::None, Use::Unsupported),
    ("exclude-limit", None, Arg::Required, Use::Unsupported),
    ("no-exclude-limit", None, Arg::None, Use::Unsupported),
    ("[no-]offense-counts", None, Arg::None, Use::Unsupported),
    ("[no-]auto-gen-only-exclude", None, Arg::None, Use::Unsupported),
    ("[no-]auto-gen-timestamp", None, Arg::None, Use::Unsupported),
    ("[no-]auto-gen-enforced-style", None, Arg::None, Use::Unsupported),
    ("list-target-files", Some('L'), Arg::None, Use::Handle),
    ("list-enabled-cops-for", None, Arg::Required, Use::Unsupported),
    ("show-cops", None, Arg::Optional, Use::Handle),
    ("show-docs-url", None, Arg::Optional, Use::Unsupported),
    ("init", None, Arg::None, Use::Unsupported),
    ("config", Some('c'), Arg::Required, Use::Handle),
    ("debug", Some('d'), Arg::None, Use::Unsupported),
    ("plugin", None, Arg::Required, Use::Unsupported),
    ("require", Some('r'), Arg::Required, Use::Unsupported),
    ("[no-]color", None, Arg::None, Use::Handle),
    ("version", Some('v'), Arg::None, Use::Handle),
    ("verbose-version", Some('V'), Arg::None, Use::Handle),
    ("profile", None, Arg::None, Use::Unsupported),
    ("memory", None, Arg::None, Use::Unsupported),
    ("help", Some('h'), Arg::None, Use::Handle),
];

/// A failure before linting starts: (message, how RuboCop words it on stderr).
enum Fail {
    /// OptionParser::ParseError: message, then the usage hint.
    Parse(String),
    /// OptionArgumentError and friends: message only.
    Arg(String),
    /// Something rbcop does not implement yet.
    Unsupported(String),
}

#[derive(Default)]
struct Cli {
    opts: Options,
    /// -a / -A
    autocorrect: Option<rbcop::autocorrect::Mode>,
    only: Option<Vec<String>>,
    except: Vec<String>,
    /// `-f`/`-o` pairs, in order.
    formatters: Vec<(String, Option<PathBuf>)>,
    /// `-o` before any `-f`.
    output_path: Option<PathBuf>,
    /// `@options[:format]`: the last `-f` value.
    format: Option<String>,
    list_targets: bool,
    force_exclusion: bool,
    paths: Vec<PathBuf>,
    annotate: Annotate,
    display_time: bool,
    fail_level: Option<FailLevel>,
    only_fail_level: bool,
    only_correctable: bool,
    only_safe_correctable: bool,
    only_failed: bool,
    color: Option<bool>,
    stdin: Option<String>,
    stderr: bool,
    fail_fast: bool,
    parallel: Option<bool>,
    cache: Option<String>,
    show_cops: Option<Vec<String>>,
    version: bool,
    verbose_version: bool,
    help: bool,
    /// EXITING_OPTIONS seen, in order (for "Incompatible cli options").
    exiting: Vec<&'static str>,
}

/// Shellwords.split
fn shellsplit(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur: Option<String> = None;
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => out.extend(cur.take()),
            '\'' => {
                let w = cur.get_or_insert_with(String::new);
                w.extend(chars.by_ref().take_while(|&c| c != '\''));
            }
            '"' => {
                let w = cur.get_or_insert_with(String::new);
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => w.extend(chars.next()),
                        c => w.push(c),
                    }
                }
            }
            '\\' => cur.get_or_insert_with(String::new).extend(chars.next()),
            c => cur.get_or_insert_with(String::new).push(c),
        }
    }
    out.extend(cur);
    out
}

/// ArgumentsFile (`.rubocop` in the working directory) + ArgumentsEnv (`RUBOCOP_OPTS`) + argv.
///
/// Deliberate difference: a checked-out project controls `.rubocop`, so `-o/--out` there is refused
/// rather than letting a repository choose which files rbcop writes.
fn all_args() -> Result<Vec<String>, Fail> {
    let mut args = Vec::new();
    if Path::new(".rubocop").is_file() {
        let from_file = shellsplit(&fs::read_to_string(".rubocop").unwrap_or_default());
        // Checked token by token rather than by parsing the file alone: a trailing `-o` would fail
        // to parse there yet, combined with argv, take the user's first path as the output file.
        // `--o`, `--ou`, `--out[=...]` and any short-option cluster containing `o` are refused
        // (this also refuses `-fo`, which RuboCop reads as format `o`; pass that on the command line).
        let may_write = |t: &String| {
            (t.starts_with("--o") && "--out".starts_with(t.split('=').next().unwrap_or("")))
                || (t.starts_with('-') && !t.starts_with("--") && t[1..].contains('o'))
        };
        let writes = |cli: &Cli| cli.output_path.is_some() || cli.formatters.iter().any(|f| f.1.is_some());
        if from_file.iter().any(may_write) || parse_args(from_file.clone()).is_ok_and(|cli| writes(&cli)) {
            return Err(Fail::Unsupported(
                "-o/--out in the .rubocop arguments file is refused; pass it on the command line".into(),
            ));
        }
        args.extend(from_file);
    }
    args.extend(shellsplit(&env::var("RUBOCOP_OPTS").unwrap_or_default()));
    args.extend(env::args().skip(1));
    Ok(args)
}

/// Find the option spec for `--name` (with OptionParser's unique-prefix completion).
fn long_spec(name: &str) -> Result<(usize, bool), Fail> {
    let mut names: Vec<(String, usize, bool)> = Vec::new();
    for (i, (long, ..)) in SPECS.iter().enumerate() {
        match long.strip_prefix("[no-]") {
            Some(base) => {
                names.push((base.to_string(), i, false));
                names.push((format!("no-{base}"), i, true));
            }
            None => names.push((long.to_string(), i, false)),
        }
    }
    if let Some((_, i, neg)) = names.iter().find(|(n, ..)| n == name) {
        return Ok((*i, *neg));
    }
    let hits: Vec<_> = names.iter().filter(|(n, ..)| n.starts_with(name)).collect();
    match hits.as_slice() {
        [(_, i, neg)] => Ok((*i, *neg)),
        [] => Err(Fail::Parse(format!("invalid option: --{name}"))),
        _ => Err(Fail::Parse(format!("ambiguous option: --{name}"))),
    }
}

fn parse_args(args: Vec<String>) -> Result<Cli, Fail> {
    let mut cli = Cli::default();
    let mut it = args.into_iter().peekable();
    let mut rest_are_paths = false;
    while let Some(arg) = it.next() {
        if rest_are_paths || arg == "-" || !arg.starts_with('-') {
            cli.paths.push(PathBuf::from(arg));
            continue;
        }
        if arg == "--" {
            rest_are_paths = true;
            continue;
        }
        // (spec index, negated, inline value, how the option was written)
        let mut pending: Vec<(usize, bool, Option<String>, String)> = Vec::new();
        if let Some(long) = arg.strip_prefix("--") {
            let (name, inline) = match long.split_once('=') {
                Some((n, v)) => (n, Some(v.to_string())),
                None => (long, None),
            };
            let (i, neg) = long_spec(name)?;
            pending.push((i, neg, inline, format!("--{name}")));
        } else {
            let shorts: Vec<char> = arg[1..].chars().collect();
            let mut k = 0;
            while k < shorts.len() {
                let c = shorts[k];
                let Some(i) = SPECS.iter().position(|s| s.1 == Some(c)) else {
                    return Err(Fail::Parse(format!("invalid option: -{c}")));
                };
                if SPECS[i].2 != Arg::None {
                    let rest: String = shorts[k + 1..].iter().collect();
                    pending.push((i, false, (!rest.is_empty()).then_some(rest), format!("-{c}")));
                    break;
                }
                pending.push((i, false, None, format!("-{c}")));
                k += 1;
            }
        }
        for (i, neg, inline, written) in pending {
            let (long, _, kind, usage) = SPECS[i];
            let value = match kind {
                Arg::None => {
                    if inline.is_some() {
                        return Err(Fail::Parse(format!("needless argument: {written}")));
                    }
                    None
                }
                Arg::Required => match inline.or_else(|| it.next()) {
                    Some(v) => Some(v),
                    None => return Err(Fail::Parse(format!("missing argument: {written}"))),
                },
                Arg::Optional => inline.or_else(|| it.next_if(|n| !n.starts_with('-'))),
            };
            let name = long.trim_start_matches("[no-]");
            if usage == Use::Unsupported {
                let shown = if neg { format!("--no-{name}") } else { format!("--{name}") };
                return Err(Fail::Unsupported(format!("{shown} is not supported by rbcop yet")));
            }
            apply(&mut cli, name, neg, value)?;
        }
    }
    Ok(cli)
}

fn cop_list(name: &str, value: Option<String>) -> Result<Vec<String>, Fail> {
    let Some(list) = value else {
        return Err(Fail::Arg(format!("--{name} argument should be [COP1,COP2,...].")));
    };
    Ok(if list.is_empty() { vec![String::new()] } else { list.split(',').map(String::from).collect() })
}

/// `--fail-level`: a severity name or code (OptionParser completes unique prefixes).
fn fail_level(v: &str) -> Result<FailLevel, Fail> {
    const NAMES: &[&str] = &["info", "refactor", "convention", "warning", "error", "fatal", "autocorrect"];
    const CODES: &[(&str, &str)] = &[
        ("I", "info"),
        ("R", "refactor"),
        ("C", "convention"),
        ("W", "warning"),
        ("E", "error"),
        ("F", "fatal"),
        ("A", "autocorrect"),
    ];
    let name = if let Some((_, n)) = CODES.iter().find(|(c, _)| *c == v) {
        *n
    } else if NAMES.contains(&v) {
        v
    } else {
        let hits: Vec<&'static str> = NAMES.iter().copied().filter(|n| n.starts_with(v)).collect();
        match hits.as_slice() {
            [one] if !v.is_empty() => *one,
            [_, _, ..] => return Err(Fail::Parse(format!("ambiguous argument: --fail-level {v}"))),
            _ => return Err(Fail::Parse(format!("invalid argument: --fail-level {v}"))),
        }
    };
    Ok(match Severity::parse(name) {
        Some(s) => FailLevel::Severity(s),
        None => FailLevel::Autocorrect,
    })
}

fn apply(cli: &mut Cli, name: &str, neg: bool, value: Option<String>) -> Result<(), Fail> {
    match name {
        "only" => {
            let mut list = cop_list(name, value)?;
            if !list.iter().any(|c| c == "Lint/Syntax") {
                list.insert(0, "Lint/Syntax".into());
            }
            cli.opts.only = list.clone();
            cli.only = Some(list);
        }
        "except" => cli.except = cop_list(name, value)?,
        "config" => cli.opts.config_file = value.map(PathBuf::from),
        "force-default-config" => cli.opts.force_default_config = true,
        "force-exclusion" => cli.force_exclusion = true,
        "ignore-parent-exclusion" => cli.opts.ignore_parent_exclusion = true,
        "enable-pending-cops" => cli.opts.enable_pending_cops = true,
        "disable-pending-cops" => cli.opts.disable_pending_cops = true,
        "fail-fast" => cli.fail_fast = true,
        "stdin" => cli.stdin = value,
        "parallel" => cli.parallel = Some(!neg),
        "fail-level" => cli.fail_level = Some(fail_level(&value.unwrap_or_default())?),
        "cache" => cli.cache = value,
        "cache-root" => {
            if cli.cache.as_deref() == Some("false") {
                return Err(Fail::Arg("--cache-root cannot be used with --cache false".into()));
            }
        }
        "server" if !neg => return Err(Fail::Unsupported("--server is not supported by rbcop yet".into())),
        "format" => {
            let v = value.unwrap_or_default();
            cli.format = Some(v.clone());
            cli.formatters.push((v, None));
        }
        "out" => {
            let p = value.map(PathBuf::from);
            match cli.formatters.last_mut() {
                Some(last) => {
                    if last.1.is_none() {
                        last.1 = p;
                    }
                }
                None => cli.output_path = p,
            }
        }
        "display-cop-names" => cli.annotate.display_cop_names = Some(!neg),
        "extra-details" => cli.annotate.extra_details = true,
        "display-style-guide" => cli.annotate.display_style_guide = true,
        "stderr" => cli.stderr = true,
        "display-time" => cli.display_time = true,
        "autocorrect" => cli.autocorrect = Some(rbcop::autocorrect::Mode::Safe),
        "autocorrect-all" => cli.autocorrect = Some(rbcop::autocorrect::Mode::All),
        "display-only-failed" => cli.only_failed = true,
        "display-only-fail-level-offenses" => cli.only_fail_level = true,
        "display-only-correctable" => cli.only_correctable = true,
        "display-only-safe-correctable" => cli.only_safe_correctable = true,
        "list-target-files" => cli.list_targets = true,
        "show-cops" => {
            cli.show_cops = Some(value.map(|v| v.split(',').map(String::from).collect()).unwrap_or_default());
            cli.exiting.push("show_cops");
        }
        "color" => cli.color = Some(!neg),
        "version" => {
            cli.version = true;
            cli.exiting.push("version");
        }
        "verbose-version" => {
            cli.verbose_version = true;
            cli.exiting.push("verbose_version");
        }
        "help" => cli.help = true,
        _ => {} // ignored options
    }
    Ok(())
}

/// OptionsValidator#validate_compatibility (the checks that apply to what rbcop supports).
fn validate(cli: &mut Cli) -> Result<(), Fail> {
    if cli.only.as_ref().is_some_and(|o| {
        o.iter().any(|c| c == "Lint/RedundantCopDisableDirective" || c == "RedundantCopDisableDirective")
    }) {
        return Err(Fail::Arg("Lint/RedundantCopDisableDirective cannot be used with --only.".into()));
    }
    if cli.except.iter().any(|c| c == "Lint/Syntax" || c == "Syntax") {
        return Err(Fail::Arg("Syntax checking cannot be turned off.".into()));
    }
    if cli.cache.as_ref().is_some_and(|c| c != "true" && c != "false") {
        return Err(Fail::Arg("-C/--cache argument must be true or false".into()));
    }
    if cli.only_failed && cli.format.as_deref() != Some("junit") {
        return Err(Fail::Arg("--display-only-failed can only be used together with --format junit.".into()));
    }
    if cli.only_failed && (cli.only_correctable || cli.only_safe_correctable) {
        return Err(Fail::Arg("--display-only-failed cannot be used together with other display options.".into()));
    }
    if cli.parallel.is_some() {
        let invalid: Vec<&str> = [
            cli.fail_fast.then_some("-F/--fail-fast"),
            (cli.cache.as_deref() == Some("false")).then_some("--cache false"),
        ]
        .into_iter()
        .flatten()
        .collect();
        if !invalid.is_empty() {
            println!("-P/--parallel is being ignored because it is not compatible with {}.", invalid.join(", "));
        }
    }
    if cli.exiting.len() > 1 {
        let list: Vec<String> = cli.exiting.iter().map(|k| format!(":{k}")).collect();
        return Err(Fail::Arg(format!("Incompatible cli options: [{}]", list.join(", "))));
    }
    if cli.stdin.is_some() && !cli.paths.is_empty() {
        return Err(Fail::Arg(
            "-s/--stdin requires exactly one path, relative to the root of the project. RuboCop will use this \
             path to determine which cops are enabled (via eg. Include/Exclude), and so that certain cops like \
             Naming/FileName can be checked."
                .into(),
        ));
    }
    Ok(())
}

/// Rainbow's initial global state, then CLI#act_on_options.
fn global_color(option: Option<bool>) -> bool {
    option.unwrap_or_else(|| {
        let mut on = std::io::stdout().is_terminal() && std::io::stderr().is_terminal();
        if env::var("TERM").as_deref() == Ok("dumb") {
            on = false;
        }
        if env::var("CLICOLOR_FORCE").as_deref() == Ok("1") {
            on = true;
        }
        on
    })
}

fn red(on: bool, s: &str) -> String {
    formatter::Paint(on).red(s)
}

/// Float#to_s after #round(5).
fn ruby_seconds(x: f64) -> String {
    let r = (x * 1e5).round() / 1e5;
    if r > 0.0 && r < 1e-4 {
        format!("{:.1}e-05", r * 1e5)
    } else if r.fract() == 0.0 {
        format!("{r:.1}")
    } else {
        format!("{r}")
    }
}

fn main() -> ExitCode {
    let t0 = Instant::now();
    // A panic inside a cop is reported RuboCop-style by runner::guarded, so Rust's own panic text is
    // kept for every other panic (and for cop panics under RBCOP_DEBUG).
    let default_hook = std::panic::take_hook();
    let debug = std::env::var_os("RBCOP_DEBUG").is_some();
    std::panic::set_hook(Box::new(move |info| {
        if debug || !runner::in_cop() {
            default_hook(info);
        }
    }));
    let parsed = all_args().and_then(parse_args);
    let (display_time, color_opt) = match &parsed {
        Ok(cli) => (cli.display_time, cli.color),
        Err(_) => (false, None),
    };
    let global = global_color(color_opt);
    let status = match parsed.and_then(|mut cli| validate(&mut cli).map(|_| cli)) {
        Err(Fail::Parse(m)) => {
            eprintln!("{m}\nFor usage information, use --help");
            ERROR
        }
        Err(Fail::Arg(m)) => {
            eprintln!("{}", red(global, &m));
            ERROR
        }
        Err(Fail::Unsupported(m)) => {
            eprintln!("rbcop: {m}");
            ERROR
        }
        Ok(cli) => match run(cli, global) {
            Ok(code) => code,
            Err(e) => {
                eprintln!("{}", red(global, &e));
                ERROR
            }
        },
    };
    if display_time {
        println!("Finished in {} seconds", ruby_seconds(t0.elapsed().as_secs_f64()));
    }
    ExitCode::from(status)
}

fn ruby_version_string(v: f64) -> String {
    let s = format!("{v}");
    if s.contains('.') { s } else { format!("{s}.0") }
}

/// `--show-cops`: CLI::Command::ShowCops for the cops rbcop implements.
fn show_cops(patterns: &[String], cfg: &Resolved, pwd: &str) {
    let mut names: Vec<&'static str> = cops::ALL.iter().map(|r| r.name).collect();
    names.sort_unstable();
    let show_all = patterns.is_empty();
    let selected = |name: &str| {
        show_all
            || patterns
                .iter()
                .any(|p| if p.contains('*') { rbcop::config::glob::fnmatch(p, name, false) } else { p == name })
    };
    let mut out = String::new();
    if show_all {
        out.push_str(&format!("# Available cops ({}) + config for {pwd}: \n", names.len()));
    }
    let mut depts: Vec<&str> = names.iter().filter_map(|n| n.rsplit_once('/').map(|(d, _)| d)).collect();
    depts.dedup();
    for dept in depts {
        let in_dept: Vec<&str> =
            names.iter().copied().filter(|n| n.rsplit_once('/').map(|(d, _)| d) == Some(dept)).collect();
        let chosen: Vec<&str> = in_dept.into_iter().filter(|n| selected(n)).collect();
        if show_all {
            out.push_str(&format!("# Department '{dept}' ({}):\n", chosen.len()));
        }
        for name in chosen {
            if formatter::supports_autocorrect(name) {
                out.push_str("# Supports --autocorrect\n");
            }
            out.push_str(&format!("{name}:\n"));
            // Config#for_cop: the cop's own hash with Enabled resolved to true/false.
            let mut map = cfg.hash.map(name).cloned().unwrap_or_default();
            let enabled = rbcop::config::value::Value::Bool(cfg.cop(name).enabled);
            match map.get_mut("Enabled") {
                Some(v) => *v = enabled,
                None => map.insert("Enabled", enabled),
            }
            for line in formatter::yaml::dump(&map).lines() {
                out.push_str(&format!("  {line}\n"));
            }
            out.push('\n');
        }
    }
    print!("{out}");
}

/// RuboCop::Version.verbose
fn verbose_version(cfg: &Resolved) -> String {
    let target = cfg.target_ruby;
    let parser = if target >= 3.3 {
        let engine = cfg.all_cops().str("ParserEngine").map(|e| format!(", {e}")).unwrap_or_default();
        format!("Parser 3.3.12.0, Prism 1.9.0{engine}")
    } else {
        "Parser 3.3.12.0".to_string()
    };
    format!(
        "{RUBOCOP_VERSION} (using {parser}, rubocop-ast 1.50.0, analyzing as Ruby {}, running on ruby 4.0.6) [{}]",
        ruby_version_string(target),
        formatter::ruby_platform()
    )
}

/// Returns the exit status.
fn run(cli: Cli, global: bool) -> Result<u8, String> {
    if cli.help {
        println!("{USAGE}");
        return Ok(0);
    }
    if cli.autocorrect.is_some() && cli.stdin.is_some() {
        // ponytail: RuboCop prints the corrected source after a separator line; not ported yet.
        return Err("rbcop: --stdin with -a/-A is not supported yet".into());
    }
    let names: Vec<&'static str> = cops::ALL.iter().map(|c| c.name).collect();
    // ConfigLoader: a missing -c file is an error, even with --force-default-config.
    if let Some(c) = &cli.opts.config_file {
        let abs = expand_path(c, &rbcop::config::cwd());
        if !abs.is_file() {
            return Err(format!("Configuration file not found: {}", abs.display()));
        }
    }
    let mut store = Store::new(cli.opts.clone(), names.clone())?;
    let cwd = store.cwd().to_path_buf();
    let pwd_config = store.for_dir(&cwd)?;
    let pwd = cwd.display().to_string();

    // CLI#handle_exiting_options
    if cli.version {
        println!("{RUBOCOP_VERSION}");
        return Ok(0);
    }
    if cli.verbose_version {
        println!("{}", verbose_version(&pwd_config));
        return Ok(0);
    }
    if let Some(patterns) = &cli.show_cops {
        show_cops(patterns, &pwd_config, &pwd);
        return Ok(0);
    }

    // CLI#apply_default_formatter, then FormatterSet#add_formatter's lookup.
    let pairs: Vec<(String, Option<PathBuf>)> = if cli.formatters.is_empty() {
        let default = pwd_config.all_cops().str("DefaultFormatter").unwrap_or("progress").to_string();
        vec![(default, cli.output_path.clone())]
    } else {
        cli.formatters.clone()
    };
    let mut resolved = Vec::new();
    for (key, path) in &pairs {
        let name = formatter::resolve(key)?;
        if name == "[a]utogenconf" {
            return Err("rbcop: the autogenconf formatter is not supported yet".into());
        }
        resolved.push((name, path.clone()));
    }

    // Runner#find_target_files. Paths that do not exist are kept, as RuboCop keeps them, and fail
    // when their turn to be inspected comes.
    let mut files: Vec<PathBuf> = Vec::new();
    let stdin_text = match &cli.stdin {
        Some(name) => {
            let mut buf = Vec::new();
            std::io::stdin().read_to_end(&mut buf).map_err(|e| e.to_string())?;
            let abs = expand_path(Path::new(name), &cwd);
            let excluded = cli.force_exclusion && store.for_dir(&cwd)?.file_to_exclude(&abs);
            if !excluded {
                files.push(abs);
            }
            Some(String::from_utf8_lossy(&buf).into_owned())
        }
        None => {
            let args = if cli.paths.is_empty() { vec![PathBuf::new()] } else { cli.paths.clone() };
            for arg in args {
                let abs = expand_path(&arg, &cwd);
                if !arg.as_os_str().is_empty() && !abs.exists() {
                    files.push(abs);
                    continue;
                }
                let one = if arg.as_os_str().is_empty() { vec![] } else { vec![arg] };
                let mut found = targets::find(&mut store, &one, cli.force_exclusion)?;
                if cli.fail_fast && abs.is_dir() {
                    // TargetFinder#order under --fail-fast: most recently modified (whole seconds) first.
                    // ponytail: ties keep path order; Ruby's unstable sort_by may order them differently.
                    let secs = |p: &PathBuf| {
                        fs::metadata(p)
                            .and_then(|m| m.modified())
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map_or(0, |d| d.as_secs())
                    };
                    found.sort_by_key(|p| std::cmp::Reverse(secs(p)));
                }
                files.extend(found);
            }
            let mut seen = std::collections::HashSet::new();
            files.retain(|f| seen.insert(f.clone()));
            None
        }
    };

    let cx = Ctx {
        pwd: pwd.clone(),
        opts: Opts {
            color: cli.color,
            display_style_guide: cli.annotate.display_style_guide,
            display_only_failed: cli.only_failed,
            fail_level: cli.fail_level,
            only: cli.only.clone(),
            except: cli.except.clone(),
        },
        global_color: global,
        pwd_config: pwd_config.clone(),
        default_config: Store::new(Options { force_default_config: true, ..Options::default() }, names)?
            .for_dir(&cwd)?,
        io_console: resolved.iter().any(|(n, _)| matches!(*n, "[fu]ubar" | "[o]ffenses")),
        stdout_tty: std::io::stdout().is_terminal(),
    };

    if cli.list_targets {
        for f in &files {
            println!("{}", cx.relative(f));
        }
        return Ok(0);
    }

    // Resolve configs up front (single-threaded), then lint in parallel.
    let mut jobs = Vec::with_capacity(files.len());
    for f in &files {
        jobs.push((f.clone(), store.for_file(f)?));
    }
    for w in &store.warnings {
        eprintln!("Warning: {w}");
    }
    let only = &cli.opts.only;
    let except = &cli.except;
    let selected = |cop: &str| (only.is_empty() || only.iter().any(|o| o == cop)) && !except.iter().any(|e| e == cop);
    let workers = thread::available_parallelism().map_or(4, |n| n.get());
    let chunk = jobs.len().div_ceil(workers).max(1);
    let stdin_text = &stdin_text;
    let reports: Vec<Option<FileReport>> = thread::scope(|s| {
        let handles: Vec<_> = jobs
            .chunks(chunk)
            .map(|part| {
                s.spawn(|| {
                    part.iter()
                        .map(|(path, cfg)| {
                            // -a/-A: read, correct until stable, rewrite the file; offenses from every
                            // iteration come back (fixed ones marked corrected).
                            if let (Some(mode), None) = (cli.autocorrect, stdin_text) {
                                if !path.is_file() {
                                    return None;
                                }
                                let display = rbcop::config::relative_path(path, &cwd);
                                return Some(rbcop::autocorrect::correct_file(path, &display, cfg, &selected, mode));
                            }
                            let text = match stdin_text {
                                Some(t) => t.clone(),
                                None => match fs::read(path) {
                                    Ok(b) => String::from_utf8_lossy(&b).into_owned(),
                                    Err(_) => return None,
                                },
                            };
                            let display = rbcop::config::relative_path(path, &cwd);
                            Some(runner::inspect(path, &display, text, cfg, &selected))
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles.into_iter().flat_map(|h| h.join().unwrap()).collect()
    });

    // Deliberate difference: RuboCop truncates an -o file before inspecting, so `rubocop -o a.rb a.rb`
    // empties a.rb. rbcop refuses to write output over a file it inspects.
    for (_, out) in &resolved {
        if let Some(p) = out {
            let abs = expand_path(p, &cwd);
            let canon = abs.canonicalize().ok();
            if files.iter().any(|f| *f == abs || (canon.is_some() && f.canonicalize().ok() == canon)) {
                return Err(format!("rbcop: refusing to write --out {}: it is a file being inspected", p.display()));
            }
        }
    }

    // Runner#inspect_files
    let mut set = formatter::Set::new(&resolved, &cx, cli.stderr)?;
    let mut error = set.started(&cx, &files).err().map(|e| format!("{e}\n"));
    let min_fail = formatter::minimum_to_fail(cli.fail_level, &pwd_config);
    let considered_failure = |o: &formatter::Off| {
        !o.status.disabled()
            && ((cli.fail_level == Some(FailLevel::Autocorrect) && o.status.correctable())
                || (!o.status.corrected() && o.severity >= min_fail))
    };
    let displayed = |o: &formatter::Off| {
        if cli.only_fail_level {
            considered_failure(o)
        } else if cli.only_safe_correctable {
            o.status.correctable() && cx.supports_autocorrect(o.cop) && cx.safe_autocorrect(o.cop)
        } else if cli.only_correctable {
            o.status.correctable()
        } else {
            true
        }
    };
    let mut annotate = cli.annotate;
    annotate.json = cli.format.as_deref() == Some("json");
    let mut all_passed = true;
    let mut inspected: Vec<PathBuf> = Vec::new();
    if error.is_none() {
        for ((path, cfg), report) in jobs.iter().zip(&reports) {
            set.file_started(&cx, path);
            let Some(report) = report else {
                error = Some(format!("Error: No such file or directory: {}", path.display()));
                break;
            };
            let offs = formatter::offenses(&report.text, &report.offenses, |o| {
                formatter::annotate(cfg, o.cop, &o.message, annotate)
            });
            let passed = !offs.iter().any(|o| considered_failure(o) && displayed(o));
            all_passed &= passed;
            inspected.push(path.clone());
            let shown: Vec<_> = offs.into_iter().filter(|o| displayed(o)).collect();
            set.file_finished(&cx, &FileOut { path, offenses: shown, config: cfg });
            if cli.fail_fast && !passed {
                break;
            }
        }
    }
    set.finished(&cx, &inspected);
    // ExecuteRunner#display_error_summary; any cop error makes the run fail (runner_status).
    let cop_errors = runner::take_errors();
    if !cop_errors.is_empty() {
        let n = cop_errors.len();
        eprintln!("{}", red(global, &format!("\n{n} error{} occurred:", if n == 1 { "" } else { "s" })));
        for e in &cop_errors {
            eprintln!("{}", red(global, e));
        }
        eprintln!(
            "Errors are usually caused by rbcop bugs.\nPlease report a bug with the file and configuration that \
             triggered it.\nMention the following information in the issue report:\nrbcop {}",
            env!("CARGO_PKG_VERSION")
        );
    }
    if let Some(e) = error {
        eprintln!("{}", red(global, e.trim_end()));
        return Ok(ERROR);
    }
    Ok(if all_passed && cop_errors.is_empty() { 0 } else { OFFENSES })
}
