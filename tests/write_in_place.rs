//! autocorrect::write_source's in-place path (a hard-linked file) writes the new content before
//! cutting the file to its length, so a write that fails part way never leaves it truncated (#732).
//! Alone in its test binary: RLIMIT_FSIZE is per process.

use std::fs;

#[test]
fn failed_in_place_write_does_not_truncate() {
    let dir = std::env::temp_dir().join(format!("rbcop-write-in-place-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("a.rb");
    let old = format!("{}\n", "a".repeat(99));
    fs::write(&file, &old).unwrap();
    fs::hard_link(&file, dir.join("b.rb")).unwrap();
    let roots = [fs::canonicalize(&dir).unwrap()];
    // Writes past byte 50 fail with EFBIG, as a full disk would fail them.
    // SAFETY: plain libc calls on a local struct; nothing else in this process writes files meanwhile.
    let saved = unsafe {
        libc::signal(libc::SIGXFSZ, libc::SIG_IGN);
        let mut saved = std::mem::zeroed::<libc::rlimit>();
        assert_eq!(libc::getrlimit(libc::RLIMIT_FSIZE, &mut saved), 0);
        let limit = libc::rlimit { rlim_cur: 50, rlim_max: saved.rlim_max };
        assert_eq!(libc::setrlimit(libc::RLIMIT_FSIZE, &limit), 0);
        saved
    };
    let written = rbcop::autocorrect::write_source(&file, &"b".repeat(200), None, &roots);
    // SAFETY: as above.
    unsafe { assert_eq!(libc::setrlimit(libc::RLIMIT_FSIZE, &saved), 0) };
    assert!(written.is_err());
    assert_eq!(fs::metadata(&file).unwrap().len(), old.len() as u64, "file truncated by a failed write");
    fs::remove_dir_all(&dir).unwrap();
}
