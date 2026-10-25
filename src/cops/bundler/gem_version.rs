use crate::helpers::gem_declaration::{
    includes_commit_reference, includes_version_specification, is_gem_declaration, str_value,
};
use crate::prelude::*;

pub const NAME: &str = "Bundler/GemVersion";
pub static COP: GemVersion = GemVersion;
pub struct GemVersion;

impl Cop for GemVersion {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if !is_gem_declaration(&call) {
            return;
        }
        let name = str_value(&send_args(&call)[0]).unwrap();
        if cx.cfg.strings("AllowedGems").iter().any(|g| g.as_bytes() == name.as_slice()) {
            return;
        }
        let specified = includes_version_specification(&call) || includes_commit_reference(&call);
        let msg = match cx.cfg.str("EnforcedStyle") {
            Some("required") if !specified => "Gem version specification is required.",
            Some("forbidden") if specified => "Gem version specification is forbidden.",
            _ => return cx.correct_style_detected(),
        };
        let (s, e) = send_range(&call);
        cx.add(s, e, msg);
        cx.opposite_style_detected();
    }
}
