use crate::prelude::*;

pub const NAME: &str = "Lint/ReturnInVoidContext";
pub static COP: ReturnInVoidContext = ReturnInVoidContext;
pub struct ReturnInVoidContext;

impl Cop for ReturnInVoidContext {
    fn kinds(&self) -> &'static [usize] {
        &[kind::RETURN_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let ret = node.as_return_node().unwrap();
        if ret.arguments().is_none() {
            return;
        }
        let Some(def) = cx.ancestors.iter().rev().find_map(|a| a.as_def_node()) else { return };
        let name = std::str::from_utf8(def.name().as_slice()).unwrap_or("");
        let assignment = name.ends_with('=') && !matches!(name, "==" | "===" | "!=" | "<=" | ">=");
        if !(name == "initialize" && def.receiver().is_none()) && !assignment {
            return;
        }
        // Any block ancestor (all the way up) of lambda / define_method / define_singleton_method.
        let scope_changing = cx.ancestors.iter().enumerate().any(|(i, a)| match kind::of(a) {
            kind::LAMBDA_NODE => true,
            kind::BLOCK_NODE => cx.ancestors[..i].iter().rev().find_map(|p| p.as_call_node()).is_some_and(|c| {
                matches!(method_name(&c), "lambda" | "define_method" | "define_singleton_method")
            }),
            _ => false,
        });
        if scope_changing {
            return;
        }
        let kw = ret.keyword_loc();
        cx.add(kw.start_offset(), kw.end_offset(), format!("Do not return a value in `{name}`."));
    }
}
