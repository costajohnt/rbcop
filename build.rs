//! Generates the cop registry from src/cops/<department>/<cop>.rs so cops never edit a shared list.
//! Each cop file must define `pub const NAME: &str` and `pub static COP: <type>` implementing `Cop`.

use std::fs;
use std::path::Path;

fn main() {
    let root = Path::new("src/cops");
    println!("cargo:rerun-if-changed=src/cops");
    let mut mods = String::new();
    let mut entries = Vec::new();
    let mut depts: Vec<_> = fs::read_dir(root).unwrap().flatten().filter(|e| e.path().is_dir()).collect();
    depts.sort_by_key(|e| e.file_name());
    for dept in depts {
        println!("cargo:rerun-if-changed={}", dept.path().display());
        let mut files: Vec<_> = fs::read_dir(dept.path())
            .unwrap()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "rs"))
            .collect();
        files.sort_by_key(|e| e.file_name());
        for f in files {
            let stem = f.path().file_stem().unwrap().to_string_lossy().into_owned();
            let module = format!("{}_{}", dept.file_name().to_string_lossy(), stem);
            let abs = fs::canonicalize(f.path()).unwrap();
            mods.push_str(&format!("#[path = {:?}]\npub mod {module};\n", abs.display().to_string()));
            entries.push(module);
        }
    }
    let mut out = mods;
    out.push_str("\n/// Every implemented cop, sorted by department then file name.\npub static ALL: &[crate::cop::Registered] = &[\n");
    for m in &entries {
        out.push_str(&format!("    crate::cop::Registered {{ name: {m}::NAME, cop: &{m}::COP }},\n"));
    }
    out.push_str("];\n");
    let dest = Path::new(&std::env::var("OUT_DIR").unwrap()).join("cops.rs");
    fs::write(dest, out).unwrap();
}
