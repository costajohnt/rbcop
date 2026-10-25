//! Remote `inherit_from` URLs (RuboCop's RemoteConfig, run by ruby), served by a local HTTP
//! server. Expected output recorded with RuboCop 1.91.0 against the same files.

mod common;

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::sync::{Arc, Mutex};

type Log = Arc<Mutex<Vec<String>>>;

/// Serves `files`; logs each request's path and whether it sent If-Modified-Since. `/r/redir`
/// redirects to `/r/two.yml`, `/loop` to itself, `/old` answers 304 to If-Modified-Since, any
/// other path not in `files` 404.
fn serve(files: &'static [(&'static str, &'static str)]) -> (u16, Log) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let log = Log::default();
    let seen = log.clone();
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut head = Vec::new();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                head.push(line.trim_end().to_string());
            }
            let path = head.first().and_then(|l| l.split(' ').nth(1)).unwrap_or("").to_string();
            let ims = head.iter().any(|l| l.to_ascii_lowercase().starts_with("if-modified-since:"));
            seen.lock().unwrap().push(format!("{path} {ims}"));
            let (status, extra, body) = match path.as_str() {
                "/r/redir" => ("302 Found", "Location: /r/two.yml\r\n", ""),
                "/loop" => ("302 Found", "Location: /loop\r\n", ""),
                "/old" if ims => ("304 Not Modified", "", ""),
                p => match files.iter().find(|(f, _)| *f == p) {
                    Some((_, body)) => ("200 OK", "", *body),
                    None => ("404 Not Found", "", ""),
                },
            };
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    (port, log)
}

/// The file is downloaded into $RUBOCOP_CACHE_ROOT/rubocop_cache/<name>-<md5 of the URL>.yml and
/// used as it is for a day, then asked for again with If-Modified-Since (a 304 keeps it).
/// Redirects are followed. A relative inherit_from inside a remote file resolves against its URL
/// and, as in RuboCop (inherit_from_remote rewrites the URI path it shares with the remote
/// file's own), a second relative entry resolves against the first one's: `other.yml` is
/// `/r/sub/other.yml`. Errors are RuboCop's messages (RuboCop prints a backtrace after them),
/// with the URL's credentials left out.
#[test]
fn remote_inherit_from() {
    static FILES: &[(&str, &str)] = &[
        ("/r/two.yml", "inherit_from:\n  - sub/more.yml\n  - other.yml\n"),
        ("/r/sub/more.yml", "Layout/LineLength:\n  Max: 10\n"),
        ("/r/sub/other.yml", "Style/StringLiterals:\n  EnforcedStyle: double_quotes\n"),
        ("/r/other.yml", "Style/StringLiterals:\n  Enabled: false\n"),
        ("/old", "Layout/LineLength:\n  Max: 12\n"),
    ];
    let (port, log) = serve(FILES);
    let dir = std::env::temp_dir().join(format!("rbcop-remote-config-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let dir = dir.canonicalize().unwrap();
    let cache = dir.join("cache/rubocop_cache");
    fs::write(dir.join("Gemfile"), "").unwrap();
    fs::write(dir.join("a.rb"), "x = 'abcdefghijklmnop'\n").unwrap();
    let run = |url: &str| {
        let config = format!("AllCops:\n  NewCops: disable\ninherit_from: http://u:p@127.0.0.1:{port}{url}\n");
        fs::write(dir.join(".rubocop.yml"), config).unwrap();
        let out = common::command(env!("CARGO_BIN_EXE_rbcop"))
            .args(["--only", "Layout/LineLength,Style/StringLiterals", "-f", "e", "a.rb"])
            .current_dir(&dir)
            .env("RUBOCOP_CACHE_ROOT", dir.join("cache"))
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout).replace(&format!("{}/", dir.display()), "");
        (out.status.code(), stdout, String::from_utf8_lossy(&out.stderr).into_owned())
    };
    let take = || std::mem::take(&mut *log.lock().unwrap());
    let want = "a.rb:1:5: C: [Correctable] Style/StringLiterals: Prefer double-quoted strings unless you need \
                single quotes to avoid extra backslashes for escaping.\n\
                a.rb:1:11: C: Layout/LineLength: Line is too long. [22/10]\n";
    for url in ["/r/two.yml", "/r/redir"] {
        assert_eq!(run(url), (Some(1), want.to_string(), String::new()), "{url}");
    }
    assert_eq!(
        take(),
        ["/r/two.yml false", "/r/sub/more.yml false", "/r/sub/other.yml false", "/r/redir false", "/r/two.yml false"]
    );
    let mut names: Vec<String> =
        fs::read_dir(&cache).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    let stems: Vec<&str> =
        names.iter().map(|n| &n[..n.len() - "-0123456789abcdef0123456789abcdef.yml".len()]).collect();
    assert_eq!(stems, ["more", "other", "redir", "two"]);

    // Within a day the cached files are used as they are.
    assert_eq!(run("/r/two.yml").1, want);
    assert_eq!(take(), Vec::<String>::new());

    // An older one is asked for again; the 304 keeps it.
    assert_eq!(run("/old").1, "a.rb:1:13: C: Layout/LineLength: Line is too long. [22/12]\n");
    let old = fs::read_dir(&cache)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.file_name().unwrap().to_string_lossy().starts_with("old-"))
        .unwrap();
    fs::write(&old, "Layout/LineLength:\n  Max: 15\n").unwrap();
    let two_days_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(2 * 24 * 60 * 60);
    fs::File::options().write(true).open(&old).unwrap().set_modified(two_days_ago).unwrap();
    assert_eq!(run("/old").1, "a.rb:1:16: C: Layout/LineLength: Line is too long. [22/15]\n");
    assert_eq!(take(), ["/old false", "/old true"]);

    let (code, _, stderr) = run("/nope.yml");
    assert_eq!(code, Some(2));
    let want = format!("404 \"Not Found\" while downloading remote config file http://127.0.0.1:{port}/nope.yml\n");
    assert!(stderr.starts_with(&want), "{stderr}");
    let (code, _, stderr) = run("/loop");
    assert_eq!(code, Some(2));
    assert!(stderr.starts_with("HTTP redirect too deep\n"), "{stderr}");
    let _ = fs::remove_dir_all(&dir);
}
