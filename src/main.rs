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

mod fallback;

// musl's malloc is a bottleneck for the worker threads; glibc and macOS keep the system allocator.
#[cfg(target_env = "musl")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// `rubocop -h` (OptionParser#help for RuboCop 1.91.0's options), recorded verbatim.
const HELP: &str = include_str!("help.txt");

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
    ("lint", Some('l'), Arg::None, Use::Handle),
    ("fix-layout", Some('x'), Arg::None, Use::Handle),
    ("safe", None, Arg::None, Use::Handle),
    ("except", None, Arg::Optional, Use::Handle),
    ("only", None, Arg::Optional, Use::Handle),
    ("only-guide-cops", None, Arg::None, Use::Handle),
    ("fail-fast", Some('F'), Arg::None, Use::Handle),
    ("disable-pending-cops", None, Arg::None, Use::Handle),
    ("enable-pending-cops", None, Arg::None, Use::Handle),
    ("disable-all-cops", None, Arg::None, Use::Handle),
    ("enable-all-cops", None, Arg::None, Use::Handle),
    ("[no-]preview", None, Arg::None, Use::Handle),
    ("ignore-disable-comments", None, Arg::None, Use::Handle),
    ("force-exclusion", None, Arg::None, Use::Handle),
    ("only-recognized-file-types", None, Arg::None, Use::Handle),
    ("ignore-parent-exclusion", None, Arg::None, Use::Handle),
    ("ignore-unrecognized-cops", None, Arg::None, Use::Handle),
    ("force-default-config", None, Arg::None, Use::Handle),
    ("changed", None, Arg::Optional, Use::Unsupported),
    ("stdin", Some('s'), Arg::Required, Use::Handle),
    ("editor-mode", None, Arg::None, Use::Handle),
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
    ("auto-correct", None, Arg::None, Use::Handle),
    ("safe-auto-correct", None, Arg::None, Use::Handle),
    ("autocorrect-all", Some('A'), Arg::None, Use::Handle),
    ("auto-correct-all", None, Arg::None, Use::Handle),
    ("disable-uncorrectable", None, Arg::None, Use::Unsupported),
    ("diff", None, Arg::None, Use::Unsupported),
    ("auto-gen-config", None, Arg::None, Use::Unsupported),
    ("regenerate-todo", None, Arg::None, Use::Unsupported),
    ("report-unused-todo-entries", None, Arg::None, Use::Unsupported),
    ("exclude-limit", None, Arg::Required, Use::Handle),
    ("no-exclude-limit", None, Arg::None, Use::Handle),
    ("[no-]offense-counts", None, Arg::None, Use::Handle),
    ("[no-]auto-gen-only-exclude", None, Arg::None, Use::Handle),
    ("[no-]auto-gen-timestamp", None, Arg::None, Use::Handle),
    ("[no-]auto-gen-enforced-style", None, Arg::None, Use::Handle),
    ("list-target-files", Some('L'), Arg::None, Use::Handle),
    ("list-enabled-cops-for", None, Arg::Required, Use::Handle),
    ("show-cops", None, Arg::Optional, Use::Handle),
    ("show-docs-url", None, Arg::Optional, Use::Handle),
    ("init", None, Arg::None, Use::Handle),
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
    /// `-h`: OptionParser prints the help and exits as soon as it reaches the option.
    Help,
}

#[derive(Default)]
struct Cli {
    opts: Options,
    /// -a / -A
    autocorrect: Option<rbcop::autocorrect::Mode>,
    /// `@options[:safe_autocorrect]` / `[:autocorrect_all]`: both may be given (and then refused).
    safe_autocorrect: bool,
    autocorrect_all: bool,
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
    /// `--list-enabled-cops-for PATH`
    list_enabled_for: Option<String>,
    /// `--show-docs-url [COPS]`
    show_docs_url: Option<Vec<String>>,
    /// `--only-recognized-file-types`
    only_recognized: bool,
    /// `--only-guide-cops`
    only_guide: bool,
    /// `--init`
    init: bool,
    /// `--enable-all-cops` / `--disable-all-cops` were given.
    enable_all: bool,
    disable_all: bool,
    /// Given without `--auto-gen-config` (validate_auto_gen_config, in its order): --exclude-limit,
    /// --[no-]offense-counts, --[no-]auto-gen-timestamp, --[no-]auto-gen-only-exclude.
    auto_gen: [bool; 4],
    version: bool,
    verbose_version: bool,
    /// EXITING_OPTIONS seen, in order (for "Incompatible cli options").
    exiting: Vec<&'static str>,
}

