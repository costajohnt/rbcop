//! rubocop-packaging's LibHelperModule.

use crate::prelude::*;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// `RuboCop::ConfigFinder.project_root` as a string: "" when there is none (`"#{nil}/lib"`).
pub fn root_dir() -> &'static str {
    static ROOT: OnceLock<String> = OnceLock::new();
    ROOT.get_or_init(|| {
        crate::config::project_root(&crate::config::cwd()).map_or(String::new(), |r| r.to_string_lossy().into_owned())
    })
}

/// The inspected file (`processed_source.file_path`) and its directory.
pub struct Inspected {
    pub path: PathBuf,
    pub dir: PathBuf,
}

impl Inspected {
    pub fn new(cx: &Cx) -> Inspected {
        let path = PathBuf::from(cx.src.absolute_path());
        let dir = path.parent().map_or_else(|| path.clone(), Path::to_path_buf);
        Inspected { path, dir }
    }

    /// `inspected_file_is_not_in_lib_or_gemspec?`
    pub fn not_in_lib_or_gemspec(&self) -> bool {
        let p = self.path.as_os_str().as_bytes();
        !p.starts_with(format!("{}/lib", root_dir()).as_bytes()) && !p.ends_with(b"gemspec")
    }
}

/// `File.expand_path(str, base).start_with?("#{root_dir}/lib")`.
// ponytail: no `~` (home) expansion; a require path starting with `~` is not seen as lib.
pub fn falls_in_lib(s: &[u8], base: &Path) -> bool {
    expand(s, base).as_os_str().as_bytes().starts_with(format!("{}/lib", root_dir()).as_bytes())
}

/// `File.expand_path(str, base)`
pub fn expand(s: &[u8], base: &Path) -> PathBuf {
    crate::config::expand_path(Path::new(std::ffi::OsStr::from_bytes(s)), base)
}

/// The value of a parser `str` node.
pub fn str_value(node: &Node) -> Option<Vec<u8>> {
    node.as_string_node().filter(|_| !is_parser_dstr(node)).map(|s| s.unescaped().to_vec())
}

/// `(send nil? :name <one argument>)`: the argument.
pub fn single_arg<'pr>(call: &ruby_prism::CallNode<'pr>, name: &str) -> Option<Node<'pr>> {
    if call.receiver().is_some() || method_name(call) != name {
        return None;
    }
    match <[Node; 1]>::try_from(send_args(call)) {
        Ok([arg]) => Some(arg),
        Err(_) => None,
    }
}
