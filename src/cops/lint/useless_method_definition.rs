use crate::prelude::*;

pub const NAME: &str = "Lint/UselessMethodDefinition";
pub static COP: UselessMethodDefinition = UselessMethodDefinition;
pub struct UselessMethodDefinition;

impl Cop for UselessMethodDefinition {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let def = node.as_def_node().unwrap();
        // node.parent&.send_type?: the def is an argument of a (non-safe-navigation) call.
        let n = cx.ancestors.len();
        let parent_send = (n >= 2 && kind::of(&cx.ancestors[n - 1]) == kind::ARGUMENTS_NODE)
            .then(|| cx.ancestors[n - 2].as_call_node())
            .flatten()
            .filter(|c| !is_csend(c));
        if let Some(call) = &parent_send {
            let non_bare_modifier = call.receiver().is_none()
                && matches!(method_name(call), "public" | "protected" | "private" | "module_function")
                && helpers::macro_scope::in_macro_scope_of_child(&cx.ancestors[..n - 2]);
            if !non_bare_modifier {
                return;
            }
        }
        let params: Vec<Node> = match def.parameters() {
            Some(p) => {
                if p.optionals().iter().next().is_some()
                    || p.rest().is_some_and(|r| r.as_rest_parameter_node().is_some())
                    || p.keywords().iter().any(|k| k.as_optional_keyword_parameter_node().is_some())
                    || p.keyword_rest().is_some_and(|k| k.as_keyword_rest_parameter_node().is_some())
                {
                    return;
                }
                let mut v: Vec<Node> = p.requireds().iter().collect();
                v.extend(p.optionals().iter());
                v.extend(p.rest());
                v.extend(p.posts().iter());
                v.extend(p.keywords().iter());
                v.extend(p.keyword_rest());
                v.extend(p.block().map(|b| b.as_node()));
                v
            }
            None => Vec::new(),
        };
        let Some(body) = def.body().and_then(|b| b.as_statements_node()) else { return };
        let stmts: Vec<Node> = body.body().iter().collect();
        if stmts.len() != 1 {
            return;
        }
        let delegating = if let Some(z) = stmts[0].as_forwarding_super_node() {
            z.block().is_none()
        } else if let Some(s) = stmts[0].as_super_node() {
            if s.block().is_some_and(|b| b.as_block_node().is_some()) {
                return;
            }
            let mut args: Vec<&str> = s.arguments().map(|a| a.arguments().iter().map(|x| node_text(&x)).collect()).unwrap_or_default();
            args.extend(s.block().map(|b| node_text(&b)));
            let def_args: Vec<&str> = params.iter().map(node_text).collect();
            args == def_args
        } else {
            false
        };
        if !delegating {
            return;
        }
        let loc = node.location();
        let (rs, re) = match &parent_send {
            Some(c) => send_range(c),
            None => (loc.start_offset(), loc.end_offset()),
        };
        cx.add_fix(loc.start_offset(), loc.end_offset(), "Useless method definition detected.", |c| c.remove(rs, re));
    }
}
