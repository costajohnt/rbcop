//! The `rubocop` command line: options, formatters and exit status as RuboCop 1.92.1 has them
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

mod cache;
mod explain;
mod fallback;
mod lsp;
mod mcp;
mod serve;

// musl's malloc is a bottleneck for the worker threads; glibc and macOS keep the system allocator.
#[cfg(target_env = "musl")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// `rubocop -h` (OptionParser#help for RuboCop 1.92.1's options), recorded verbatim.
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

/// Every option RuboCop 1.92.1 defines: (long name, short, argument, what rbcop does with it).
/// Long names with `[no-]` also accept `--no-NAME`.
#[derive(Clone, Copy, PartialEq)]
enum Use {
    Handle,
    Ignore,
    /// Loads Ruby code: the whole run goes to RuboCop (Fail::Ruby).
    Ruby,
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
    ("changed", None, Arg::Optional, Use::Handle),
    ("stdin", Some('s'), Arg::Required, Use::Handle),
    ("editor-mode", None, Arg::None, Use::Handle),
    ("[no-]parallel", Some('P'), Arg::None, Use::Handle),
    ("raise-cop-error", None, Arg::None, Use::Ignore),
    ("fail-level", None, Arg::Required, Use::Handle),
    ("cache", Some('C'), Arg::Required, Use::Handle),
    ("cache-root", None, Arg::Required, Use::Handle),
    ("lsp", None, Arg::None, Use::Handle),
    ("mcp", None, Arg::None, Use::Handle),
    ("[no-]server", None, Arg::None, Use::Handle),
    ("restart-server", None, Arg::None, Use::Ignore),
    ("start-server", None, Arg::None, Use::Ignore),
    ("stop-server", None, Arg::None, Use::Ignore),
    ("server-status", None, Arg::None, Use::Ignore),
    ("no-detach", None, Arg::None, Use::Ignore),
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
    ("display-suppressed", None, Arg::None, Use::Handle),
    ("max-offenses-per-cop", None, Arg::Required, Use::Handle),
    ("autocorrect", Some('a'), Arg::None, Use::Handle),
    ("auto-correct", None, Arg::None, Use::Handle),
    ("safe-auto-correct", None, Arg::None, Use::Handle),
    ("autocorrect-all", Some('A'), Arg::None, Use::Handle),
    ("auto-correct-all", None, Arg::None, Use::Handle),
    ("disable-uncorrectable", None, Arg::None, Use::Handle),
    ("diff", None, Arg::None, Use::Handle),
    ("auto-gen-config", None, Arg::None, Use::Handle),
    ("regenerate-todo", None, Arg::None, Use::Handle),
    ("report-unused-todo-entries", None, Arg::None, Use::Handle),
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
    ("explain", None, Arg::Required, Use::Handle),
    ("init", None, Arg::None, Use::Handle),
    ("config", Some('c'), Arg::Required, Use::Handle),
    ("debug", Some('d'), Arg::None, Use::Handle),
    ("plugin", None, Arg::Required, Use::Ruby),
    ("require", Some('r'), Arg::Required, Use::Ruby),
    ("[no-]color", None, Arg::None, Use::Handle),
    ("version", Some('v'), Arg::None, Use::Handle),
    ("verbose-version", Some('V'), Arg::None, Use::Handle),
    ("profile", None, Arg::None, Use::Handle),
    ("memory", None, Arg::None, Use::Handle),
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
    /// Ruby code to load (`-r`, `--plugin`, a custom formatter class): RuboCop runs instead, when
    /// one is found (fallback::delegate).
    Ruby(String),
    /// `-h`: OptionParser prints the help and exits as soon as it reaches the option.
    Help,
}

#[derive(Default, Clone)]
struct Cli {
    opts: Options,
    /// -a / -A
    autocorrect: Option<rbcop::autocorrect::Mode>,
    /// `--diff`: autocorrect in memory and print the unified diffs instead of writing files.
    diff: bool,
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
    /// `--cache-root DIR`
    cache_root: Option<String>,
    show_cops: Option<Vec<String>>,
    /// `--list-enabled-cops-for PATH`
    list_enabled_for: Option<String>,
    /// `--show-docs-url [COPS]`
    show_docs_url: Option<Vec<String>>,
    /// `--explain COPS`
    explain: Option<Vec<String>>,
    /// `--max-offenses-per-cop COUNT` (OffenseLimit's max).
    max_offenses: Option<usize>,
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
    /// `--auto-gen-config` and the options of the todo it writes.
    auto_gen_config: bool,
    gen_opts: rbcop::autogen::GenOptions,
    /// `--disable-uncorrectable`
    disable_uncorrectable: bool,
    /// `--report-unused-todo-entries`
    report_unused_todo: bool,
    version: bool,
    verbose_version: bool,
    /// `--lsp`, `--mcp`
    lsp: bool,
    mcp: bool,
    /// EXITING_OPTIONS seen, in order (for "Incompatible cli options").
    exiting: Vec<&'static str>,
    /// `-d/--debug`
    debug: bool,
    /// `--profile`, `--memory` (which does nothing without `--profile`).
    profile: bool,
    memory: bool,
    /// The keys of RuboCop's `@options` hash (CLI#parallel_by_default! looks at them).
    keys: Vec<String>,
    /// The first option that loads Ruby code (`-r`, `--plugin`): the run goes to RuboCop
    /// (Fail::Ruby) once every option has parsed.
    ruby: Option<String>,
    /// `--changed [REVISION]`: the revision (HEAD when none is given).
    changed: Option<String>,
    /// `--changed PATH`: the path OptionParser took as the revision (Options#return_path_given_to_changed).
    changed_path: Option<String>,
}

/// ChangedFiles.revision?: whether `name` resolves to a commit in the current repository.
fn is_revision(name: &str) -> bool {
    std::process::Command::new("git")
        .args(["rev-parse", "--verify", "--quiet", &format!("{name}^{{commit}}")])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
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
fn all_args(argv: Vec<String>) -> Result<Vec<String>, Fail> {
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
    args.extend(argv);
    Ok(args)
}

/// RuboCop::Server::CLI#run, which exe/rubocop runs on ARGV before anything else: it takes the server
/// options out of ARGV, rejects bad combinations, and runs `--start-server` & co. (Some(status) when
/// the command ends there).
///
/// Deliberate difference: rbcop runs in process and never starts a server, so the server is never
/// running: `--server-status` says so, `--stop-server` warns it, `--start-server` starts nothing and
/// prints no `RuboCop server starting on ...` line (`--restart-server` only warns from the stop), and
/// `--server` / `--no-detach` lint in this process.
fn server_cli(argv: &mut Vec<String>, color: bool) -> Option<u8> {
    const SERVER_OPTIONS: [&str; 7] = [
        "--server",
        "--no-server",
        "--server-status",
        "--restart-server",
        "--start-server",
        "--stop-server",
        "--no-detach",
    ];
    let fail = |m: String| {
        warn_red(color, &m);
        Some(ERROR)
    };
    // Server::CLI#fetch_cache_root_path_from: `split('=')[1]` (nil when only `=`s follow, as
    // String#split drops trailing empty fields), or the next argument.
    let cache_root = argv.iter().position(|a| a.starts_with("--cache-root")).is_some_and(|i| {
        match argv[i].strip_prefix("--cache-root=") {
            Some(v) => v.chars().any(|c| c != '='),
            None => i + 1 < argv.len(),
        }
    });
    let mut server_arguments: Vec<&str> = SERVER_OPTIONS.into_iter().filter(|o| argv.iter().any(|a| a == o)).collect();
    argv.retain(|a| !SERVER_OPTIONS.contains(&a.as_str()));
    let detach =
        server_arguments.iter().position(|o| *o == "--no-detach").map(|i| server_arguments.remove(i)).is_none();
    if server_arguments.len() >= 2 {
        return fail(format!("{} cannot be specified together.", server_arguments.join(", ")));
    }
    let command = server_arguments.first().copied().unwrap_or("");
    if !detach && !["--server", "--start-server", "--restart-server"].contains(&command) {
        return fail(format!("{command} cannot be combined with --no-detach."));
    }
    let allowed = if cache_root { 2 } else { 1 };
    if SERVER_OPTIONS[2..].contains(&command) && argv.len() > allowed {
        return fail(format!("{command} cannot be combined with {}.", argv[0]));
    }
    match command {
        "--server-status" => println!("RuboCop server is not running."),
        "--stop-server" | "--restart-server" => eprintln!("RuboCop server is not running."),
        "--start-server" => {}
        _ => return None,
    }
    Some(0)
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
    let all = args.clone();
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
            // OptionsValidator#validate_max_offenses_per_cop_option raises OptionParser::InvalidArgument
            // from the option's block, which OptionParser words with the option as written (or the
            // whole `--option=VALUE` when given inline). `/^[1-9]\d*$/` matches any line.
            if name == "max-offenses-per-cop"
                && !value.as_deref().is_some_and(|v| {
                    v.split('\n').any(|l| {
                        l.starts_with(|c: char| ('1'..='9').contains(&c)) && l.bytes().all(|b| b.is_ascii_digit())
                    })
                })
            {
                return Err(Fail::Parse(if inline_given {
                    format!("invalid argument: {written}={}", value.unwrap_or_default())
                } else {
                    format!("invalid argument: {written} --max-offenses-per-cop must be a positive integer")
                }));
            }
            // `@options.replace(ConfigRegeneration.new.options.merge(@options))`: the options of the
            // command recorded in the todo file, overridden by those given before this one (then
            // the rest apply as usual), so the arguments are parsed again in that order.
            if name == "regenerate-todo" {
                let rest: Vec<String> = it.collect();
                let mut again = regeneration_args();
                again.extend_from_slice(&all[..all.len() - rest.len() - 1]);
                // Expanded once: a later `--regenerate-todo` would only repeat the same options.
                let mut paths = false;
                for a in rest {
                    paths |= a == "--";
                    if paths || !is_regenerate_todo(&a) {
                        again.push(a);
                    }
                }
                return parse_args(again);
            }
            if usage == Use::Ruby {
                cli.ruby.get_or_insert_with(|| format!("--{name} loads Ruby code, which rbcop cannot run"));
                continue;
            }
            cli.keys.push(name.replace('-', "_"));
            apply(&mut cli, name, neg, value)?;
        }
    }
    if let Some(p) = cli.changed_path.take() {
        cli.paths.insert(0, PathBuf::from(p));
    }
    Ok(cli)
}