/// Shellwords.split; Err is its ArgumentError for an unterminated quote.
fn shellsplit(s: &str) -> Result<Vec<String>, String> {
    let unmatched = || format!("Unmatched quote: {s:?}");
    let mut out = Vec::new();
    let mut cur: Option<String> = None;
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match c {
            // Ruby's \s
            ' ' | '\t' | '\n' | '\x0B' | '\x0C' | '\r' => out.extend(cur.take()),
            '\'' => {
                let w = cur.get_or_insert_with(String::new);
                loop {
                    match chars.next().ok_or_else(unmatched)? {
                        '\'' => break,
                        c => w.push(c),
                    }
                }
            }
            '"' => {
                let w = cur.get_or_insert_with(String::new);
                loop {
                    match chars.next().ok_or_else(unmatched)? {
                        '"' => break,
                        // Only $ ` " \ and newline are escaped inside double quotes.
                        '\\' => match chars.next().ok_or_else(unmatched)? {
                            c @ ('$' | '`' | '"' | '\\' | '\n') => w.push(c),
                            c => {
                                w.push('\\');
                                w.push(c);
                            }
                        },
                        c => w.push(c),
                    }
                }
            }
            '\\' => {
                let w = cur.get_or_insert_with(String::new);
                w.push(chars.next().unwrap_or('\\'));
            }
            c => cur.get_or_insert_with(String::new).push(c),
        }
    }
    out.extend(cur);
    Ok(out)
}

