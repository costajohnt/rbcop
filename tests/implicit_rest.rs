//! A trailing comma (`a, = x`, `|a, b,|`) is a Prism ImplicitRestNode but no parser node, so cops
//! must see `|x,|` as `(args (arg :x))` and `a, b, = x` as an mlhs ending at `b`.

use std::fs;
use std::process::Command;

#[test]
fn trailing_comma_is_no_parser_argument() {
    let dir = std::env::temp_dir().join(format!("rbcop-implicit-rest-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("Gemfile"), "").unwrap();
    let src = "arr.sort_by { |x,| x }\nhash.each { |k, v,| p v }\na, b, = arr.each do |x|\n  x\n  end\n";
    fs::write(dir.join("t.rb"), src).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_rbcop"))
        .args([
            "--force-default-config",
            "--only",
            "Style/RedundantSortBy,Style/HashEachMethods,Layout/BlockAlignment",
            "-f",
            "emacs",
            "t.rb",
        ])
        .current_dir(&dir)
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    for want in [
        "t.rb:1:5: C: [Correctable] Style/RedundantSortBy: Use `sort` instead of `sort_by { |x| x }`.",
        "t.rb:2:1: C: [Correctable] Style/HashEachMethods: Use `each_value` instead of `each` and remove the unused `k` block argument.",
        "t.rb:5:3: C: [Correctable] Layout/BlockAlignment: `end` at 5, 2 is not aligned with `a, b` at 3, 0.",
    ] {
        assert!(stdout.contains(want), "missing {want}\n{stdout}");
    }
    let _ = fs::remove_dir_all(&dir);
}