/// ConfigRegeneration#options: the `rubocop ...` command on the second line of `.rubocop_todo.yml`,
/// or `--auto-gen-config` without one.
///
/// Deliberate difference: the file comes with the project, so only the options
/// DisabledConfigFormatter#command writes are taken from it; anything else (an output file, a
/// config, paths, `--regenerate-todo` itself) is dropped with a warning.
fn regeneration_args() -> Vec<String> {
    const ALLOWED: &[&str] = &[
        "--auto-gen-config",
        "--auto-gen-only-exclude",
        "--no-auto-gen-only-exclude",
        "--disable-pending-cops",
        "--enable-pending-cops",
        "--preview",
        "--no-preview",
        "--no-exclude-limit",
        "--offense-counts",
        "--no-offense-counts",
        "--auto-gen-timestamp",
        "--no-auto-gen-timestamp",
        "--auto-gen-enforced-style",
        "--no-auto-gen-enforced-style",
    ];
    let text = fs::read_to_string(AUTO_GENERATED_FILE).unwrap_or_default();
    // File.foreach(...).take(2).last
    let line = text.split_inclusive('\n').take(2).last().unwrap_or("");
    // /(?<=`rubocop )(.*?)(?=`)/
    let Some((cmd, _)) = line.split_once("`rubocop ").and_then(|(_, r)| r.split_once('`')) else {
        return vec!["--auto-gen-config".into()];
    };
    let mut out = Vec::new();
    let mut words = cmd.split_whitespace().peekable();
    while let Some(w) = words.next() {
        if ALLOWED.contains(&w) {
            out.push(w.to_string());
        } else if w == "--exclude-limit" && words.peek().is_some_and(|n| n.bytes().all(|b| b.is_ascii_digit())) {
            out.push(w.to_string());
            out.push(words.next().unwrap().to_string());
        } else {
            eprintln!("rbcop: ignoring `{w}` from the command recorded in {AUTO_GENERATED_FILE}");
        }
    }
    out
}

/// A `--regenerate-todo` written in full or as a prefix OptionParser completes to it.
fn is_regenerate_todo(arg: &str) -> bool {
    arg.len() >= "--reg".len() && "--regenerate-todo".starts_with(arg)
}

fn cop_list(name: &str, value: Option<String>) -> Result<Vec<String>, Fail> {
    let Some(list) = value else {
        return Err(Fail::Arg(format!("--{name} argument should be [COP1,COP2,...].")));
    };
    Ok(if list.is_empty() { vec![String::new()] } else { list.split(',').map(String::from).collect() })
}

/// String#to_i for a non-negative count: leading whitespace, then digits (with `_` between them).
fn ruby_to_i(s: &str) -> usize {
    let t = s.trim_start_matches([' ', '\t', '\n', '\x0B', '\x0C', '\r']);
    let t = t.strip_prefix('+').unwrap_or(t);
    let mut n: usize = 0;
    let b = t.as_bytes();
    for (i, &c) in b.iter().enumerate() {
        if c.is_ascii_digit() {
            n = n.saturating_mul(10).saturating_add((c - b'0') as usize);
        } else if !(c == b'_' && i > 0 && b[i - 1].is_ascii_digit() && b.get(i + 1).is_some_and(u8::is_ascii_digit)) {
            break;
        }
    }
    n
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
    // The other `@options` keys an option's block sets.
    let implied: &[&str] = match name {
        "lint" => &["only"],
        "fix-layout" => &["only", "autocorrect"],
        "autocorrect" => &["safe_autocorrect"],
        "autocorrect-all" => &["autocorrect"],
        "format" => &["formatters"],
        "out" if cli.formatters.is_empty() => &["output_path"],
        "profile" if cli.cache.is_none() => &["cache"],
        _ => &[],
    };
    cli.keys.extend(implied.iter().map(|k| k.to_string()));
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
        "exclude-limit" => {
            cli.auto_gen[0] = true;
            cli.gen_opts.exclude_limit = value;
        }
        "offense-counts" => {
            cli.auto_gen[1] = true;
            cli.gen_opts.offense_counts = Some(!neg);
        }
        "auto-gen-timestamp" => {
            cli.auto_gen[2] = true;
            cli.gen_opts.timestamp = Some(!neg);
        }
        "auto-gen-only-exclude" => {
            cli.auto_gen[3] = true;
            cli.gen_opts.only_exclude = Some(!neg);
        }
        "auto-gen-enforced-style" => cli.gen_opts.enforced_style = Some(!neg),
        "no-exclude-limit" => cli.gen_opts.no_exclude_limit = true,
        "auto-gen-config" => cli.auto_gen_config = true,
        "disable-uncorrectable" => {
            cli.disable_uncorrectable = true;
            rbcop::autocorrect::set_disable_uncorrectable();
        }
        "report-unused-todo-entries" => cli.report_unused_todo = true,
        "list-enabled-cops-for" => {
            cli.list_enabled_for = value;
            cli.exiting.push("list_enabled_cops_for");
        }
        "explain" => {
            // Ruby's split(',') drops trailing empty fields.
            let mut list: Vec<String> = value.unwrap_or_default().split(',').map(String::from).collect();
            while list.last().is_some_and(String::is_empty) {
                list.pop();
            }
            cli.explain = Some(list);
            cli.exiting.push("explain");
        }
        "max-offenses-per-cop" => cli.max_offenses = Some(ruby_to_i(&value.unwrap_or_default())),
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
        "changed" => {
            // OptionsValidator#path_given_to_changed?: OptionParser hands `--changed lib/` the path
            // as the revision, so an existing path goes back among the paths (unless it also names
            // a revision, which is ambiguous).
            let mut value = value;
            if let Some(r) = value.as_deref().filter(|r| Path::new(r).exists()) {
                if is_revision(r) {
                    return Err(Fail::Arg(format!(
                        "--changed got `{r}`, which is both a path and a git revision. Write `./{r}` for the \
                         path, or `--changed={r}^0` for the revision."
                    )));
                }
                cli.changed_path = value.take();
            }
            cli.changed = Some(value.unwrap_or_else(|| "HEAD".into()));
        }
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
            cli.cache_root = value;
        }
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
            // `@options[new] = @options.delete(old)`
            let old = name.replace('-', "_");
            cli.keys.retain(|k| *k != old);
            cli.keys.push(new[2..].replace('-', "_"));
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
        "display-suppressed" => rbcop::runner::set_display_suppressed(true),
        "diff" => cli.diff = true,
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
        "lsp" => {
            cli.lsp = true;
            cli.exiting.push("lsp");
        }
        "mcp" => {
            cli.mcp = true;
            cli.exiting.push("mcp");
        }
        "verbose-version" => {
            cli.verbose_version = true;
            cli.exiting.push("verbose_version");
        }
        "debug" => {
            cli.debug = true;
            cli.opts.debug = true;
        }
        // `--profile` turns the cache off unless `--cache` came first.
        "profile" => {
            cli.profile = true;
            cli.cache.get_or_insert_with(|| "false".into());
        }
        "memory" => cli.memory = true,
        "help" => return Err(Fail::Help),
        _ => {} // ignored options
    }
    Ok(())
}

