use crate::helpers::packaging::{Inspected, expand, falls_in_lib, root_dir, single_arg, str_value};
use crate::prelude::*;
use std::os::unix::ffi::OsStrExt;

pub const NAME: &str = "Packaging/RequireRelativeHardcodingLib";
pub static COP: RequireRelativeHardcodingLib = RequireRelativeHardcodingLib;
pub struct RequireRelativeHardcodingLib;

impl Cop for RequireRelativeHardcodingLib {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn restrict_on_send(&self) -> &'static [&'static str] {
        &["require_relative"]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send nil? :require_relative (str #falls_in_lib?))
        let call = node.as_call_node().unwrap();
        if is_csend(&call) {
            return;
        }
        let Some(value) = single_arg(&call, "require_relative").and_then(|a| str_value(&a)) else { return };
        let file = Inspected::new(cx);
        if !falls_in_lib(&value, &file.dir) || !file.not_in_lib_or_gemspec() {
            return;
        }
        let expanded = expand(&value, &file.dir);
        let expanded = expanded.as_os_str().as_bytes();
        let prefix = format!("{}/lib/", root_dir());
        let good = String::from_utf8_lossy(expanded.strip_prefix(prefix.as_bytes()).unwrap_or(expanded)).into_owned();
        let (s, e) = send_range(&call);
        cx.add_fix(
            s,
            e,
            "Avoid using `require_relative` with relative path to lib. Use `require` with absolute path instead.",
            |c| c.replace(s, e, &format!("require \"{good}\"")),
        );
    }
}
