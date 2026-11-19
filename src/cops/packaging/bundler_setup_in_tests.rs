use crate::helpers::packaging::{Inspected, root_dir, single_arg, str_value};
use crate::prelude::*;
use std::os::unix::ffi::OsStrExt;

pub const NAME: &str = "Packaging/BundlerSetupInTests";
pub static COP: BundlerSetupInTests = BundlerSetupInTests;
pub struct BundlerSetupInTests;

impl Cop for BundlerSetupInTests {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn restrict_on_send(&self) -> &'static [&'static str] {
        &["require"]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send nil? :require (str #bundler_setup_in_test_dir?))
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || single_arg(&call, "require").is_none_or(|a| str_value(&a).as_deref() != Some(&b"bundler/setup"[..])) {
            return;
        }
        let dir = Inspected::new(cx).dir;
        let dir = dir.as_os_str().as_bytes();
        if !["spec", "specs", "test", "tests"].iter().any(|d| dir.starts_with(format!("{}/{d}", root_dir()).as_bytes())) {
            return;
        }
        let (s, e) = send_range(&call);
        let (ws, we) = helpers::whole_lines::range_by_whole_lines(cx.src.text, s, e, true);
        cx.add_fix(s, e, "Using `bundler/setup` in tests is redundant. Consider removing it.", |c| c.remove(ws, we));
    }
}