/// OptionsValidator#validate_compatibility (the checks that apply to what rbcop supports).
fn validate(cli: &mut Cli, global: bool) -> Result<(), Fail> {
    // Options#imply_autocorrect_for_diff (before the validation): safe autocorrection unless -a,
    // -A or -x picked a mode.
    if cli.diff && cli.autocorrect.is_none() {
        cli.autocorrect = Some(rbcop::autocorrect::Mode::Safe);
        cli.safe_autocorrect = true;
        cli.keys.extend(["autocorrect".into(), "safe_autocorrect".into()]);
    }
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
    if let Some(i) = cli.auto_gen.iter().position(|&given| given).filter(|_| !cli.auto_gen_config) {
        return Err(Fail::Arg(format!("--{} can only be used together with --auto-gen-config.", AUTO_GEN[i])));
    }
    if cli.safe_autocorrect && cli.autocorrect_all {
        // Rainbow(<<~MESSAGE).red, then warned red again: the trailing newline is part of the message.
        let m = "Error: Both safe and unsafe autocorrect options are specified, use only one.\n";
        return Err(Fail::Arg(red(global, m)));
    }
    if cli.disable_uncorrectable && cli.autocorrect.is_none() {
        return Err(Fail::Arg("--disable-uncorrectable can only be used together with --autocorrect.".into()));
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
    if cli.lsp && cli.keys.iter().any(|k| k == "editor_mode") {
        return Err(Fail::Arg("Do not specify `--editor-mode` as it is redundant in `--lsp`.".into()));
    }
    if cli.diff && cli.auto_gen_config {
        return Err(Fail::Arg("--diff cannot be used with --auto-gen-config.".into()));
    }
    if cli.changed.is_some() && cli.stdin.is_some() {
        return Err(Fail::Arg("--changed cannot be used with --stdin.".into()));
    }
    if cli.enable_all && cli.disable_all {
        return Err(Fail::Arg("--enable-all-cops cannot be used together with --disable-all-cops.".into()));
    }
    if cli.enable_all || cli.disable_all {
        cli.opts.all_cops = Some(cli.enable_all);
    }
    if cli.parallel == Some(true) {
        let invalid: Vec<&str> = [
            cli.fail_fast.then_some("-F/--fail-fast"),
            (cli.cache.as_deref() == Some("false")).then_some("--cache false"),
        ]
        .into_iter()
        .flatten()
        .collect();
        if !invalid.is_empty() {
            cli.parallel = None;
            cli.keys.retain(|k| k != "parallel");
            // On stderr, so that it can't end up in the middle of a formatter's output.
            eprintln!("-P/--parallel is being ignored because it is not compatible with {}.", invalid.join(", "));
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
    // env::args panics on an argument that is not UTF-8 (a file name on Linux, say).
    let mut argv: Vec<String> = env::args_os().skip(1).map(|a| a.to_string_lossy().into_owned()).collect();
    let given = argv.len();
    if let Some(status) = server_cli(&mut argv, global_color(None)) {
        return ExitCode::from(status);
    }
    // An editor or a server opening a project runs none of its Ruby unless RBCOP_FALLBACK=1: the
    // server options (those server_cli took out of argv, or any parsed from `.rubocop` and
    // RUBOCOP_OPTS or abbreviated), --lsp, --mcp, --editor-mode and --stdin.
    const EDITOR: [&str; 10] = [
        "lsp",
        "mcp",
        "server",
        "start_server",
        "restart_server",
        "stop_server",
        "server_status",
        "no_detach",
        "editor_mode",
        "stdin",
    ];
    let server = argv.len() != given;
    let parsed = all_args(argv.clone()).and_then(parse_args).and_then(|mut cli| {
        if server || cli.keys.iter().any(|k| EDITOR.contains(&k.as_str())) {
            fallback::editor_context();
        }
        let ruby = cli.ruby.take();
        let class = cli.formatters.iter().find(|(k, _)| formatter::is_class(k));
        match ruby.or_else(|| class.map(|(k, _)| format!("custom formatter classes are not supported by rbcop: {k}"))) {
            Some(m) => Err(Fail::Ruby(m)),
            None => Ok(cli),
        }
    });
    let color_opt = parsed.as_ref().ok().and_then(|cli| cli.color);
    // CLI#run's ensure: `@options` is only set once Options#parse (validation included) succeeded.
    let mut display_time = false;
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
        // Output identical to RuboCop's, since it is RuboCop's.
        Err(Fail::Ruby(m)) => match fallback::rubocop() {
            Some(program) => {
                fallback::announce(&program, &format!("the whole run ({m})"));
                fallback::delegate(&program, &argv)
            }
            None => {
                eprintln!("rbcop: {m}");
                if let Some(hint) = fallback::missing() {
                    eprintln!("{hint}");
                }
                ERROR
            }
        },
        Err(Fail::Help) => {
            print_help(parse_color && !env::args().any(|a| a == "--no-color"));
            0
        }
        // CLI#run: --init runs before the configuration is loaded.
        Ok(cli) if cli.init => {
            display_time = cli.display_time || cli.debug;
            init_dotfile(parse_color)
        }
        Ok(cli) => {
            display_time = cli.display_time || cli.debug;
            let profile = cli.profile.then_some(cli.memory);
            let roots = rbcop::autocorrect::write_roots(&rbcop::config::cwd(), &cli.paths);
            if profile.is_some() {
                runner::enable_profiling();
            }
            let result = if cli.auto_gen_config {
                auto_gen_config(cli, global, parse_color)
            } else {
                run(cli, global, parse_color, None)
            };
            let code = match result {
                Ok(code) => code,
                Err(e) => {
                    warn_red(global, &e);
                    ERROR
                }
            };
            if let Some(memory) = profile {
                profile_reports(memory, t0.elapsed(), &roots);
            }
            code
        }
    };
    if display_time {
        println!("Finished in {} seconds", ruby_seconds(t0.elapsed().as_secs_f64()));
    }
    ExitCode::from(status)
}

/// A write `--auto-gen-config` makes failed, or was refused (autocorrect::write_contained).
fn not_writing(e: &std::io::Error, path: &Path) -> String {
    match e.kind() {
        std::io::ErrorKind::PermissionDenied => format!("rbcop: not writing {}: {e}", path.display()),
        _ => runner::io_error(e, path),
    }
}

/// AutoGenerateConfig::AUTO_GENERATED_FILE
const AUTO_GENERATED_FILE: &str = ".rubocop_todo.yml";

/// `--auto-gen-config`: CLI::Command::AutoGenerateConfig. A run with only Layout/LineLength (so
/// cops that read its Max see the generated one), then a run of every cop; both write
/// `.rubocop_todo.yml`, and the first run's Layout/LineLength entry is appended to the second's.
fn auto_gen_config(mut cli: Cli, global: bool, parse_color: bool) -> Result<u8, String> {
    const PHASE_1: &str = "Phase 1 of 2: run Layout/LineLength cop";
    const PHASE_2: &str = "Phase 2 of 2: run all cops";
    // CLI#act_on_options handles the exiting options before any command runs.
    if cli.version
        || cli.verbose_version
        || cli.show_cops.is_some()
        || cli.list_enabled_for.is_some()
        || cli.show_docs_url.is_some()
        || cli.explain.is_some()
    {
        return run(cli, global, parse_color, None);
    }
    let names: Vec<&'static str> = cops::ALL.iter().map(|c| c.name).chain(["Lint/Syntax"]).collect();
    // CLI#run up to execute_runners: the configuration as it is before the todo is wired in.
    if let Some(c) = &cli.opts.config_file {
        let abs = expand_path(c, &rbcop::config::cwd());
        if !abs.is_file() {
            return Err(format!("Configuration file not found: {}", abs.display()));
        }
    }
    cli.opts.color = parse_color;
    let load = |opts: &rbcop::config::Options| {
        Store::new(opts.clone(), names.clone()).and_then(|mut store| {
            let cwd = store.cwd().to_path_buf();
            store.for_dir(&cwd)
        })
    };
    let pwd_config = match load(&cli.opts) {
        Ok(c) => c,
        Err(e) => {
            warn_red(parse_color, &e);
            return Ok(ERROR);
        }
    };
    if !cli.opts.disable_pending_cops && !cli.opts.enable_pending_cops {
        report_pending_cops(&pwd_config, global);
    }
    let mut gen_opts = cli.gen_opts.clone();
    gen_opts.disable_pending_cops = cli.opts.disable_pending_cops;
    gen_opts.enable_pending_cops = cli.opts.enable_pending_cops;
    gen_opts.preview = cli.opts.preview;
    rbcop::autogen::set_gen_context(rbcop::autogen::GenContext {
        opts: gen_opts.clone(),
        store_opts: cli.opts.clone(),
        cop_names: names.clone(),
        stderr: cli.stderr,
        roots: rbcop::autocorrect::write_roots(&rbcop::config::cwd(), &cli.paths),
    });
    rbcop::autogen::enable();
    let paint = formatter::Paint(global);
    // reset_config_and_auto_gen_file. Deliberate difference: like autocorrect, these writes stay
    // inside the project (the working directory and the command-line paths) and never follow a
    // symlink.
    let roots = rbcop::autocorrect::write_roots(&rbcop::config::cwd(), &cli.paths);
    rbcop::autocorrect::write_contained(Path::new(AUTO_GENERATED_FILE), b"", &roots, false)
        .map_err(|e| not_writing(&e, Path::new(AUTO_GENERATED_FILE)))?;
    add_inheritance_from_auto_generated_file(cli.opts.config_file.as_deref(), &roots)?;
    // maybe_run_line_length_cop, with a new ConfigStore (which now reads the empty todo).
    let pwd_config = match load(&cli.opts) {
        Ok(c) => c,
        Err(e) => {
            warn_red(global, &e);
            return Ok(ERROR);
        }
    };
    let default = Store::new(cli.opts.clone(), names.clone())?.default_config();
    let max = |c: &Resolved| c.hash.map("Layout/LineLength").and_then(|m| m.get("Max")).cloned();
    let skip = if gen_opts.only_exclude == Some(true) {
        Some("(skipped because only excludes will be generated due to `--auto-gen-only-exclude` flag)")
    } else if matches!(pwd_config.enable_cop("Layout/LineLength"), rbcop::config::value::Value::Null)
        || pwd_config.enable_cop("Layout/LineLength") == rbcop::config::value::Value::Bool(false)
    {
        Some("(skipped because Layout/LineLength is disabled)")
    } else if max(&pwd_config) != max(&default) {
        Some("(skipped because the default Layout/LineLength:Max is overridden)")
    } else if cli.only.is_some() {
        Some("(skipped because a list of cops is passed to the `--only` flag)")
    } else {
        None
    };
    let line_length_contents = match skip {
        Some(reason) => {
            println!("{}", paint.yellow(&format!("{PHASE_1} {reason}")));
            String::new()
        }
        None => {
            println!("{}", paint.yellow(PHASE_1));
            let mut one = cli.clone();
            one.only = Some(vec!["Layout/LineLength".into()]);
            one.opts.only = vec!["Layout/LineLength".into()];
            if run(one, global, parse_color, Some(1))? == ERROR {
                return Ok(ERROR);
            }
            // Save the todo configuration of the LineLength cop.
            let text = fs::read_to_string(AUTO_GENERATED_FILE).unwrap_or_default();
            text.split_inclusive('\n').skip_while(|l| l.starts_with('#')).collect()
        }
    };
    println!("{}", paint.yellow(PHASE_2));
    let status = run(cli, global, parse_color, Some(2))?;
    if status == ERROR {
        return Ok(status);
    }
    // This run was made with the current maximum length allowed, so append the saved setting.
    rbcop::autocorrect::write_contained(Path::new(AUTO_GENERATED_FILE), line_length_contents.as_bytes(), &roots, true)
        .map_err(|e| not_writing(&e, Path::new(AUTO_GENERATED_FILE)))?;
    Ok(status)
}

/// AutoGenerateConfig#add_inheritance_from_auto_generated_file: `.rubocop.yml` (or the `-c` file)
/// inherits from the todo first.
fn add_inheritance_from_auto_generated_file(config_file: Option<&Path>, roots: &[PathBuf]) -> Result<(), String> {
    const PLACEHOLDER: &str = "###rubocop:inherit_here";
    // relative_path_to_todo_from_options_config
    let todo = match config_file {
        None => AUTO_GENERATED_FILE.to_string(),
        Some(c) => {
            let base = rbcop::config::cwd();
            let dir =
                fs::canonicalize(c).map_err(|e| e.to_string())?.parent().map(Path::to_path_buf).unwrap_or_default();
            if dir == base {
                AUTO_GENERATED_FILE.to_string()
            } else {
                format!("{}/{AUTO_GENERATED_FILE}", rbcop::config::relative_path(&base, &dir))
            }
        }
    };
    let file = config_file.unwrap_or(Path::new(".rubocop.yml"));
    let mut file_string = format!(" {todo}");
    let mut contents: Option<String> = None;
    if file.exists() {
        let text = fs::read(file).map_err(|e| runner::io_error(&e, file))?;
        let text = String::from_utf8_lossy(&text).into_owned();
        use rbcop::config::value::{Value, parse_yaml_file};
        let inherit = match parse_yaml_file(&text, &file.display().to_string()) {
            Ok(Value::Map(m)) => m.get("inherit_from").cloned(),
            _ => None,
        };
        // Array(...)
        let mut files: Vec<String> = match inherit {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Seq(items)) => items.iter().map(|v| v.as_string().unwrap_or_default()).collect(),
            Some(v) => vec![v.as_string().unwrap_or_default()],
        };
        if files.contains(&todo) {
            return Ok(());
        }
        files.insert(0, todo.clone());
        if files.len() > 1 {
            file_string = format!("\n  - {}", files.join("\n  - "));
        }
        // existing_configuration
        let single = regex::Regex::new(r"(?m)^inherit_from: *[^\n]+").unwrap();
        let list = regex::Regex::new(r"(?m)^inherit_from: *(\n *- *[^\n]+)+").unwrap();
        let text = single.replacen(&text, 1, PLACEHOLDER).into_owned();
        contents = Some(list.replacen(&text, 1, PLACEHOLDER).into_owned());
    }
    // write_config_file
    let mut lines: Vec<String> = match &contents {
        Some(c) if c.chars().any(|ch| !(ch.is_ascii_whitespace() || ch == '\x0b')) => {
            c.split('\n').map(String::from).collect()
        }
        _ => Vec::new(),
    };
    if !contents.as_deref().is_some_and(|c| c.contains(PLACEHOLDER)) {
        // YAML_OPTIONAL_DOC_START = /\A---(\s+#|\s*\z)/
        let doc_start = lines.iter().position(|l| {
            l.strip_prefix("---").is_some_and(|r| {
                let t = r.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '\x0b');
                t.is_empty() || (t.len() < r.len() && t.starts_with('#'))
            })
        });
        lines.insert(doc_start.map_or(0, |i| i + 1), PLACEHOLDER.to_string());
    }
    let joined = lines.join("\n");
    // .sub(/#{PLACEHOLDER}\n*/, ...)
    let at = joined.find(PLACEHOLDER).unwrap_or(0);
    let after = joined[at + PLACEHOLDER.len()..].trim_start_matches('\n');
    let mut out = format!("{}inherit_from:{file_string}\n\n{after}", &joined[..at]);
    // .sub(/\n\n+\Z/, "\n")
    let body = out.trim_end_matches('\n').len();
    if out.len() - body >= 2 {
        out.truncate(body);
        out.push('\n');
    }
    rbcop::autocorrect::write_contained(file, out.as_bytes(), roots, false).map_err(|e| not_writing(&e, file))?;
    println!("Added inheritance from `{todo}` in `.rubocop.yml`.");
    Ok(())
}

