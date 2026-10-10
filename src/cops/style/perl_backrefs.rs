use crate::prelude::*;

pub const NAME: &str = "Style/PerlBackrefs";
pub static COP: PerlBackrefs = PerlBackrefs;
pub struct PerlBackrefs;

impl Cop for PerlBackrefs {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::BACK_REFERENCE_READ_NODE, kind::NUMBERED_REFERENCE_READ_NODE, kind::GLOBAL_VARIABLE_READ_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let original = node_text(node);
        let preferred = if let Some(n) = node.as_numbered_reference_read_node() {
            format!("Regexp.last_match({})", n.number())
        } else {
            match original {
                "$&" | "$MATCH" => "Regexp.last_match(0)".into(),
                "$`" | "$PREMATCH" => "Regexp.last_match.pre_match".into(),
                "$'" | "$POSTMATCH" => "Regexp.last_match.post_match".into(),
                _ => return,
            }
        };
        let in_class = cx.ancestors.iter().any(|a| matches!(kind::of(a), kind::CLASS_NODE | kind::MODULE_NODE));
        let preferred = format!("{}{preferred}", if in_class { "::" } else { "" });
        // derived_from_braceless_interpolation?: `"#$1"` (Prism wraps the read in an EmbeddedVariableNode)
        let n = cx.ancestors.len();
        let braceless = n >= 2
            && kind::of(&cx.ancestors[n - 1]) == kind::EMBEDDED_VARIABLE_NODE
            && matches!(
                kind::of(&cx.ancestors[n - 2]),
                kind::INTERPOLATED_STRING_NODE | kind::INTERPOLATED_REGULAR_EXPRESSION_NODE | kind::INTERPOLATED_X_STRING_NODE
            );
        let msg = format!("Prefer `{preferred}` over `{original}`.");
        let fix = if braceless { format!("{{{preferred}}}") } else { preferred };
        let loc = node.location();
        let (s, e) = (loc.start_offset(), loc.end_offset());
        cx.add_fix(s, e, msg, |c| c.replace(s, e, &fix));
    }
}
