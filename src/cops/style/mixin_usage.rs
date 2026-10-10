use crate::prelude::*;

pub const NAME: &str = "Style/MixinUsage";
pub static COP: MixinUsage = MixinUsage;
pub struct MixinUsage;

impl Cop for MixinUsage {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        // (send nil? {:include :extend :prepend} const+); a literal block makes the parent a `block`.
        if call.receiver().is_some() || call.block().is_some() || !matches!(name, "include" | "extend" | "prepend") {
            return;
        }
        let args = call_args(&call);
        let is_const = |a: &Node| matches!(kind::of(a), kind::CONSTANT_READ_NODE | kind::CONSTANT_PATH_NODE);
        if args.is_empty() || !args.iter().all(is_const) || !top_level(cx.ancestors) {
            return;
        }
        let loc = node.location();
        cx.add(
            loc.start_offset(),
            loc.end_offset(),
            format!("`{name}` is used at the top level. Use inside `class` or `module`."),
        );
    }
}

/// `{root? ^[{kwbegin begin if def} #in_top_level_scope?]}` over the parser ancestors.
fn top_level(ancestors: &[Node]) -> bool {
    for a in ancestors.iter().rev() {
        match kind::of(a) {
            kind::PROGRAM_NODE => return true,
            kind::STATEMENTS_NODE | kind::ELSE_NODE | kind::PARENTHESES_NODE | kind::IF_NODE | kind::UNLESS_NODE => {}
            kind::DEF_NODE if a.as_def_node().unwrap().receiver().is_none() => {}
            kind::BEGIN_NODE => {
                let b = a.as_begin_node().unwrap();
                // implicit begin or rescue/ensure: parser has a `rescue`/`ensure` node in between
                if b.begin_keyword_loc().is_none() || b.rescue_clause().is_some() || b.ensure_clause().is_some() {
                    return false;
                }
            }
            _ => return false,
        }
    }
    true
}