/// ArgumentsFile (`.rubocop` in the working directory) + ArgumentsEnv (`RUBOCOP_OPTS`) + argv.
///
/// Deliberate difference: a checked-out project controls `.rubocop`, so `-o/--out` there is refused
/// rather than letting a repository choose which files rbcop writes.
fn all_args() -> Result<Vec<String>, Fail> {
    let mut args = Vec::new();
    if Path::new(".rubocop").is_file() {
        let text = fs::read_to_string(".rubocop").map_err(|e| Fail::Arg(format!(".rubocop: {e}")))?;
        let from_file = shellsplit(&text).map_err(Fail::Arg)?;
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
    args.extend(shellsplit(&env::var("RUBOCOP_OPTS").unwrap_or_default()).map_err(Fail::Arg)?);
    // env::args panics on an argument that is not UTF-8 (a file name on Linux, say).
    args.extend(env::args_os().skip(1).map(|a| a.to_string_lossy().into_owned()));
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
            let inline_given = inline.is_some();
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
            // validate_exclude_limit_option raises OptionParser::MissingArgument, which OptionParser
            // words with the option as written (and `=VALUE` when given inline).
            if name == "exclude-limit"
                && !value
                    .as_deref()
                    .is_some_and(|v| v.split('\n').any(|l| !l.is_empty() && l.bytes().all(|b| b.is_ascii_digit())))
            {
                let shown = if inline_given { format!("{written}={}", value.unwrap_or_default()) } else { written };
                return Err(Fail::Parse(format!("missing argument: {shown}")));
            }
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

/// `@options[:only] ||= []; @options[:only] << dept` (-l, -x): no Lint/Syntax is added.
fn push_only(cli: &mut Cli, dept: &str) {
    let only = cli.only.get_or_insert_with(Vec::new);
    only.push(dept.to_string());
    cli.opts.only = only.clone();
}

fn apply(cli: &mut Cli, name: &str, neg: bool, value: Option<String>) -> Result<(), Fail> {
    match name {
        "lint" => push_only(cli, "Lint"),
        "fix-layout" => {
            // `@options[:autocorrect] = true` without `:safe_autocorrect`: all corrections, unless
            // -a asks for safe ones.
            push_only(cli, "Layout");
            cli.autocorrect.get_or_insert(rbcop::autocorrect::Mode::All);
        }
        "safe" => cli.opts.safe = true,
        "enable-all-cops" => cli.enable_all = true,
        "disable-all-cops" => cli.disable_all = true,
        "ignore-disable-comments" => rbcop::directives::ignore_disable_comments(),
        "only-recognized-file-types" => cli.only_recognized = true,
        "only-guide-cops" => cli.only_guide = true,
        "init" => cli.init = true,
        "exclude-limit" => cli.auto_gen[0] = true,
        "offense-counts" => cli.auto_gen[1] = true,
        "auto-gen-timestamp" => cli.auto_gen[2] = true,
        "auto-gen-only-exclude" => cli.auto_gen[3] = true,
        "list-enabled-cops-for" => {
            cli.list_enabled_for = value;
            cli.exiting.push("list_enabled_cops_for");
        }
        "show-docs-url" => {
            // Ruby's split(',') drops trailing empty fields; only emptiness matters here.
            cli.show_docs_url = Some(
                value.map(|v| v.split(',').filter(|c| !c.is_empty()).map(String::from).collect()).unwrap_or_default(),
            );
            cli.exiting.push("show_docs_url");
        }
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
        "ignore-unrecognized-cops" => cli.opts.ignore_unrecognized_cops = true,
        "enable-pending-cops" => cli.opts.enable_pending_cops = true,
        "disable-pending-cops" => cli.opts.disable_pending_cops = true,
        "preview" => cli.opts.preview = Some(!neg),
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
        "editor-mode" => rbcop::helpers::lsp::enable(),
        "display-time" => cli.display_time = true,
        "auto-correct" | "safe-auto-correct" | "auto-correct-all" => {
            // Options#handle_deprecated_option, warned while parsing (only a literal --no-color
            // on the command line turns its colour off).
            let new = if name == "auto-correct-all" { "--autocorrect-all" } else { "--autocorrect" };
            let color = global_color(None) && !env::args().any(|a| a == "--no-color");
            eprintln!("{}", formatter::Paint(color).yellow(&format!("--{name} is deprecated; use {new} instead.")));
            return apply(cli, &new[2..], neg, value);
        }
        "autocorrect" => {
            cli.autocorrect = Some(rbcop::autocorrect::Mode::Safe);
            cli.safe_autocorrect = true;
        }
        "autocorrect-all" => {
            cli.autocorrect = Some(rbcop::autocorrect::Mode::All);
            cli.autocorrect_all = true;
        }
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
        "help" => return Err(Fail::Help),
        _ => {} // ignored options
    }
    Ok(())
}

/// OptionsValidator#validate_compatibility (the checks that apply to what rbcop supports).
fn validate(cli: &mut Cli, global: bool) -> Result<(), Fail> {
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
    const AUTO_GEN: [&str; 4] = ["exclude-limit", "offense-counts", "auto-gen-timestamp", "auto-gen-only-exclude"];
    if let Some(i) = cli.auto_gen.iter().position(|&given| given) {
        return Err(Fail::Arg(format!("--{} can only be used together with --auto-gen-config.", AUTO_GEN[i])));
    }
    if cli.safe_autocorrect && cli.autocorrect_all {
        // Rainbow(<<~MESSAGE).red, then warned red again: the trailing newline is part of the message.
        let m = "Error: Both safe and unsafe autocorrect options are specified, use only one.\n";
        return Err(Fail::Arg(red(global, m)));
    }
    if cli.only_failed && cli.format.as_deref() != Some("junit") {
        return Err(Fail::Arg("--display-only-failed can only be used together with --format junit.".into()));
    }
    if cli.only_failed && (cli.only_correctable || cli.only_safe_correctable) {
        return Err(Fail::Arg("--display-only-failed cannot be used together with other display options.".into()));
    }
    if cli.autocorrect.is_some() && (cli.only_correctable || cli.only_safe_correctable) {
        return Err(Fail::Arg("--autocorrect cannot be used with --display-only-[safe-]correctable.".into()));
    }
    if cli.enable_all && cli.disable_all {
        return Err(Fail::Arg("--enable-all-cops cannot be used together with --disable-all-cops.".into()));
    }
    if cli.enable_all || cli.disable_all {
        cli.opts.all_cops = Some(cli.enable_all);
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

/// `warn Rainbow(s).red`: Kernel#warn adds a newline unless the painted text ends with one.
fn warn_red(on: bool, s: &str) {
    let s = red(on, s);
    eprint!("{s}{}", if s.ends_with('\n') { "" } else { "\n" });
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
    // Option errors are raised inside Options#parse, before --color/--no-color reach Rainbow.
    let parse_color = global_color(None);
    let status = match parsed.and_then(|mut cli| validate(&mut cli, parse_color).map(|_| cli)) {
        Err(Fail::Parse(m)) => {
            eprintln!("{m}\nFor usage information, use --help");
            ERROR
        }
        Err(Fail::Arg(m)) => {
            // Kernel#warn: no newline added after one the message already ends with.
            let m = red(parse_color, &m);
            eprint!("{m}{}", if m.ends_with('\n') { "" } else { "\n" });
            ERROR
        }
        Err(Fail::Unsupported(m)) => {
            eprintln!("rbcop: {m}");
            ERROR
        }
        Err(Fail::Help) => {
            print_help(parse_color && !env::args().any(|a| a == "--no-color"));
            0
        }
        // CLI#run: --init runs before the configuration is loaded.
        Ok(cli) if cli.init => init_dotfile(parse_color),
        Ok(cli) => match run(cli, global, parse_color) {
            Ok(code) => code,
            Err(e) => {
                warn_red(global, &e);
                ERROR
            }
        },
    };
    if display_time {
        println!("Finished in {} seconds", ruby_seconds(t0.elapsed().as_secs_f64()));
    }
    ExitCode::from(status)
}

/// `--init`: CLI::Command::InitDotfile.
fn init_dotfile(color: bool) -> u8 {
    const DOTFILE: &str = ".rubocop.yml";
    const TEMPLATE: &str = "# The behavior of RuboCop can be controlled via the .rubocop.yml
# configuration file. It makes it possible to enable/disable
# certain cops (checks) and to alter their behavior if they accept
# any parameters. The file can be placed either in your home
# directory or in some project directory.
#
# RuboCop will start looking for the configuration file in the directory
# where the inspected file is and continue its way up to the root directory.
#
# See https://docs.rubocop.org/rubocop/configuration
";
    // File.expand_path: Dir.pwd, the real working directory.
    let path = env::current_dir().map(|d| d.join(DOTFILE).display().to_string()).unwrap_or(DOTFILE.into());
    if Path::new(DOTFILE).exists() {
        warn_red(color, &format!("{DOTFILE} already exists at {path}"));
        return ERROR;
    }
    if let Err(e) = fs::write(DOTFILE, TEMPLATE) {
        warn_red(color, &runner::io_error(&e, Path::new(DOTFILE)));
        return ERROR;
    }
    println!("Writing new {DOTFILE} to {path}");
    0
}

/// `puts opts`: the banner and section headings are bright when Options#rainbow is on.
fn print_help(color: bool) {
    let p = formatter::Paint(color);
    let mut out = String::new();
    for (i, line) in HELP.lines().enumerate() {
        match line.strip_suffix(':') {
            _ if i == 0 => out.push_str(&p.wrap(line, "1")),
            Some(heading) if !line.starts_with(' ') => {
                out.push_str(&p.wrap(heading, "1"));
                out.push(':');
            }
            _ => out.push_str(line),
        }
        out.push('\n');
    }
    print!("{out}");
}

/// `--list-enabled-cops-for PATH`: CLI::Command::ListEnabledCopsFor, every registered cop whose
/// `for_cop(name)['Enabled']` is truthy in PATH's config, by department then name.
fn list_enabled_cops_for(cfg: &Resolved) {
    use rbcop::config::value::Value;
    let mut names: Vec<&str> = rbcop::helpers::cop_registry::REGISTRY.to_vec();
    names.sort_unstable();
    let mut out = String::new();
    for name in names {
        if !matches!(cfg.enable_cop(name), Value::Null | Value::Bool(false)) {
            out.push_str(name);
            out.push('\n');
        }
    }
    print!("{out}");
}

/// `--show-docs-url [COPS]`: CLI::Command::ShowDocsUrl.
fn show_docs_url(cops: &[String], cfg: &Resolved) {
    let mut out = String::new();
    if cops.is_empty() {
        out.push_str("https://docs.rubocop.org/rubocop\n");
    }
    for cop in cops {
        if let Some(name) = rbcop::helpers::cop_registry::REGISTRY.iter().find(|c| **c == cop.as_str()) {
            out.push_str(&formatter::docs_url(cfg, name));
            out.push('\n');
        }
    }
    out.push('\n');
    print!("{out}");
}

fn ruby_version_string(v: f64) -> String {
    let s = format!("{v}");
    if s.contains('.') { s } else { format!("{s}.0") }
}

/// `--show-cops`: CLI::Command::ShowCops for the cops rbcop implements.
fn show_cops(patterns: &[String], cfg: &Resolved, pwd: &str) {
    let mut names: Vec<&'static str> = cops::ALL.iter().map(|r| r.name).collect();
    // Lint/Syntax lives in the runner, not in cops::ALL, but RuboCop's registry lists it.
    names.push("Lint/Syntax");
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
            // Config#for_cop: the cop's own hash with Enabled from Config#enable_cop? (raw `pending` stays).
            let mut map = cfg.hash.map(name).cloned().unwrap_or_default();
            let enabled = cfg.enable_cop(name);
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

/// CLI#report_pending_cops (PendingCopsReporter): on stderr, before any formatter output.
fn report_pending_cops(cfg: &Resolved, global: bool) {
    const BANNER: &str = "The following cops were added to RuboCop, but are not configured. Please set Enabled to \
                          either `true` or `false` in your `.rubocop.yml` file.\n\nPlease also note that you can \
                          opt-in to new cops by default by adding this to your config:\n  AllCops:\n    NewCops: \
                          enable\n";
    let pending = cfg.pending_cops();
    if pending.is_empty() {
        return;
    }
    let p = formatter::Paint(global);
    // Kernel#warn adds a newline unless the (painted) text already ends with one.
    let warn = |s: String| if s.ends_with('\n') { s } else { s + "\n" };
    let mut out = warn(p.yellow(BANNER));
    for (name, version) in pending {
        out.push_str(&warn(p.yellow(&format!("{name}: # new in {version}"))));
        out.push_str(&warn(p.yellow("  Enabled: true")));
    }
    out.push_str(&warn(p.yellow("For more information: https://docs.rubocop.org/rubocop/versioning.html")));
    eprint!("{out}");
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

/// Badge.camel_case
fn badge_camel_case(part: &str) -> String {
    if part == "rspec" {
        return "RSpec".into();
    }
    // gsub(/^[a-z]|_[a-z]/) { |m| m[-1].upcase }
    let chars: Vec<char> = part.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if i == 0 && c.is_ascii_lowercase() {
            out.push(c.to_ascii_uppercase());
        } else if c == '_' && chars.get(i + 1).is_some_and(|n| n.is_ascii_lowercase()) {
            out.push(chars[i + 1].to_ascii_uppercase());
            i += 1;
        } else {
            out.push(c);
        }
        i += 1;
    }
    out
}

/// Registry#qualified_cop_name(name, origin) with its warnings; Err is AmbiguousCopName.
fn qualify_option_name(name: &str, origin: &str, warnings: &mut Vec<String>) -> Result<String, String> {
    use rbcop::helpers::cop_registry::REGISTRY;
    // Badge.parse: Ruby's split drops trailing empty fields.
    let mut parts: Vec<String> = name.split('/').map(badge_camel_case).collect();
    while parts.len() > 1 && parts.last().is_some_and(|p| p.is_empty()) {
        parts.pop();
    }
    let cop = parts.pop().unwrap_or_default();
    let dept = parts.join("/");
    let qualified = !parts.is_empty();
    let basename = |c: &str| c.rsplit('/').next().unwrap_or(c).to_string();
    if !qualified && REGISTRY.iter().any(|c| basename(c) == name) {
        warnings.push(format!("{origin}: Warning: no department given for {name}."));
    }
    if qualified && REGISTRY.contains(&format!("{dept}/{cop}").as_str()) {
        return Ok(name.to_string());
    }
    let mut depts: Vec<&str> = REGISTRY.iter().filter_map(|c| c.rsplit_once('/').map(|(d, _)| d)).collect();
    depts.dedup();
    let hits: Vec<String> =
        depts.into_iter().map(|d| format!("{d}/{cop}")).filter(|q| REGISTRY.contains(&q.as_str())).collect();
    let given = if qualified { format!("{dept}/{cop}") } else { cop.clone() };
    match hits.as_slice() {
        [] => Ok(name.to_string()),
        [real] => {
            if qualified && *real != given {
                warnings.push(format!("{origin}: Warning: {given} has the wrong namespace - replace it with {real}"));
            }
            Ok(real.clone())
        }
        many => Err(format!(
            "Error: Ambiguous cop name `{given}` used in {origin} needs department qualifier. Did you mean {}?",
            many.join(" or ")
        )),
    }
}

/// Runner#qualify_option_cop_names, then OptionsValidator.validate_cop_list. RuboCop does this when
/// it mobilizes the first team, so the warnings and the error surface at the first inspected file.
fn qualify_cop_options(cli: &mut Cli) -> (Vec<String>, Option<String>) {
    use rbcop::helpers::{cop_registry, did_you_mean};
    let mut warnings = Vec::new();
    for (opt, list) in [("only", cli.only.as_mut()), ("except", Some(&mut cli.except))] {
        let Some(list) = list else { continue };
        if opt == "except" && list.is_empty() {
            continue;
        }
        for n in list.iter_mut() {
            match qualify_option_name(n, &format!("--{opt} option"), &mut warnings) {
                Ok(q) => *n = q,
                Err(e) => return (warnings, Some(e)),
            }
        }
    }
    cli.opts.only = cli.only.clone().unwrap_or_default();
    for list in [cli.only.as_deref().unwrap_or_default(), &cli.except] {
        for n in list {
            let n = n.as_str();
            if cop_registry::REGISTRY.contains(&n)
                || cop_registry::is_department(n)
                || n == "Syntax"
                || n == "Lint/Syntax"
            {
                continue;
            }
            let names: Vec<&str> = cop_registry::REGISTRY.iter().copied().filter(|c| *c != n).collect();
            let similar = did_you_mean::correct(n, &names);
            let mut msg = format!("Unrecognized cop or department: {n}.");
            if !similar.is_empty() {
                msg.push_str(&format!("\nDid you mean? {}", similar.join(", ")));
            }
            return (warnings, Some(msg));
        }
    }
    (warnings, None)
}

/// One linted file: (report, corrected source to write back, infinite-loop error), or a read error.
/// One file's result: report, corrected source, loop error, and the warnings its cops raised.
type Linted = Result<(FileReport, Option<String>, Option<String>, Vec<String>), String>;

/// Whether two targets are the same file on disk (one of them through a symlink, say).
fn same_file_twice(jobs: &[(PathBuf, std::sync::Arc<Resolved>)]) -> bool {
    use std::os::unix::fs::MetadataExt;
    let mut seen = std::collections::HashSet::new();
    jobs.iter().filter_map(|(p, _)| fs::metadata(p).ok()).any(|m| !seen.insert((m.dev(), m.ino())))
}

/// Returns the exit status.
fn run(mut cli: Cli, global: bool, parse_color: bool) -> Result<u8, String> {
    let (cop_warnings, cop_error) = qualify_cop_options(&mut cli);
    // Lint/Syntax lives in the runner but is a registered cop with its own config in RuboCop.
    let names: Vec<&'static str> = cops::ALL.iter().map(|c| c.name).chain(["Lint/Syntax"]).collect();
    // ConfigLoader: a missing -c file is an error, even with --force-default-config.
    if let Some(c) = &cli.opts.config_file {
        let abs = expand_path(c, &rbcop::config::cwd());
        if !abs.is_file() {
            return Err(format!("Configuration file not found: {}", abs.display()));
        }
    }
    // CLI#act_on_options loads the -c and pwd configs before --color/--no-color reach Rainbow.
    cli.opts.color = parse_color;
    let loaded = Store::new(cli.opts.clone(), names.clone()).and_then(|mut store| {
        let cwd = store.cwd().to_path_buf();
        store.for_dir(&cwd).map(|c| (store, c))
    });
    let (mut store, pwd_config) = match loaded {
        Ok(x) => x,
        Err(e) => {
            warn_red(parse_color, &e);
            return Ok(ERROR);
        }
    };
    store.set_color(global);
    let cwd = store.cwd().to_path_buf();
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
    if let Some(path) = &cli.list_enabled_for {
        // ConfigStore#for: a directory's own config, else the file's directory's.
        let abs = expand_path(Path::new(path), &cwd);
        let dir = if abs.is_dir() { abs } else { abs.parent().map_or(cwd.clone(), Path::to_path_buf) };
        list_enabled_cops_for(&*store.for_dir(&dir)?);
        return Ok(0);
    }
    if let Some(cops) = &cli.show_docs_url {
        show_docs_url(cops, &pwd_config);
        return Ok(0);
    }
    if !cli.opts.disable_pending_cops && !cli.opts.enable_pending_cops {
        report_pending_cops(&pwd_config, global);
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
            Some(buf)
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
                let mut found = targets::find(&mut store, &one, cli.force_exclusion, cli.only_recognized)?;
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
            safe_autocorrect: cli.autocorrect == Some(rbcop::autocorrect::Mode::Safe),
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

    // Deliberate difference: RuboCop truncates an -o file before inspecting, so `rubocop -o a.rb a.rb`
    // empties a.rb. rbcop refuses to write output over a file it inspects (checked before -a changes
    // any file).
    for (_, out) in &resolved {
        if let Some(p) = out {
            let abs = expand_path(p, &cwd);
            let canon = abs.canonicalize().ok();
            if files.iter().any(|f| *f == abs || (canon.is_some() && f.canonicalize().ok() == canon)) {
                return Err(format!("rbcop: refusing to write --out {}: it is a file being inspected", p.display()));
            }
        }
    }

    let only = &cli.opts.only;
    let except = &cli.except;
    // Runner#mobilize_cop_badge?: Badge#match_name? takes cop names and department names.
    // Sets: every file asks about every cop, and `--only` may list all of them.
    let only_set: std::collections::HashSet<&str> = only.iter().map(String::as_str).collect();
    let except_set: std::collections::HashSet<&str> = except.iter().map(String::as_str).collect();
    let match_name = |list: &std::collections::HashSet<&str>, cop: &str| {
        list.contains(cop) || cop.rsplit_once('/').is_some_and(|(d, _)| list.contains(d))
    };
    let selected = |cop: &str| {
        // Runner#check_for_redundant_disables? never runs the directive cop under --only.
        // ponytail: so it also drops out of Registry#disabled_names there; matters only when the
        // config disables Lint/RedundantCopDisableDirective and --only names its department.
        if cop == "Lint/RedundantCopDisableDirective" && !only.is_empty() {
            return false;
        }
        (only.is_empty() || match_name(&only_set, cop)) && (except.is_empty() || !match_name(&except_set, cop))
    };
    // Runner#mobilize_cop_badge?: without --only, --only-guide-cops or `AllCops:
    // StyleGuideCopsOnly` keeps only cops whose own config has a StyleGuide.
    // Lint/RedundantCopDisableDirective is not a team cop: Runner#redundant_cop_disable_directive
    // runs it past this filter.
    // ponytail: it so also enters Registry#disabled_names under the filter, which matters only
    // for a config that disables it.
    let badge_selected = |cop: &str, cfg: &Resolved| {
        let guide_only = only.is_empty() && (cli.only_guide || cfg.all_cops().truthy("StyleGuideCopsOnly"));
        selected(cop)
            && (!guide_only
                || cfg.hash.map(cop).is_some_and(|c| c.truthy("StyleGuide"))
                || cop == "Lint/RedundantCopDisableDirective")
    };
    // Resolve configs up front (single-threaded), then lint in parallel.
    let mut jobs = Vec::with_capacity(files.len());
    // RuboCop loads a file's config when the file's turn comes, so an invalid config stops the run
    // there, after the files before it have been reported.
    let mut config_error = None;
    // Team#validate_config fails the same way, but later: when the file's team is mobilized, after
    // the --only/--except warnings and Config#for_cop's. (index in jobs, error)
    let mut team_error = None;
    for f in &files {
        match store.for_file(f) {
            Ok(c) => {
                let valid = rbcop::config::validator::cop_validate_config(&c, |cop| {
                    badge_selected(cop, &c) && c.cop_enabled(cop)
                });
                jobs.push((f.clone(), c));
                if let Err(e) = valid {
                    team_error = Some((jobs.len() - 1, e));
                    break;
                }
            }
            Err(e) => {
                config_error = Some((f.clone(), e));
                break;
            }
        }
    }
    for w in &store.warnings {
        eprintln!("Warning: {w}");
    }
    // Deliberate difference: cops rbcop cannot run are named on stderr, or run by RuboCop.
    let names: Vec<&'static str> = cops::ALL.iter().map(|c| c.name).chain(["Lint/Syntax"]).collect();
    let implemented: std::collections::HashSet<&str> = names.iter().copied().collect();
    let mut seen = std::collections::HashSet::new();
    let configs = jobs.iter().filter(|(_, c)| seen.insert(std::sync::Arc::as_ptr(c))).map(|(_, c)| &**c);
    let skipped = fallback::unimplemented(configs, &implemented, |cop, cfg| badge_selected(cop, cfg));
    let mut fallback_offenses = std::collections::HashMap::new();
    if !skipped.is_empty() || store.requires_ruby_code() {
        if fallback::enabled() && stdin_text.is_none() && config_error.is_none() && team_error.is_none() {
            let files: Vec<PathBuf> = jobs.iter().map(|(p, _)| p.clone()).collect();
            let child = fallback::Child {
                opts: &cli.opts,
                except,
                only_guide: cli.only_guide,
                force_exclusion: cli.force_exclusion,
                autocorrect: cli.autocorrect,
            };
            fallback_offenses = fallback::run(&files, &cwd, &names, child)?;
        } else {
            eprintln!("{}", fallback::notice(&skipped, store.requires_ruby_code()));
        }
    }
    let stdin_text = &stdin_text;
    // --stdin with -a/-A: the corrected source, printed after the report.
    let mut stdin_corrected: Option<String> = None;
    let write_roots = rbcop::autocorrect::write_roots(&cwd, &cli.paths);
    // One file: (report, corrected source to write back, infinite-loop error), or a read error.
    // -a/-A: read, correct until stable; offenses from every iteration come back (fixed ones marked
    // corrected). The file is written in the reporting loop below, in file order, as RuboCop does.
    let lint = |path: &PathBuf,
                cfg: &Resolved,
                mode: Option<rbcop::autocorrect::Mode>|
     -> Result<(FileReport, Option<String>, Option<String>), String> {
        let display = rbcop::config::relative_path(path, &cwd);
        let selected = |cop: &str| badge_selected(cop, cfg);
        if let Some(bytes) = stdin_text {
            // --stdin with -a/-A: the corrected source is printed after the report (@options[:stdin]).
            if let Some(mode) = mode {
                let c = rbcop::autocorrect::correct_bytes(path, &display, bytes.clone(), cfg, &selected, mode);
                return Ok((c.report, c.source, c.error));
            }
            return Ok((runner::inspect_bytes(path, &display, bytes.clone(), cfg, &selected), None, None));
        }
        if let Some(mode) = mode {
            let c = rbcop::autocorrect::correct_file(path, &display, cfg, &selected, mode)?;
            return Ok((c.report, c.source, c.error));
        }
        let bytes = fs::read(path).map_err(|e| runner::io_error(&e, path))?;
        Ok((runner::inspect_bytes(path, &display, bytes, cfg, &selected), None, None))
    };
    // A panic outside a cop (cop panics are reported by runner::guarded) ends the run at that file,
    // like any error, instead of taking down every worker's results with it.
    let lint_one = |path: &PathBuf, cfg: &Resolved, mode: Option<rbcop::autocorrect::Mode>| -> Linted {
        rbcop::config::value::reset_run_budget();
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| lint(path, cfg, mode)))
            .unwrap_or_else(|_| Err(format!("rbcop: internal error while inspecting {}", path.display())))
            .map(|(report, corrected, loop_error)| (report, corrected, loop_error, runner::take_file_warnings()))
    };
    // Cops recurse over the AST; runner::MAX_AST_DEPTH (1000) needs under 2 MiB, the rest is
    // headroom. Only touched pages are committed.
    let big_stack = || thread::Builder::new().stack_size(64 << 20);
    // Files are linted in parallel up front, unless RuboCop's sequential order is observable:
    // --fail-fast stops at the first failing file (later files are neither inspected nor corrected),
    // and under -a a file reached through two paths (a symlink) must see the first correction.
    // An invalid --only/--except list fails before any file is inspected (or corrected).
    // A failing validate_config stops the run before its file is inspected (lazily, so no cop
    // error of that file is printed).
    let lazy = cop_error.is_some()
        || team_error.is_some()
        || cli.fail_fast
        || (cli.autocorrect.is_some() && same_file_twice(&jobs));
    let mut reports: Vec<Option<Linted>> = if lazy {
        jobs.iter().map(|_| None).collect()
    } else {
        let workers = thread::available_parallelism().map_or(4, |n| n.get()).clamp(1, jobs.len().max(1));
        // Each worker takes the next file, largest first, so no core idles while another finishes
        // a long run of big files (fixed chunks of the sorted list did). Results keep file order.
        let mut order: Vec<usize> = (0..jobs.len()).collect();
        order.sort_by_cached_key(|&i| std::cmp::Reverse(fs::metadata(&jobs[i].0).map_or(0, |m| m.len())));
        let next = std::sync::atomic::AtomicUsize::new(0);
        let mut reports: Vec<Option<Linted>> = jobs.iter().map(|_| None).collect();
        thread::scope(|s| {
            let handles: Vec<_> = (0..workers)
                .map(|_| {
                    big_stack()
                        .spawn_scoped(s, || {
                            let mut done = Vec::new();
                            while let Some(&i) = order.get(next.fetch_add(1, std::sync::atomic::Ordering::Relaxed)) {
                                let (path, cfg) = &jobs[i];
                                done.push((i, lint_one(path, cfg, cli.autocorrect)));
                            }
                            done
                        })
                        .expect("spawn worker thread")
                })
                .collect();
            for h in handles {
                for (i, linted) in h.join().unwrap() {
                    reports[i] = Some(linted);
                }
            }
        });
        reports
    };

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
    let mut config_failure = None;
    let mut cop_warnings_raised: Vec<String> = Vec::new();
    if error.is_none() {
        let mut cop_notes = Some((cop_warnings, cop_error));
        let mut stopped = false;
        let mut warned_configs = std::collections::HashSet::new();
        for (idx, ((path, cfg), linted)) in jobs.iter().zip(reports.iter_mut()).enumerate() {
            set.file_started(&cx, path);
            // Runner#mobilized_cop_classes runs once the first file has been read.
            if let Some((warnings, err)) = cop_notes.take_if(|_| stdin_text.is_some() || path.is_file()) {
                for w in warnings {
                    eprintln!("{w}");
                }
                if let Some(e) = err {
                    error = Some(e);
                    stopped = true;
                    break;
                }
            }
            // Config#for_cop warns about deprecated-name configuration once per config, when the
            // cops are mobilized for its first file.
            if (stdin_text.is_some() || path.is_file()) && warned_configs.insert(std::sync::Arc::as_ptr(cfg)) {
                for (cop, msg) in &cfg.deprecated_warnings {
                    if selected(cop) {
                        let msg = formatter::Paint(global).yellow(msg);
                        eprint!("{msg}{}", if msg.ends_with('\n') { "" } else { "\n" });
                    }
                }
            }
            if let Some((_, e)) = team_error.take_if(|(i, _)| *i == idx) {
                config_failure = Some(e);
                stopped = true;
                break;
            }
            let linted = match linted.take() {
                Some(l) => l,
                None => thread::scope(|s| {
                    big_stack().spawn_scoped(s, || lint_one(path, cfg, cli.autocorrect)).unwrap().join().unwrap()
                }),
            };
            // Deliberate difference: under -a/-A a file that resolves outside the project (the
            // working directory and the command-line paths) is not written; it is reported as
            // without -a/-A. write_source checks again right before writing.
            let refused = match &linted {
                Ok((_, Some(_), _, _)) if stdin_text.is_none() => {
                    fs::canonicalize(path).ok().filter(|real| rbcop::autocorrect::outside_roots(real, &write_roots))
                }
                _ => None,
            };
            let linted = match refused {
                Some(real) => {
                    let display = rbcop::config::relative_path(path, &cwd);
                    eprintln!(
                        "rbcop: not autocorrecting {display}: it resolves to {}, outside the project",
                        real.display()
                    );
                    thread::scope(|s| {
                        big_stack().spawn_scoped(s, || lint_one(path, cfg, None)).unwrap().join().unwrap()
                    })
                }
                None => linted,
            };
            let (mut report, corrected, loop_error, warnings) = match linted {
                Ok(l) => l,
                Err(e) => {
                    error = Some(e);
                    stopped = true;
                    break;
                }
            };
            // Registry#print_department_missing_warning, written while the file was inspected.
            for w in &report.warnings {
                eprintln!("{w}");
            }
            // Team#handle_warning prints each cop warning as the file is inspected; Runner#warnings
            // keeps them for the summary, in the same order.
            for w in &warnings {
                eprintln!("{w}");
            }
            cop_warnings_raised.extend(warnings);
            // Team#autocorrect writes the file before it is reported (with --stdin, keeps the source
            // for printing); a failed write ends the run.
            if stdin_text.is_some() {
                stdin_corrected = corrected;
            } else if let Some(new) = &corrected
                && let Err(e) = rbcop::autocorrect::write_source(path, new, &write_roots)
            {
                error = Some(runner::io_error(&e, path));
                stopped = true;
                break;
            }
            // Runner#process_file: `errors << e`, so the summary lists it and the run fails.
            if let Some(e) = loop_error {
                runner::file_error(e);
            }
            report.offenses.extend(fallback_offenses.remove(path).unwrap_or_default());
            let offs = formatter::offenses(&report.text, &report.offenses, |o| {
                formatter::annotate(cfg, o.cop, &o.message, annotate)
            });
            let passed = !offs.iter().any(|o| considered_failure(o) && displayed(o));
            all_passed &= passed;
            inspected.push(path.clone());
            let shown: Vec<_> = offs.into_iter().filter(|o| displayed(o)).collect();
            set.file_finished(&cx, &FileOut { path, offenses: shown, config: cfg });
            if cli.fail_fast && !passed {
                stopped = true;
                break;
            }
            // A formatter's output could not be written: RuboCop raises there and stops.
            if let Some(e) = set.take_error() {
                error = Some(e);
                stopped = true;
                break;
            }
        }
        if !stopped && let Some((path, e)) = config_error {
            set.file_started(&cx, &path);
            config_failure = Some(e);
        }
    }
    set.finished(&cx, &inspected);
    if let Some(e) = set.take_error() {
        error.get_or_insert(e);
    }
    // The ValidationError reaches the CLI before ExecuteRunner's error summary.
    if let Some(e) = config_failure {
        warn_red(global, &e);
        return Ok(ERROR);
    }
    // ExecuteRunner#display_warning_summary, before the error summary.
    if !cop_warnings_raised.is_empty() {
        let n = cop_warnings_raised.len();
        let paint = formatter::Paint(global);
        eprintln!("{}", paint.yellow(&format!("\n{n} warning{}:", if n == 1 { "" } else { "s" })));
        for w in &cop_warnings_raised {
            eprintln!("{}", paint.yellow(w));
        }
    }
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
    // ExecuteRunner#maybe_print_corrected_source: the separator follows --stderr, the source is
    // printed to stdout either way; integration formatters get neither.
    if let (Some(_), Some(original)) = (cli.autocorrect, stdin_text)
        && !matches!(cli.format.as_deref(), Some("h" | "html" | "j" | "json" | "ju" | "junit"))
    {
        let separator = "=".repeat(20);
        if cli.stderr {
            eprintln!("{separator}");
        } else {
            println!("{separator}");
        }
        // Uncorrected: the original bytes as given (a non-UTF-8 source is never corrected).
        match stdin_corrected.as_deref() {
            Some(corrected) => print!("{corrected}"),
            None => std::io::Write::write_all(&mut std::io::stdout(), original).map_err(|e| e.to_string())?,
        }
    }
    Ok(if all_passed && cop_errors.is_empty() { 0 } else { OFFENSES })
}