/// CLI#profile_if_needed's reports. Deliberate difference: RuboCop's are StackProf's CPU profile
/// and MemoryProfiler's allocations of the Ruby VM; rbcop's are the wall time spent in each cop
/// and its peak resident set size, as text files under `<project root>/tmp`.
fn profile_reports(memory: bool, elapsed: std::time::Duration, roots: &[PathBuf]) {
    let cwd = rbcop::config::cwd();
    // ponytail: RuboCop fails (File.join(nil, 'tmp')) without a Gemfile; rbcop uses the working directory.
    let root = rbcop::config::project_root(&cwd).unwrap_or(cwd);
    let mut text = format!(
        "rbcop --profile: wall time in each cop, summed over files and threads (run: {:.3} s)\n\n{:>12}  {:>10}  cop\n",
        elapsed.as_secs_f64(),
        "ms",
        "calls"
    );
    for (cop, time, calls) in runner::profile() {
        text.push_str(&format!("{:>12.3}  {calls:>10}  {cop}\n", time.as_secs_f64() * 1e3));
    }
    match write_report(&root, "rbcop-profile.txt", &text, roots) {
        Ok(path) => println!("Profile report generated at {}", path.display()),
        Err(e) => eprintln!("rbcop: not writing the --profile report: {e}"),
    }
    if memory {
        println!("Building memory report...");
        // SAFETY: getrusage fills the struct it is given.
        let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
        unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
        // ru_maxrss is in kilobytes on Linux, bytes on macOS.
        let bytes = usage.ru_maxrss as u64 * if cfg!(target_os = "macos") { 1 } else { 1024 };
        let text = format!("rbcop --memory: peak resident set size {bytes} bytes\n");
        match write_report(&root, "rbcop-memory.txt", &text, roots) {
            Ok(path) => println!("Memory report generated at {}", path.display()),
            Err(e) => eprintln!("rbcop: not writing the --memory report: {e}"),
        }
    }
}

