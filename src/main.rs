use rbcop::config::{Options, Store, relative_path};
use rbcop::formatter::{self, Inspected};
use rbcop::runner::{self, FileReport};
use rbcop::{cops, targets};
use std::path::PathBuf;
use std::process::ExitCode;
use std::{env, fs, thread};

const USAGE: &str = "usage: rbcop [options] [file1, file2, ...]

  -c, --config FILE              Specify configuration file.
      --force-default-config     Use default configuration even if a config file exists.
      --only Cop1,Cop2           Run only the given cops.
      --except Cop1,Cop2         Exclude the given cops.
  -f, --format FORMATTER         emacs
  -L, --list-target-files        List all files rbcop will inspect.
      --show-cops                List implemented cops.
      --force-exclusion          Apply AllCops/Exclude to files passed explicitly.
      --ignore-parent-exclusion  Do not inherit AllCops/Exclude from parent .rubocop.yml files.
      --enable-pending-cops      Run cops marked `Enabled: pending`.
      --disable-pending-cops     Never run pending cops.
  -a, --autocorrect              Autocorrect offenses (only when it's safe).
  -A, --autocorrect-all          Autocorrect offenses (safe and unsafe).
  -v, --version                  Show the RuboCop version rbcop matches.";

struct Cli {
    opts: Options,
    except: Vec<String>,
    format: String,
    list_targets: bool,
    force_exclusion: bool,
    paths: Vec<PathBuf>,
    /// -a / -A
    autocorrect: Option<rbcop::autocorrect::Mode>,
}

fn parse_args() -> Result<Option<Cli>, String> {
    let mut cli = Cli {
        opts: Options::default(),
        except: Vec::new(),
        format: "emacs".into(),
        list_targets: false,
        force_exclusion: false,
        paths: Vec::new(),
        autocorrect: None,
    };
    let mut args = env::args().skip(1);
    let list = |v: Option<String>| v.map(|v| v.split(',').map(|s| s.trim().to_string()).collect()).unwrap_or_default();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--only" => cli.opts.only = list(args.next()),
            "--except" => cli.except = list(args.next()),
            "-c" | "--config" => cli.opts.config_file = args.next().map(PathBuf::from),
            "--force-default-config" => cli.opts.force_default_config = true,
            "-f" | "--format" => cli.format = args.next().ok_or("--format needs a value")?,
            "-L" | "--list-target-files" => cli.list_targets = true,
            "--force-exclusion" => cli.force_exclusion = true,
            "--ignore-parent-exclusion" => cli.opts.ignore_parent_exclusion = true,
            "--enable-pending-cops" => cli.opts.enable_pending_cops = true,
            "--disable-pending-cops" => cli.opts.disable_pending_cops = true,
            // autocorrect
            "-a" | "--autocorrect" => cli.autocorrect = Some(rbcop::autocorrect::Mode::Safe),
            "-A" | "--autocorrect-all" => cli.autocorrect = Some(rbcop::autocorrect::Mode::All),
            "--show-cops" => {
                cops::ALL.iter().for_each(|c| println!("{}", c.name));
                return Ok(None);
            }
            "-v" | "--version" => {
                println!("{}", rbcop::config::RUBOCOP_VERSION);
                return Ok(None);
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(None);
            }
            a if a.starts_with('-') => return Err(format!("invalid option: {a}")),
            _ => cli.paths.push(PathBuf::from(arg)),
        }
    }
    if !formatter::FORMATS.contains(&cli.format.as_str()) {
        return Err(format!("unknown formatter {}", cli.format));
    }
    Ok(Some(cli))
}

fn main() -> ExitCode {
    let cli = match parse_args() {
        Ok(Some(cli)) => cli,
        Ok(None) => return ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("rbcop: {e}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match run(cli) {
        Ok(true) => ExitCode::from(1),
        Ok(false) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("rbcop: {e}");
            ExitCode::from(2)
        }
    }
}

/// Returns whether any offense was found.
fn run(cli: Cli) -> Result<bool, String> {
    let names: Vec<&'static str> = cops::ALL.iter().map(|c| c.name).collect();
    let mut store = Store::new(cli.opts.clone(), names)?;
    let files = targets::find(&mut store, &cli.paths, cli.force_exclusion)?;
    let cwd = store.cwd().to_path_buf();
    if cli.list_targets {
        for f in &files {
            println!("{}", relative_path(f, &cwd));
        }
        return Ok(false);
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
    let reports: Vec<FileReport> = thread::scope(|s| {
        let handles: Vec<_> = jobs
            .chunks(chunk)
            .map(|part| {
                s.spawn(|| {
                    part.iter()
                        .map(|(path, cfg)| {
                            // autocorrect: reads, corrects and rewrites the file; offenses come sorted.
                            if let Some(mode) = cli.autocorrect {
                                let display = relative_path(path, &cwd);
                                return rbcop::autocorrect::correct_file(path, &display, cfg, &selected, mode);
                            }
                            let text =
                                fs::read(path).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
                            let display = relative_path(path, &cwd);
                            let mut r = runner::inspect(path, &display, text, cfg, &selected);
                            r.offenses.sort_by(|a, b| (a.start, a.cop).cmp(&(b.start, b.cop)));
                            r
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles.into_iter().flat_map(|h| h.join().unwrap()).collect()
    });
    let inspected: Vec<Inspected> = files
        .iter()
        .zip(&reports)
        .map(|(path, report)| Inspected { path, relative: relative_path(path, &cwd), report })
        .collect();
    print!("{}", formatter::emacs(&inspected));
    // Corrected offenses do not fail the run (Runner#considered_failure?).
    Ok(reports.iter().any(|r| r.offenses.iter().any(|o| !o.corrected)))
}
