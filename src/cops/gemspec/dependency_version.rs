use crate::helpers::gem_declaration::{includes_commit_reference, includes_version_specification, str_value};
use crate::helpers::gemspec_help::{first_spec_var, lvar_name};
use crate::prelude::*;

pub const NAME: &str = "Gemspec/DependencyVersion";
pub static COP: DependencyVersion = DependencyVersion;
pub struct DependencyVersion;

/// Block variable of the file's first `Gem::Specification.new do |x|`.
#[derive(Default)]
struct State(Option<Vec<u8>>);

impl Cop for DependencyVersion {
    fn kinds(&self) -> &'static [usize] {
        &[kind::PROGRAM_NODE, kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = node.as_call_node() else {
            cx.state::<State>().0 = first_spec_var(node).map(<[u8]>::to_vec);
            return;
        };
        // (send (lvar #match_block_variable_name?) #add_dependency_method? ...)
        if is_csend(&call)
            || !matches!(method_name(&call), "add_dependency" | "add_runtime_dependency" | "add_development_dependency")
        {
            return;
        }
        let Some(recv) = call.receiver().and_then(|r| lvar_name(&r, &cx.src.numbered_sends)) else { return };
        if cx.state::<State>().0.as_deref() != Some(recv) {
            return;
        }
        // `node.first_argument.str_content` raises without arguments.
        let args = send_args(&call);
        let Some(first) = args.first() else { return };
        if let Some(name) = str_value(first)
            && cx.cfg.strings("AllowedGems").iter().any(|g| g.as_bytes() == name.as_slice())
        {
            return;
        }
        let specified = includes_version_specification(&call) || includes_commit_reference(&call);
        let msg = match cx.cfg.str("EnforcedStyle") {
            Some("required") if !specified => "Dependency version specification is required.",
            Some("forbidden") if specified => "Dependency version specification is forbidden.",
            _ => return cx.correct_style_detected(),
        };
        let (s, e) = send_range(&call);
        cx.add(s, e, msg);
        cx.opposite_style_detected();
    }
}