/// `<root>/tmp/<name>`, written as `--profile` writes its reports, but only inside the write roots
/// (the working directory and the command-line paths, as for -a) and never through a symlink: the
/// project root and `tmp` must be real directories there, and the report is a new temporary file
/// (O_NOFOLLOW) renamed over `name`, which replaces a planted symlink instead of following it.
fn write_report(root: &Path, name: &str, text: &str, roots: &[PathBuf]) -> Result<PathBuf, String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let root = fs::canonicalize(root).map_err(|e| format!("{}: {e}", root.display()))?;
    if rbcop::autocorrect::outside_roots(&root, roots) {
        return Err(format!("{} is outside the project", root.display()));
    }
    let tmp = root.join("tmp");
    match fs::symlink_metadata(&tmp) {
        Ok(m) if m.file_type().is_symlink() => return Err(format!("{} is a symlink", tmp.display())),
        Ok(m) if !m.is_dir() => return Err(format!("{} is not a directory", tmp.display())),
        Ok(_) => {}
        Err(_) => fs::create_dir(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?,
    }
    let path = tmp.join(name);
    if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(format!("{} is a symlink", path.display()));
    }
    // ponytail: path-based checks; a `tmp` swapped for a symlink between the check and the open
    // is not caught (the -a writer walks directory descriptors for that).
    let part = tmp.join(format!(".{name}.{}", std::process::id()));
    let written = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&part)
        .and_then(|mut f| f.write_all(text.as_bytes()))
        .and_then(|()| fs::rename(&part, &path));
    if let Err(e) = written {
        let _ = fs::remove_file(&part);
        return Err(format!("{}: {e}", path.display()));
    }
    Ok(path)
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
    rbcop::helpers::cop_registry::sort_by_department(&mut names);
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
    for name in explain::known_cops(cops) {
        if let Some(url) = formatter::docs_url(cfg, name) {
            out.push_str(&url);
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
    // Registry.global: RuboCop's cops and every loaded plugin's, those rbcop does not implement too.
    let mut names: Vec<&'static str> = rbcop::helpers::cop_registry::REGISTRY.to_vec();
    rbcop::helpers::cop_registry::sort_by_department(&mut names);
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
    // Sorted by department, so each one's cops are adjacent.
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

/// RuboCop::Lockfile as Bundler reads it: the lockfile next to the Gemfile (`gems.rb` →
/// `gems.locked`, else `Gemfile` → `Gemfile.lock`) found upwards from the working directory
/// (`BUNDLE_GEMFILE` overrides), as (DEPENDENCIES names, those plus every locked spec's
/// dependency names). None without one.
fn lockfile_gems(cwd: &Path) -> Option<(Vec<String>, Vec<String>)> {
    let gemfile = match env::var_os("BUNDLE_GEMFILE").filter(|g| !g.is_empty()) {
        Some(g) => PathBuf::from(g),
        None => cwd.ancestors().find_map(|d| ["gems.rb", "Gemfile"].iter().map(|n| d.join(n)).find(|f| f.is_file()))?,
    };
    let lockfile = if gemfile.file_name().is_some_and(|n| n == "gems.rb") {
        gemfile.with_file_name("gems.locked")
    } else {
        PathBuf::from(format!("{}.lock", gemfile.display()))
    };
    let text = fs::read_to_string(lockfile).ok()?;
    let name = |l: &str| l.trim().split([' ', '!']).next().unwrap_or("").to_string();
    let (mut deps, mut spec_deps) = (Vec::new(), Vec::new());
    let mut section = "";
    for line in text.lines() {
        if !line.starts_with(' ') {
            section = line.trim_end();
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if section == "DEPENDENCIES" && indent == 2 {
            deps.push(name(line));
        } else if matches!(section, "GEM" | "PATH" | "GIT" | "PLUGIN SOURCE") && indent == 6 {
            spec_deps.push(name(line));
        }
    }
    let mut gems = deps.clone();
    gems.extend(spec_deps);
    Some((deps, gems))
}

/// CLI::Command::SuggestExtensions, run after the report: the RuboCop extensions the lockfile's
/// gems suggest that the config does not load.
fn suggest_extensions(cli: &Cli, pwd_config: &Resolved, default_config: &Resolved, loaded: &[String]) -> String {
    use rbcop::config::value::Value;
    let formatter = cli
        .format
        .clone()
        .or_else(|| pwd_config.all_cops().str("DefaultFormatter").map(String::from))
        .unwrap_or_else(|| "p".into());
    let out = cli.output_path.is_some() || cli.formatters.iter().any(|(_, o)| o.is_some());
    if env::var_os("CI").is_some()
        || cli.only.is_some()
        || cli.debug
        || cli.list_targets
        || out
        || cli.stdin.is_some()
        || !["p", "progress", "fu", "fuubar", "pa", "pacman"].contains(&formatter.as_str())
    {
        return String::new();
    }
    let Some((deps, gems)) = lockfile_gems(&rbcop::config::cwd()) else { return String::new() };
    if deps.is_empty() {
        return String::new();
    }
    let map = match pwd_config.all_cops().get("SuggestExtensions") {
        Some(Value::Bool(true)) => default_config.all_cops().get("SuggestExtensions").and_then(Value::as_map).cloned(),
        Some(Value::Map(m)) => Some((**m).clone()),
        _ => None,
    }
    .unwrap_or_default();
    // all_extensions: those with a gem among the lockfile's dependencies.
    let all: Vec<&str> = map
        .0
        .iter()
        .filter(|(_, v)| {
            let wanted: Vec<String> = match v {
                Value::Seq(items) => items.iter().filter_map(Value::as_string).collect(),
                v => v.as_string().into_iter().collect(),
            };
            wanted.iter().any(|g| deps.contains(g))
        })
        .map(|(k, _)| k.as_str())
        .collect();
    let mut not_installed: Vec<&str> = all.iter().copied().filter(|e| !gems.iter().any(|g| g == e)).collect();
    let mut not_loaded: Vec<&str> =
        all.iter().copied().filter(|e| gems.iter().any(|g| g == e) && !loaded.iter().any(|l| l == e)).collect();
    if not_installed.is_empty() && not_loaded.is_empty() {
        return String::new();
    }
    not_installed.sort_unstable();
    not_loaded.sort_unstable();
    let mut s = String::new();
    if !not_installed.is_empty() {
        s.push_str("\nTip: Based on detected gems, the following RuboCop extension libraries might be helpful:\n");
        for e in &not_installed {
            s.push_str(&format!("  * {e} (https://rubygems.org/gems/{e})\n"));
        }
    }
    if !not_loaded.is_empty() {
        s.push_str("\nThe following RuboCop extension libraries are installed but not loaded in config:\n");
        for e in &not_loaded {
            s.push_str(&format!("  * {e}\n"));
        }
    }
    s.push_str(
        "\nYou can opt out of this message by adding the following to your config (see \
         https://docs.rubocop.org/rubocop/plugins.html#plugin-suggestions for more options):\n  AllCops:\n    \
         SuggestExtensions: false\n",
    );
    if cli.display_time {
        s.push('\n');
    }
    s
}

/// CLI#report_pending_cops (PendingCopsReporter): on stderr, before any formatter output.
fn report_pending_cops(cfg: &Resolved, global: bool) {
    const BANNER: &str = "The following cops were added to RuboCop, but are not configured. Please set Enabled to \
                          either `true` or `false` in your `.rubocop.yml` file.\n\nPlease also note that you can \
                          opt-in to new cops by default by adding this to your config:\n  AllCops:\n    NewCops: \
                          enable\n";
    // pending_cops_only_qualified
    let mut pending = cfg.pending_cops();
    pending.retain(|(name, _)| rbcop::helpers::cop_registry::is_qualified_cop(name));
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
        "{RUBOCOP_VERSION} (using {parser}, rubocop-ast 1.50.0, analyzing as Ruby {}, running on ruby 4.0.6){} [{}]",
        ruby_version_string(target),
        // Version.rubydex_indicator: rbcop always has the index (vendor/rubydex).
        if cfg.all_cops().truthy("UseProjectIndex") { " +Rubydex" } else { "" },
        formatter::ruby_platform()
    )
}

/// Registry#qualified_cop_name(name, origin) with its warnings; Err is AmbiguousCopName.
fn qualify_option_name(name: &str, origin: &str, warnings: &mut Vec<String>) -> Result<String, String> {
    rbcop::helpers::cop_registry::qualify(name, origin, Some(origin), true, warnings).map_err(|e| format!("Error: {e}"))
}

/// Runner#qualify_option_cop_names, then OptionsValidator.validate_cop_list. RuboCop does this when
/// it mobilizes the first team, so the warnings and the error surface at the first inspected file.
fn qualify_cop_options(cli: &mut Cli) -> (Vec<String>, Option<String>) {
    let result = qualify_cop_lists(cli.only.as_mut(), &mut cli.except);
    cli.opts.only = cli.only.clone().unwrap_or_default();
    result
}

/// `qualify_cop_options` for an `only` and `except` list (also the MCP tools' `only`/`except`).
pub(crate) fn qualify_cop_lists(only: Option<&mut Vec<String>>, except: &mut Vec<String>) -> (Vec<String>, Option<String>) {
    use rbcop::helpers::{cop_registry, did_you_mean};
    let mut warnings = Vec::new();
    let mut only = only;
    for (opt, list) in [("only", only.as_deref_mut()), ("except", Some(&mut *except))] {
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
    for list in [only.as_deref().map_or(&[][..], |o| &o[..]), &except[..]] {
        for n in list {
            let n = n.as_str();
            if cop_registry::is_known(n) || cop_registry::is_department(n) || n == "Syntax" || n == "Lint/Syntax" {
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
/// The report, the corrected source, the correction loop's error, the cop warnings and the cop
/// errors printed for the file, what the result cache needs to save it, and the file's
/// `--auto-gen-config` updates (rbcop::autogen).
type Linted = Result<
    (FileReport, Option<String>, Option<String>, Vec<String>, Vec<(String, bool)>, Saving, rbcop::autogen::Updates),
    String,
>;

/// What `lint` returns for one file: the report, the corrected source, the correction loop's
/// error, the result cache key to save the report under, and the cache entry it was loaded from.
type LintResult = Result<(FileReport, Option<String>, Option<String>, Option<String>, Option<PathBuf>), String>;

/// A file that was inspected with the result cache on: its cache key, and whether an error was
/// recorded while inspecting it (Runner#errors).
struct Saving {
    key: Option<String>,
    errored: bool,
    /// The cache entry the report was loaded from.
    loaded: Option<PathBuf>,
}

/// Whether two targets are the same file on disk (one of them through a symlink, say).
fn same_file_twice(jobs: &[(PathBuf, std::sync::Arc<Resolved>)]) -> bool {
    use std::os::unix::fs::MetadataExt;
    let mut seen = std::collections::HashSet::new();
    jobs.iter().filter_map(|(p, _)| fs::metadata(p).ok()).any(|m| !seen.insert((m.dev(), m.ino())))
}

/// Returns the exit status. `phase` is the `--auto-gen-config` run (1 or 2) this is.
/// Runner#mobilize_cop_badge?: the cops `--only` / `--except` (cop or department names) and the
/// style-guide filter let in.
struct Badges<'a> {
    // Sets: every file asks about every cop, and `--only` may list all of them.
    only: std::collections::HashSet<&'a str>,
    except: std::collections::HashSet<&'a str>,
    only_guide: bool,
}

impl<'a> Badges<'a> {
    fn new(only: &'a [String], except: &'a [String], only_guide: bool) -> Self {
        let set = |l: &'a [String]| l.iter().map(String::as_str).collect();
        Badges { only: set(only), except: set(except), only_guide }
    }

    /// Badge#match_name? takes cop names and department names.
    fn matches(list: &std::collections::HashSet<&str>, cop: &str) -> bool {
        list.contains(cop) || cop.rsplit_once('/').is_some_and(|(d, _)| list.contains(d))
    }

    fn selected(&self, cop: &str) -> bool {
        // Runner#check_for_redundant_disables? never runs the directive cop under --only.
        // ponytail: so it also drops out of Registry#disabled_names there; matters only when the
        // config disables Lint/RedundantCopDisableDirective and --only names its department.
        if cop == "Lint/RedundantCopDisableDirective" && !self.only.is_empty() {
            return false;
        }
        (self.only.is_empty() || Self::matches(&self.only, cop))
            && (self.except.is_empty() || !Self::matches(&self.except, cop))
    }

    /// Without --only, --only-guide-cops or `AllCops: StyleGuideCopsOnly` keeps only cops whose
    /// own config has a StyleGuide. Lint/RedundantCopDisableDirective is not a team cop:
    /// Runner#redundant_cop_disable_directive runs it past this filter.
    // ponytail: it so also enters Registry#disabled_names under the filter, which matters only
    // for a config that disables it.
    fn badge_selected(&self, cop: &str, cfg: &Resolved) -> bool {
        self.badge_selected_in(cop, cfg, self.guide_only(cfg))
    }

    /// Whether the style-guide filter applies to `cfg` (hoisted out of per-cop checks).
    fn guide_only(&self, cfg: &Resolved) -> bool {
        self.only.is_empty() && (self.only_guide || cfg.all_cops().truthy("StyleGuideCopsOnly"))
    }

    fn badge_selected_in(&self, cop: &str, cfg: &Resolved, guide_only: bool) -> bool {
        self.selected(cop)
            && (!guide_only
                || cfg.hash.map(cop).is_some_and(|c| c.truthy("StyleGuide"))
                || cop == "Lint/RedundantCopDisableDirective")
    }
}

/// Runner#build_project_index (project_index_files): the whole project (the targets found under
/// the pwd config's base directory, plus `targets`) and, with `AllCops/ProjectIndexIncludesGems`,
/// the bundle's gem sources, sorted, so cross-file offenses do not depend on which files a run
/// inspects.
fn build_project_index(
    store: &mut Store,
    pwd_config: &Resolved,
    targets: &[PathBuf],
    force_exclusion: bool,
    only_recognized: bool,
) {
    let root = pwd_config.base_dir.clone();
    let mut all = targets::find(store, &[root], force_exclusion, only_recognized).unwrap_or_default();
    all.extend(targets.iter().cloned());
    let mut paths: Vec<String> = all.iter().map(|p| p.to_string_lossy().into_owned()).collect();
    if pwd_config.all_cops().truthy("ProjectIndexIncludesGems") {
        match rbcop::project_index::bundled_gem_source_files(store.cwd()) {
            Ok(gems) => paths.extend(gems),
            Err(e) => eprintln!(
                "{}",
                formatter::Paint(store.color())
                    .yellow(&format!("rubydex bundle indexing failed: {e}. Indexing the project only."))
            ),
        }
    }
    paths.sort();
    paths.dedup();
    rbcop::project_index::install(paths);
}

fn run(mut cli: Cli, global: bool, parse_color: bool, phase: Option<u8>) -> Result<u8, String> {
    // MessageAnnotator#display_cop_names?: always under -d.
    if cli.debug {
        cli.annotate.display_cop_names = Some(true);
    }
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
    // CLI#act_on_options: ConfigLoader.cache_root(--cache-root) is set before the pwd config loads.
    let loaded = Store::new(cli.opts.clone(), names.clone()).and_then(|mut store| {
        let cwd = store.cwd().to_path_buf();
        let early = store.early_cache_root(cli.cache_root.clone());
        store.for_dir(&cwd).map(|c| (store, c, early))
    });
    print!("{}", rbcop::config::take_debug());
    let (mut store, mut pwd_config, early_root) = match loaded {
        Ok(x) => x,
        Err(e) => {
            warn_red(parse_color, &e);
            return Ok(ERROR);
        }
    };
    store.set_color(global);
    let cwd = store.cwd().to_path_buf();
    let pwd = cwd.display().to_string();
    // CLI#warn_if_cache_root_changed: remote configs loaded so far were cached under the early root
    // (the top-level config's, or the `-c` file's without `--cache-root`), not the one the resolved
    // config names, which later remote configs use.
    let configured_root =
        cli.cache_root.clone().or_else(|| pwd_config.all_cops().get("CacheRootDirectory")?.as_string());
    store.set_cache_root(configured_root.clone());
    if let (Some(early), Some(desired)) = (cache::root_dir(early_root), cache::root_dir(configured_root.clone())) {
        let (early, desired) = (early.join("rubocop_cache"), desired.join("rubocop_cache"));
        let same =
            |a: &Path, b: &Path| a.exists() && b.exists() && fs::canonicalize(a).ok() == fs::canonicalize(b).ok();
        if early != desired && !same(&early, &desired) {
            eprintln!(
                "{}",
                formatter::Paint(parse_color).yellow(&format!(
                    "Warning: Remote configuration cache files were stored in `{}` because a desired cache root \
                     (`{}`) was not set at the top level. Consider setting `AllCops: CacheRootDirectory` in your \
                     toplevel configuration file, using the `RUBOCOP_CACHE_ROOT` environment variable, or using \
                     CLI options.",
                    early.display(),
                    desired.display()
                ))
            );
        }
    }

    // CLI#handle_exiting_options
    if cli.version {
        println!("{RUBOCOP_VERSION}");
        return Ok(0);
    }
    if cli.verbose_version {
        store.debug_pwd_yaml();
        print!("{}", rbcop::config::take_debug());
        println!("{}", verbose_version(&pwd_config));
        for line in store.extension_versions() {
            println!("{line}");
        }
        return Ok(0);
    }
    if let Some(patterns) = &cli.show_cops {
        show_cops(patterns, &pwd_config, &pwd);
        // Checked after printing; a wildcard is a pattern, not a name.
        explain::validate_cop_names(patterns.iter().map(String::as_str).filter(|p| !p.contains('*')))?;
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
        explain::validate_cop_names(cops.iter().map(String::as_str))?;
        return Ok(0);
    }
    if let Some(cops) = &cli.explain {
        let (text, checked) = explain::explain(cops, &pwd_config, &cwd);
        print!("{text}");
        checked?;
        return Ok(0);
    }
    if cli.lsp {
        return lsp::serve(store, pwd_config);
    }
    if cli.mcp {
        return mcp::serve(store, pwd_config);
    }
    // CLI#validate_options_vs_config
    if cli.parallel == Some(true) && !pwd_config.all_cops().truthy("UseCache") {
        return Err("-P/--parallel uses caching to speed up execution, so combining with AllCops: UseCache: false \
                    is not allowed."
            .into());
    }
    // CLI#parallel_by_default!: with only these options given, RuboCop inspects in parallel.
    const DEFAULT_PARALLEL_OPTIONS: &[&str] = &[
        "color",
        "config",
        "debug",
        "display_style_guide",
        "display_time",
        "display_only_fail_level_offenses",
        "display_only_failed",
        "editor_mode",
        "except",
        "extra_details",
        "fail_level",
        "fix_layout",
        "format",
        "formatters",
        "ignore_disable_comments",
        "lint",
        "max_offenses_per_cop",
        "only",
        "only_guide_cops",
        "out",
        "preview",
        "require",
        "safe",
        "autocorrect",
        "safe_autocorrect",
        "autocorrect_all",
    ];
    let mut parallel = cli.parallel == Some(true);
    if !parallel
        && cli.keys.iter().all(|k| DEFAULT_PARALLEL_OPTIONS.contains(&k.as_str()))
        && pwd_config.all_cops().get("UseCache") != Some(&rbcop::config::value::Value::Bool(false))
    {
        if cli.debug {
            println!("Use parallel by default.");
        }
        parallel = true;
    }
    // AutoGenerateConfig runs after CLI#report_pending_cops.
    if phase.is_none() && !cli.opts.disable_pending_cops && !cli.opts.enable_pending_cops {
        report_pending_cops(&pwd_config, global);
    }

    // CLI#apply_default_formatter, then FormatterSet#add_formatter's lookup.
    let pairs: Vec<(String, Option<PathBuf>)> = if cli.formatters.is_empty() {
        let default = match cli.auto_gen_config {
            true => "autogenconf",
            false => pwd_config.all_cops().str("DefaultFormatter").unwrap_or("progress"),
        };
        vec![(default.to_string(), cli.output_path.clone())]
    } else {
        cli.formatters.clone()
    };
    let mut resolved = Vec::new();
    for (key, path) in &pairs {
        resolved.push((formatter::resolve(key)?, path.clone()));
    }
    // AutoGenerateConfig#add_formatter
    if phase.is_some() {
        resolved.push((formatter::TODO, Some(PathBuf::from(AUTO_GENERATED_FILE))));
    }
    // JSONFormatter prints Offense#corrections.
    if resolved.iter().any(|(n, _)| *n == "[j]son") {
        rbcop::cop::record_edits();
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
            // Runner#only_changed
            if let Some(revision) = &cli.changed {
                let changed = targets::changed(revision).map_err(|e| format!("Error: {e}"))?;
                files.retain(|f| changed.contains(f));
            }
            None
        }
    };
    // Config loading while finding the targets (a directory's config), then each file's, printed
    // when that file's turn comes (`-d`).
    print!("{}", rbcop::config::take_debug());
    let mut loading = vec![String::new(); files.len()];
    if !cli.list_targets && pwd_config.all_cops().truthy("UseProjectIndex") {
        build_project_index(&mut store, &pwd_config, &files, cli.force_exclusion, cli.only_recognized);
    }

    // Runner#qualify_option_cop_names runs when the first team mobilizes: after the configs of the
    // files up to the first one read have loaded (and the plugins they name, whose cops join the
    // registry), not those of later files. A config error before that ends the run first.
    let mut early_config_error = None;
    let first_read = files.iter().position(|f| stdin_text.is_some() || f.is_file());
    if !cli.list_targets
        && let Some(k) = first_read
    {
        for (i, f) in files[..=k].iter().enumerate() {
            let loaded = store.for_file(f);
            loading[i].push_str(&rbcop::config::take_debug());
            if let Err(e) = loaded {
                early_config_error = Some((i, e));
                break;
            }
        }
    }
    let (cop_warnings, cop_error) =
        if early_config_error.is_some() { (Vec::new(), None) } else { qualify_cop_options(&mut cli) };
    runner::set_except_redundant(
        cli.except.iter().any(|n| {
            matches!(n.as_str(), "Lint/RedundantCopDisableDirective" | "RedundantCopDisableDirective" | "Lint")
        }),
    );
    store.set_only(cli.opts.only.clone());
    pwd_config = store.for_dir(&cwd)?;

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
        default_config: store.default_config(),
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
    let badges = Badges::new(only, except, cli.only_guide);
    let selected = |cop: &str| badges.selected(cop);
    let badge_selected = |cop: &str, cfg: &Resolved| badges.badge_selected(cop, cfg);
    let guide_only = |cfg: &Resolved| badges.guide_only(cfg);
    let badge_selected_in =
        |cop: &str, cfg: &Resolved, guide_only: bool| badges.badge_selected_in(cop, cfg, guide_only);
    // Resolve configs up front (single-threaded), then lint in parallel.
    let mut jobs = Vec::with_capacity(files.len());
    // RuboCop loads a file's config when the file's turn comes, so an invalid config stops the run
    // there, after the files before it have been reported.
    let mut config_error = None;
    // Team#validate_config fails the same way, but later: when the file's team is mobilized, after
    // the --only/--except warnings and Config#for_cop's. (index in jobs, error)
    let mut team_error = None;
    // The registry's length once each file's config has loaded: what that file's directives see.
    let mut registry_at = std::collections::HashMap::new();
    for (i, f) in files.iter().enumerate() {
        let loaded = match early_config_error.take_if(|(at, _)| *at == i) {
            Some((_, e)) => Err(e),
            None => store.for_file(f),
        };
        loading[i].push_str(&rbcop::config::take_debug());
        match loaded {
            Ok(c) => {
                registry_at.insert(f.clone(), rbcop::helpers::cop_registry::REGISTRY.len());
                c.mobilize_team();
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
    let configs: Vec<&Resolved> =
        jobs.iter().filter(|(_, c)| seen.insert(std::sync::Arc::as_ptr(c))).map(|(_, c)| &**c).collect();
    let skipped = fallback::unimplemented(configs.iter().copied(), &implemented, |cop, cfg| badge_selected(cop, cfg));
    // RuboCop's --except rejects a native plugin's cops when that plugin is not loaded: pass only the
    // cops some resolved config has an entry for (a plugin's entries come with its default.yml).
    let known: std::collections::HashSet<&str> =
        configs.iter().flat_map(|c| c.hash.0.iter().map(|(k, _)| k.as_str())).collect();
    let names: Vec<&'static str> = names.into_iter().filter(|&n| n == "Lint/Syntax" || known.contains(n)).collect();
    let mut fallback_offenses = std::collections::HashMap::new();
    // -F with -a/-A: RuboCop corrects no file after the first failing one, so the child corrects
    // one file at a time, as each is reached (in the loop below).
    let mut fallback_per_file = None;
    // Files a cop-less plugin rbcop does not implement brings in (rubocop-erb's ERB views): rbcop
    // runs no cops on them, and the fallback runs every cop there.
    let copless = store.copless_plugins();
    let plugin_files: std::collections::HashSet<PathBuf> =
        jobs.iter().filter(|(p, c)| store.copless_plugin_file(c, p)).map(|(p, _)| p.clone()).collect();
    let unrunnable = !skipped.is_empty() || store.requires_ruby_code();
    let usable = stdin_text.is_none() && config_error.is_none() && team_error.is_none();
    // Looked up only when needed: a project that needs nothing never starts Ruby.
    let program = if usable && (unrunnable || !copless.is_empty()) { fallback::rubocop() } else { None };
    let what = fallback::what(&skipped, store.requires_ruby_code(), &copless);
    if unrunnable || !copless.is_empty() {
        let forced = env::var_os("RBCOP_FALLBACK").is_some_and(|v| v == "1");
        // Deliberate difference: the RuboCop child corrects by writing the files, which `--diff`
        // promises not to do. Refused under RBCOP_FALLBACK=1; otherwise the notice is printed.
        if let Some(program) = program.as_deref().filter(|_| !cli.diff || forced) {
            if cli.diff {
                return Err("rbcop: --diff cannot be combined with RBCOP_FALLBACK=1".into());
            }
            let child = fallback::Child {
                program,
                what: &what,
                opts: &cli.opts,
                except,
                only_guide: cli.only_guide,
                force_exclusion: cli.force_exclusion,
                autocorrect: cli.autocorrect,
            };
            if cli.fail_fast && cli.autocorrect.is_some() {
                fallback_per_file = Some(child);
            } else {
                let (whole, rest): (Vec<PathBuf>, Vec<PathBuf>) =
                    jobs.iter().map(|(p, _)| p.clone()).partition(|p| plugin_files.contains(p));
                if unrunnable {
                    fallback_offenses = fallback::run(&rest, &cwd, &names, child)?;
                }
                fallback_offenses.extend(fallback::run(&whole, &cwd, &[], child)?);
            }
        } else {
            eprintln!("{}", fallback::notice(&skipped, store.requires_ruby_code(), &copless));
            if let Some(m) = fallback::missing().filter(|_| usable && program.is_none()) {
                eprintln!("{m}");
            }
        }
    }
    // Runner#cached_run?: `--cache true|false`, else `AllCops: UseCache`; never for --stdin. Under
    // --auto-gen-config (AutoGenerateConfig#use_temporary_cache: a cache root of its own, removed
    // afterwards) every file is inspected and nothing is kept.
    let use_cache = stdin_text.is_none()
        && phase.is_none()
        && match cli.cache.as_deref() {
            Some("true") => true,
            Some("false") => false,
            _ => pwd_config.all_cops().truthy("UseCache"),
        };
    let result_cache = use_cache.then(|| cache::root_dir(configured_root.clone())).flatten().and_then(|root| {
        // ResultCache#context_checksum: the options that change offenses, plus what the cops
        // read outside each file's config: rubocop-rails' db/schema.rb (ActiveRecordHelper's
        // external_dependency_checksum), the working directory, the plugins loaded and, with
        // AllCops/UseProjectIndex, every indexed file's path, mtime and size (ProjectIndexHelp's
        // external_dependency_checksum).
        let schema = cwd.ancestors().find_map(|d| fs::read(d.join("db/schema.rb")).ok());
        let index = rbcop::project_index::get().map(|ix| cache::digest(&[ix.signature().as_bytes()]));
        let context = format!(
            "{}|{only:?}|{except:?}|{}|{}|{}|{:?}|{}|{}",
            // CachedData keeps Offense#corrections. rbcop records them only for the JSON
            // formatter (recording them on every run costs a warm run ~40% more CPU on rails),
            // so entries with and without them live apart: a JSON run never loads an entry
            // saved without its corrections.
            rbcop::cop::recording_edits(),
            cli.only_guide,
            runner::display_suppressed(),
            cwd.display(),
            store.loaded_plugins(),
            schema.map(|s| cache::digest(&[&s])).unwrap_or_default(),
            index.unwrap_or_default()
        );
        let all = pwd_config.all_cops();
        cache::Cache::new(
            &root,
            &context,
            all.truthy("AllowSymlinksInCacheRootDirectory"),
            all.int("MaxFilesInCache"),
            implemented.clone(),
        )
    });
    // Each config's signature (Config#signature), once.
    let mut signatures = std::collections::HashMap::new();
    if result_cache.is_some() {
        for (_, c) in &jobs {
            signatures
                .entry(std::sync::Arc::as_ptr(c) as usize)
                .or_insert_with(|| cache::digest(&[c.signature().as_bytes()]));
        }
    }
    let stdin_text = &stdin_text;
    // --stdin with -a/-A: the corrected source, printed after the report.
    let mut stdin_corrected: Option<String> = None;
    let write_roots = rbcop::autocorrect::write_roots(&cwd, &cli.paths);
    // One file: (report, corrected source to write back, infinite-loop error), or a read error.
    // -a/-A: read, correct until stable; offenses from every iteration come back (fixed ones marked
    // corrected). The file is written in the reporting loop below, in file order, as RuboCop does.
    let lint = |path: &PathBuf, cfg: &Resolved, mode: Option<rbcop::autocorrect::Mode>| -> LintResult {
        let display = rbcop::config::relative_path(path, &cwd);
        let plugin_file = plugin_files.contains(path);
        // Asked for every cop of the file: the config's StyleGuideCopsOnly is read once (#388).
        let guide_only = guide_only(cfg);
        let selected = |cop: &str| !plugin_file && badge_selected_in(cop, cfg, guide_only);
        if let Some(bytes) = stdin_text {
            // --stdin with -a/-A: the corrected source is printed after the report (@options[:stdin]).
            if let Some(mode) = mode {
                let c = rbcop::autocorrect::correct_bytes(path, &display, bytes.clone(), cfg, &selected, mode);
                return Ok((c.report, c.source, c.error, None, None));
            }
            return Ok((runner::inspect_bytes(path, &display, bytes.clone(), cfg, &selected), None, None, None, None));
        }
        // Runner#file_offense_cache: a cached report is used as is, except that with -a/-A one with
        // offenses is inspected (and corrected) again. The key comes back when a real run should
        // be saved.
        let mut key = None;
        if let Some(cache) = &result_cache {
            let bytes = fs::read(path).map_err(|e| runner::io_error(&e, path))?;
            let config = &signatures[&(std::ptr::from_ref(cfg) as usize)];
            let signature = format!("{config}|{:?}|{plugin_file}", registry_at.get(path));
            key = cache.key(path, &signature, &bytes);
            if let Some(k) = &key
                && let Some(report) = cache.load(k, &bytes)
                && (mode.is_none() || report.offenses.is_empty())
            {
                return Ok((report, None, None, None, Some(cache.entry(k))));
            }
            if mode.is_none() {
                return Ok((runner::inspect_bytes(path, &display, bytes, cfg, &selected), None, None, key, None));
            }
        }
        if let Some(mode) = mode {
            let c = rbcop::autocorrect::correct_file(path, &display, cfg, &selected, mode)?;
            return Ok((c.report, c.source, c.error, key, None));
        }
        let bytes = fs::read(path).map_err(|e| runner::io_error(&e, path))?;
        Ok((runner::inspect_bytes(path, &display, bytes, cfg, &selected), None, None, None, None))
    };
    // A panic outside a cop (cop panics are reported by runner::guarded) ends the run at that file,
    // like any error, instead of taking down every worker's results with it.
    let lint_one = |path: &PathBuf, cfg: &Resolved, mode: Option<rbcop::autocorrect::Mode>| -> Linted {
        rbcop::config::value::reset_run_budget();
        let view = registry_at.get(path).copied();
        let linted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            rbcop::helpers::cop_registry::with_view(view, || lint(path, cfg, mode))
        }))
        .unwrap_or_else(|_| Err(format!("rbcop: internal error while inspecting {}", path.display())));
        let errors = runner::take_file_errors();
        let errored = runner::take_errored();
        runner::flush_profile();
        let updates = rbcop::autogen::take_file();
        linted.map(|(report, corrected, loop_error, key, loaded)| {
            (
                report,
                corrected,
                loop_error,
                runner::take_file_warnings(),
                errors,
                Saving { key, errored, loaded },
                updates,
            )
        })
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
    // Runner#run_in_parallel?. Deliberate difference: RuboCop's worker processes print the configs
    // they load and the cache entries they read in an order that depends on scheduling and output
    // buffering; rbcop prints them here in file order, each config once.
    let in_parallel = parallel && !cli.diff && files.len() > 1;
    if cli.debug && error.is_none() {
        if parallel && !cli.diff {
            println!(
                "{}",
                if in_parallel {
                    "Running parallel inspection"
                } else {
                    "Skipping parallel inspection: only a single file needs inspection"
                }
            );
        }
        // Then the parent's Runner#file_started for each file, before the reports.
        if in_parallel {
            for (i, text) in loading.iter_mut().enumerate() {
                print!("{}", std::mem::take(text));
                if let Some(Some(Ok((.., Saving { loaded: Some(entry), .. }, _)))) = reports.get(i) {
                    println!("Loading cache from {}", entry.display());
                }
            }
            for (path, _) in &jobs {
                println!("Scanning {}", path.display());
            }
        }
    }
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
    // OffenseLimit: offenses offered per cop, sorted by name for #elided_per_cop.
    let mut offered: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut all_passed = true;
    // Runner#diffs (`--diff`), in file order.
    let mut diffs: Vec<String> = Vec::new();
    let mut inspected: Vec<PathBuf> = Vec::new();
    let mut config_failure = None;
    let mut cop_warnings_raised: Vec<String> = Vec::new();
    let mut aborted = false;
    let mut cache_dirty = false;
    if error.is_none() {
        let mut cop_notes = Some((cop_warnings, cop_error));
        let mut stopped = false;
        let mut warned_configs = std::collections::HashSet::new();
        let mut similar_view: Option<Option<usize>> = None;
        for (idx, ((path, cfg), linted)) in jobs.iter().zip(reports.iter_mut()).enumerate() {
            // Runner#file_started, then the file's config loading (Runner#process_file).
            if cli.debug && !in_parallel {
                println!("Scanning {}", path.display());
            }
            set.file_started(&cx, path);
            if cli.debug {
                print!("{}", std::mem::take(&mut loading[idx]));
            }
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
            // ponytail: one RuboCop start per file; only under -F with -a/-A.
            if let Some(child) = fallback_per_file {
                let one = std::slice::from_ref(path);
                if plugin_files.contains(path) {
                    fallback_offenses.extend(fallback::run(one, &cwd, &[], child)?);
                } else if unrunnable {
                    fallback_offenses.extend(fallback::run(one, &cwd, &names, child)?);
                }
            }
            let linted = match linted.take() {
                Some(l) => l,
                None => thread::scope(|s| {
                    big_stack().spawn_scoped(s, || lint_one(path, cfg, cli.autocorrect)).unwrap().join().unwrap()
                }),
            };
            // Deliberate difference: under -a/-A a file that resolves outside the project (the
            // working directory and the command-line paths) is not written; it is reported as
            // without -a/-A. write_source checks again right before writing. (`--diff` writes nothing.)
            let refused = match &linted {
                Ok((_, Some(_), _, _, _, _, _)) if stdin_text.is_none() && !cli.diff => {
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
            if cli.debug
                && !in_parallel
                && let Ok((.., Saving { loaded: Some(entry), .. }, _)) = &linted
            {
                println!("Loading cache from {}", entry.display());
            }
            let (mut report, corrected, loop_error, warnings, errors, saving, updates) = match linted {
                Ok(l) => l,
                Err(e) => {
                    error = Some(e);
                    stopped = true;
                    break;
                }
            };
            rbcop::autogen::replay(updates);
            // Runner#save_in_cache: nothing is saved for a file that warned, nor once a cop has
            // raised an error or a warning in this run. Saved here, before anything below
            // changes the report; what RuboCop prints in between, it prints only for files it
            // does not save.
            cache_dirty |= saving.errored || !warnings.is_empty() || loop_error.is_some();
            if let (Some(cache), Some(key)) = (&result_cache, &saving.key)
                && !cache_dirty
                && errors.is_empty()
                && report.warnings.is_empty()
                && report.abort.is_none()
            {
                // With -a/-A only a file without offenses (so uncorrected) is known to have the
                // offenses of a plain inspection; RuboCop also saves the others, with their
                // corrections marked uncorrected, against the source before correction.
                cache.save(key, &report, cli.autocorrect.is_none() || report.offenses.is_empty());
            }
            // Lint/RedundantCopDisableDirective's did-you-mean list is the registry of the first
            // file that described an unknown cop name (#599).
            let view = registry_at.get(path).copied();
            for o in
                report.offenses.iter_mut().filter(|o| o.cop == rbcop::cops::lint_redundant_cop_disable_directive::NAME)
            {
                match similar_view {
                    Some(v) if v != view => {
                        if let Some(m) = rbcop::cops::lint_redundant_cop_disable_directive::redescribe(&o.message, v) {
                            o.message = m;
                        }
                    }
                    Some(_) => {}
                    None => {
                        if rbcop::cops::lint_redundant_cop_disable_directive::redescribe(&o.message, view).is_some() {
                            similar_view = Some(view);
                        }
                    }
                }
            }
            // The cop errors, printed as RuboCop's Team prints them while it inspects the file, with
            // the directive warnings where CommentConfig's analysis printed them (runner::investigate).
            for (e, _) in &errors {
                eprintln!("{e}");
            }
            // Runner#collect_problems: the file's cop errors and warnings, for
            // Formatter#file_problems (the directive warnings are neither).
            let mut file_errors: Vec<String> = errors.into_iter().filter(|e| !e.1).map(|e| e.0).collect();
            // Registry#print_department_missing_warning outside that stream (a poisoned or unparsable file).
            for w in &report.warnings {
                eprintln!("{w}");
            }
            // An error raised while the file was inspected ends the run before it is reported
            // (nothing written, no summaries).
            if let Some(e) = report.abort.take() {
                error = Some(e);
                aborted = true;
                stopped = true;
                break;
            }
            // Team#handle_warning prints each cop warning as the file is inspected; Runner#warnings
            // keeps them for the summary, in the same order.
            for w in &warnings {
                eprintln!("{w}");
            }
            cop_warnings_raised.extend(warnings.iter().cloned());
            // Team#autocorrect writes the file before it is reported (with --stdin, keeps the source
            // for printing); a failed write ends the run. Runner#record_diff: under `--diff` the
            // corrections are diffed against the source as read instead.
            if cli.diff {
                for (before, after) in &report.diffs {
                    let diff = rbcop::unified_diff::unified_diff(&cx.smart_path(path), before, after);
                    if !diff.is_empty() {
                        diffs.push(diff);
                    }
                }
            } else if stdin_text.is_some() {
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
                for (e, _) in runner::take_file_errors() {
                    eprintln!("{e}");
                    file_errors.push(e);
                }
            }
            report.offenses.extend(fallback_offenses.remove(path).unwrap_or_default());
            let offs = formatter::offenses(&report.text, &report.offenses, |o| {
                formatter::annotate(cfg, o.cop, &o.message, annotate)
            });
            let passed = !offs.iter().any(|o| considered_failure(o) && displayed(o));
            all_passed &= passed;
            inspected.push(path.clone());
            // Runner#offenses_to_report: the display filters, then OffenseLimit across the run.
            let shown: Vec<_> = offs
                .into_iter()
                .filter(|o| displayed(o))
                .filter(|o| {
                    let Some(max) = cli.max_offenses else { return true };
                    let n = offered.entry(o.cop.to_string()).or_insert(0);
                    *n += 1;
                    *n <= max
                })
                .collect();
            set.file_finished(
                &cx,
                &FileOut { path, offenses: shown, config: cfg, errors: &file_errors, warnings: &warnings },
            );
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
            if cli.debug && !in_parallel {
                println!("Scanning {}", path.display());
            }
            set.file_started(&cx, &path);
            if cli.debug {
                print!("{}", std::mem::take(&mut loading[jobs.len()]));
            }
            config_failure = Some(e);
        }
    }
    // Runner#inspect_files' ensure: pruning is skipped for a single file (an editor's run).
    if files.len() > 1
        && let Some(cache) = &result_cache
    {
        cache.cleanup(cli.debug);
    }
    set.finished(&cx, &inspected);
    if let Some(e) = set.take_error() {
        error.get_or_insert(e);
    }
    // Runner#warn_about_elided_offenses, in Runner#run's ensure.
    if let Some(max) = cli.max_offenses {
        let elided: Vec<(&String, usize)> =
            offered.iter().filter(|(_, n)| **n > max).map(|(c, n)| (c, n - max)).collect();
        if !elided.is_empty() {
            let total: usize = elided.iter().map(|(_, n)| n).sum();
            let cops: Vec<&str> = elided.iter().map(|(c, _)| c.as_str()).collect();
            eprintln!(
                "{}",
                formatter::Paint(global).yellow(&format!(
                    "{total} more offenses were not reported because of --max-offenses-per-cop={max} ({}).",
                    cops.join(", ")
                ))
            );
        }
    }
    // The ValidationError reaches the CLI before ExecuteRunner's error summary.
    if let Some(e) = config_failure {
        warn_red(global, &e);
        return Ok(ERROR);
    }
    // An error raised out of Runner#run skips ExecuteRunner#display_summary.
    if aborted && let Some(e) = error {
        eprintln!("{}", red(global, e.trim_end()));
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
    let integration =
        matches!(cli.format.as_deref(), Some("h" | "html" | "j" | "json" | "ju" | "junit" | "sa" | "sarif"));
    // ExecuteRunner#print_diffs: a blank line, then each diff, to --stderr's stream; not for the
    // integration formatters. Diffs mean the run has not passed.
    if cli.diff {
        if !diffs.is_empty() && !integration {
            let text = format!("\n{}", diffs.concat());
            if cli.stderr {
                eprint!("{text}");
            } else {
                print!("{text}");
            }
        }
        all_passed &= diffs.is_empty();
    }
    // ExecuteRunner#maybe_print_corrected_source: the separator follows --stderr, the source is
    // printed to stdout either way; integration formatters get neither.
    else if let (Some(_), Some(original)) = (cli.autocorrect, stdin_text)
        && !integration
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
    // `all_passed || @options[:auto_gen_config]`
    all_passed |= phase.is_some();
    let mut status = if all_passed && cop_errors.is_empty() { 0 } else { OFFENSES };
    // ExecuteRunner#merge_todo_audit_status
    if cli.report_unused_todo {
        let names: Vec<&'static str> = cops::ALL.iter().map(|c| c.name).chain(["Lint/Syntax"]).collect();
        let audit = report_unused_todo_entries(&mut store, &cwd, &names, &badge_selected, global)?;
        if status == 0 {
            status = audit;
        }
    }
    if phase.is_some() {
        return Ok(status);
    }
    // CLI#execute_runners: SuggestExtensions follows ExecuteRunner.
    let suggestions = suggest_extensions(&cli, &pwd_config, &store.default_config(), store.loaded_plugins());
    if cli.stderr {
        eprint!("{suggestions}");
    } else {
        print!("{suggestions}");
    }
    Ok(status)
}

/// `--report-unused-todo-entries`: TodoAudit. Each `Exclude` entry of `.rubocop_todo.yml` whose
/// file the cop no longer flags (inspected with the todo's exclusions taken out), or that is not a
/// file, is listed on stderr, and the run fails.
fn report_unused_todo_entries(
    store: &mut Store,
    cwd: &Path,
    names: &[&'static str],
    selected: &dyn Fn(&str, &Resolved) -> bool,
    global: bool,
) -> Result<u8, String> {
    use rbcop::config::value::{Map, Value};
    let paint = formatter::Paint(global);
    let todo = cwd.join(AUTO_GENERATED_FILE);
    if !todo.exists() {
        let msg = paint.yellow(&format!("No `{AUTO_GENERATED_FILE}` found; nothing to audit."));
        eprintln!("{msg}");
        return Ok(0);
    }
    let dir = todo.parent().unwrap_or(cwd).to_path_buf();
    let text = fs::read_to_string(&todo).map_err(|e| runner::io_error(&e, &todo))?;
    let yaml = rbcop::config::value::parse_yaml_file(&text, &todo.display().to_string())?;
    // todo_exclude_entries: (cop, path as written), for registered cops with an Exclude list.
    // Deliberate difference: only cops rbcop implements are audited (it cannot tell whether another
    // cop still flags a file).
    let implemented = |c: &str| names.contains(&c);
    let mut entries: Vec<(&'static str, String)> = Vec::new();
    if let Value::Map(m) = &yaml {
        for (cop, cfg) in &m.0 {
            let Some(cop) = names.iter().copied().find(|n| n == cop).filter(|c| c.contains('/') && implemented(c))
            else {
                continue;
            };
            let Some(Value::Seq(items)) = cfg.as_map().and_then(|c| c.get("Exclude")) else { continue };
            entries.extend(items.iter().filter_map(|i| match i {
                Value::Str(s) => Some((cop, s.clone())),
                _ => None,
            }));
        }
    }
    let absolute = |p: &str| expand_path(Path::new(p), &dir);
    let mut files: Vec<PathBuf> = Vec::new();
    for (_, p) in &entries {
        let f = absolute(p);
        if !files.contains(&f) && f.is_file() {
            files.push(f);
        }
    }
    let mut offending: std::collections::HashMap<PathBuf, std::collections::HashSet<&'static str>> = Default::default();
    for file in files {
        let config = store.for_file(&file)?;
        // audit_config: the todo exclusions removed for the audited cops.
        let mut hash = config.hash.clone();
        for (cop, _) in &entries {
            let Some(Value::Map(cop_cfg)) = hash.get(cop).cloned() else { continue };
            let Some(Value::Seq(excludes)) = cop_cfg.get("Exclude") else { continue };
            let removals: Vec<Value> = entries
                .iter()
                .filter(|(c, _)| c == cop)
                .map(|(_, p)| Value::Str(absolute(p).display().to_string()))
                .collect();
            let kept: Vec<Value> = excludes.iter().filter(|e| !removals.contains(e)).cloned().collect();
            let mut new: Map = (*cop_cfg).clone();
            new.insert("Exclude", Value::Seq(std::sync::Arc::new(kept)));
            hash.insert(*cop, Value::Map(std::sync::Arc::new(new)));
        }
        let audited = config.with_hash(hash, names);
        let bytes = fs::read(&file).map_err(|e| runner::io_error(&e, &file))?;
        let display = rbcop::config::relative_path(&file, cwd);
        let report = runner::inspect_bytes(&file, &display, bytes, &audited, &|cop| selected(cop, &audited));
        let _ = runner::take_file_errors();
        let _ = runner::take_file_warnings();
        let _ = rbcop::autogen::take_file();
        offending.insert(file, report.offenses.iter().filter(|o| !o.disabled).map(|o| o.cop).collect());
    }
    let unused: Vec<&(&'static str, String)> =
        entries.iter().filter(|(cop, p)| !offending.get(&absolute(p)).is_some_and(|s| s.contains(cop))).collect();
    if unused.is_empty() {
        return Ok(0);
    }
    let noun = if unused.len() == 1 { "entry" } else { "entries" };
    eprintln!("{}", paint.red(&format!("\n{} unused todo {noun} found in `{AUTO_GENERATED_FILE}`:", unused.len())));
    for (cop, p) in unused {
        eprintln!("  {cop}: {p}");
    }
    Ok(OFFENSES)
}
