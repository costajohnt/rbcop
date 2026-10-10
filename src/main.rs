mod cops;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::{env, fs, thread};

// AllCops/Include from RuboCop's default.yml.
const EXTENSIONS: &[&str] = &[
    "rb",
    "arb",
    "axlsx",
    "builder",
    "gemfile",
    "gemspec",
    "jb",
    "jbuilder",
    "mspec",
    "opal",
    "pluginspec",
    "podspec",
    "rabl",
    "rake",
    "rbw",
    "ru",
    "ruby",
    "schema",
    "spec",
    "thor",
];
const FILENAMES: &[&str] = &[
    ".irbrc",
    ".pryrc",
    ".simplecov",
    "buildfile",
    "Appraisals",
    "Berksfile",
    "Brewfile",
    "Buildfile",
    "Capfile",
    "Dangerfile",
    "Deliverfile",
    "Fastfile",
    "Gemfile",
    "Guardfile",
    "Jarfile",
    "Mavenfile",
    "Podfile",
    "Puppetfile",
    "Rakefile",
    "rakefile",
    "Schemafile",
    "Snapfile",
    "Steepfile",
    "Thorfile",
    "Vagrantfile",
];
// AllCops/Exclude, relative to the directory being inspected.
const EXCLUDED_DIRS: &[&str] = &["node_modules", "tmp", "vendor", ".git"];

const USAGE: &str = "usage: rbcop [--only Cop1,Cop2] [--format emacs] [paths...]";

fn main() -> ExitCode {
    let mut only: Option<Vec<String>> = None;
    let mut list_files = false;
    let mut roots = Vec::new();
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--only" => only = args.next().map(|v| v.split(',').map(String::from).collect()),
            // emacs is the only format so far; accepted so commands match rubocop's.
            "--format" | "-f" => {
                args.next();
            }
            // Like rubocop -L: print the files that would be inspected.
            "-L" | "--list-files" => list_files = true,
            "--list-cops" => {
                cops::COPS.iter().for_each(|c| println!("{}", c.name));
                return ExitCode::SUCCESS;
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            a if a.starts_with('-') => {
                eprintln!("unknown option {a}\n{USAGE}");
                return ExitCode::from(2);
            }
            _ => roots.push(PathBuf::from(arg)),
        }
    }
    if roots.is_empty() {
        roots.push(PathBuf::from("."));
    }

    let mut files = Vec::new();
    for root in &roots {
        if root.is_dir() {
            walk(root, root, &mut files);
        } else {
            files.push(root.clone());
        }
    }
    files.sort();
    if list_files {
        files.iter().for_each(|f| println!("{}", f.display()));
        return ExitCode::SUCCESS;
    }

    let on = |cop: &str| only.as_ref().is_none_or(|o| o.iter().any(|c| c == cop));
    let workers = thread::available_parallelism().map_or(4, |n| n.get());
    let chunk = files.len().div_ceil(workers).max(1);
    let reports: Vec<String> = thread::scope(|s| {
        let handles: Vec<_> = files
            .chunks(chunk)
            .map(|part| s.spawn(|| part.iter().map(|f| inspect(f, &on)).collect::<Vec<_>>()))
            .collect();
        handles.into_iter().flat_map(|h| h.join().unwrap()).collect()
    });

    let mut found = false;
    for report in reports {
        found |= !report.is_empty();
        print!("{report}");
    }
    if found { ExitCode::from(1) } else { ExitCode::SUCCESS }
}

/// Lint one file and render its offenses in RuboCop's emacs format.
fn inspect(path: &Path, on: &dyn Fn(&str) -> bool) -> String {
    let Ok(bytes) = fs::read(path) else {
        eprintln!("rbcop: cannot read {}", path.display());
        return String::new();
    };
    let Ok(text) = String::from_utf8(bytes) else {
        eprintln!("rbcop: skipping {} (not UTF-8)", path.display());
        return String::new();
    };
    // parser's Source::Buffer normalizes CRLF before any cop sees the source.
    let text = text.replace("\r\n", "\n");
    let Some(mut offenses) = cops::check(&text, on) else {
        eprintln!("rbcop: skipping {} (syntax error)", path.display());
        return String::new();
    };
    offenses.sort_by(|a, b| (a.line, a.col, a.cop).cmp(&(b.line, b.col, b.cop)));
    let shown = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut out = String::new();
    for o in offenses {
        let cop = cops::COPS.iter().find(|c| c.name == o.cop).unwrap();
        let tag = if cop.correctable { "[Correctable] " } else { "" };
        out += &format!("{}:{}:{}: {}: {tag}{}: {}\n", shown.display(), o.line, o.col, cop.severity, o.cop, o.msg);
    }
    out
}

/// TargetFinder#ruby_executable?: extensionless file whose shebang names a RubyInterpreters entry.
fn ruby_executable(path: &Path) -> bool {
    use std::io::{BufRead, BufReader};
    if path.extension().is_some() {
        return false;
    }
    let mut first = String::new();
    let Ok(file) = fs::File::open(path) else {
        return false;
    };
    if BufReader::new(file).read_line(&mut first).is_err() || !first.starts_with("#!") {
        return false;
    }
    first.split(|c: char| !c.is_alphanumeric()).any(|w| matches!(w, "ruby" | "rake" | "jruby"))
}

/// Collect Ruby files under `dir` the way RuboCop's TargetFinder does with default config.
fn walk(root: &Path, dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            if !(dir == root && EXCLUDED_DIRS.contains(&name.as_str())) {
                walk(root, &path, files);
            }
        } else if kind.is_file()
            && (FILENAMES.contains(&name.as_str())
                || name.ends_with("Fastfile")
                || path.extension().is_some_and(|e| EXTENSIONS.contains(&e.to_string_lossy().as_ref()))
                || ruby_executable(&path))
        {
            files.push(path);
        }
    }
}
