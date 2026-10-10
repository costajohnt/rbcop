use crate::prelude::*;

pub const NAME: &str = "Style/ModuleFunction";
pub static COP: ModuleFunction = ModuleFunction;
pub struct ModuleFunction;

#[derive(PartialEq, Clone, Copy)]
enum Kind {
    ModuleFunction,
    ExtendSelf,
    Private,
}

/// `(send nil? :module_function)`, `(send nil? :extend self)`, `(send nil? :private ...)`.
fn kind_of(n: &Node) -> Option<Kind> {
    let c = n.as_call_node()?;
    if c.receiver().is_some() || c.block().is_some_and(|b| b.as_block_node().is_some()) {
        return None;
    }
    let args = send_args(&c);
    match method_name(&c) {
        "module_function" if args.is_empty() => Some(Kind::ModuleFunction),
        "extend" if args.len() == 1 && args[0].as_self_node().is_some() => Some(Kind::ExtendSelf),
        "private" => Some(Kind::Private),
        _ => None,
    }
}

impl Cop for ModuleFunction {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::MODULE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(body) = node.as_module_node().unwrap().body() else { return };
        // implicit begin (rescue/ensure) is a single non-matching parser node
        let Some(stmts) = body.as_statements_node() else { return };
        let nodes: Vec<Node> = stmts.body().iter().collect();
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("module_function");
        if style == "module_function" && nodes.iter().any(|n| kind_of(n) == Some(Kind::Private)) {
            return;
        }
        let msg = match style {
            "forbidden" => "Do not use `module_function` or `extend self`.",
            "module_function" => "Use `module_function` instead of `extend self`.",
            _ => "Use `extend self` instead of `module_function`.",
        };
        for n in &nodes {
            let k = kind_of(n);
            let hit = match style {
                "module_function" => k == Some(Kind::ExtendSelf),
                "extend_self" => k == Some(Kind::ModuleFunction),
                "forbidden" => matches!(k, Some(Kind::ExtendSelf | Kind::ModuleFunction)),
                _ => false,
            };
            if !hit {
                continue;
            }
            let loc = n.location();
            let (s, e) = (loc.start_offset(), loc.end_offset());
            let forbidden = style == "forbidden";
            let repl = if k == Some(Kind::ExtendSelf) { "module_function" } else { "extend self" };
            cx.add_fix(s, e, msg, |c| {
                if !forbidden {
                    c.replace(s, e, repl);
                }
            });
        }
    }
}
