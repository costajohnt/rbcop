use crate::helpers::code_length::global_const;
use crate::prelude::*;

pub const NAME: &str = "Metrics/ParameterLists";
pub static COP: ParameterLists = ParameterLists;
pub struct ParameterLists;

/// `args_count`: parser `args` children, without the block argument (and named keywords unless
/// `CountKeywordArgs`).
fn args_count(cx: &Cx, params: Option<&ruby_prism::ParametersNode>, locals: usize) -> i64 {
    let count_keywords = cx.cfg.bool("CountKeywordArgs").unwrap_or(false);
    let n = params.map_or(0, |p| {
        parameter_nodes(p)
            .iter()
            .filter(|a| match kind::of(a) {
                kind::BLOCK_PARAMETER_NODE | kind::IMPLICIT_REST_NODE => false,
                kind::REQUIRED_KEYWORD_PARAMETER_NODE | kind::OPTIONAL_KEYWORD_PARAMETER_NODE => count_keywords,
                _ => true,
            })
            .count()
    });
    (n + locals) as i64
}

/// `lambda_or_proc?` on the block owning these arguments.
fn lambda_or_proc(call: &ruby_prism::CallNode) -> bool {
    match (call.receiver(), call.name().as_slice()) {
        (None, b"lambda" | b"proc") => true,
        (Some(r), b"new") => global_const(&r, &["Proc"]),
        _ => false,
    }
}

/// `def initialize` that is the body of a `Struct.new` / `Data.define` block without arguments.
fn struct_initialize(cx: &Cx, def: &ruby_prism::DefNode) -> bool {
    if def.name().as_slice() != b"initialize" {
        return false;
    }
    let a = cx.ancestors;
    let n = a.len();
    if n < 3 {
        return false;
    }
    let single = a[n - 1].as_statements_node().is_some_and(|s| s.body().iter().count() == 1);
    let Some(block) = a[n - 2].as_block_node() else { return false };
    let Some(call) = a[n - 3].as_call_node() else { return false };
    let no_args = match block.parameters() {
        None => true,
        Some(p) => p.as_block_parameters_node().is_some_and(|b| b.parameters().is_none() && b.locals().iter().count() == 0),
    };
    let ctor = match (call.receiver(), call.name().as_slice()) {
        (Some(r), b"new") => global_const(&r, &["Struct"]),
        (Some(r), b"define") => global_const(&r, &["Data"]),
        _ => false,
    };
    single && no_args && ctor
}

impl Cop for ParameterLists {
    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE, kind::BLOCK_NODE, kind::LAMBDA_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let max = cx.cfg.int("Max").unwrap_or(0);
        if let Some(def) = node.as_def_node() {
            let params = def.parameters();
            let optargs = params.as_ref().map_or(0, |p| p.optionals().iter().count()) as i64;
            let max_opt = cx.cfg.int("MaxOptionalParameters").unwrap_or(0);
            if optargs > max_opt {
                let l = node.location();
                cx.add(
                    l.start_offset(),
                    l.end_offset(),
                    format!("Method has too many optional parameters. [{optargs}/{max_opt}]"),
                );
            }
            if struct_initialize(cx, &def) {
                return;
            }
            let count = args_count(cx, params.as_ref(), 0);
            if count <= max {
                return;
            }
            let range = match (def.lparen_loc(), def.rparen_loc(), &params) {
                (Some(l), Some(r), _) => (l.start_offset(), r.end_offset()),
                (_, _, Some(p)) => (p.location().start_offset(), p.location().end_offset()),
                _ => return,
            };
            cx.add(range.0, range.1, format!("Avoid parameter lists longer than {max} parameters. [{count}/{max}]"));
            return;
        }
        if node.as_lambda_node().is_some() {
            return; // arguments of a lambda literal are exempt
        }
        let block = node.as_block_node().unwrap();
        let Some(bp) = block.parameters().and_then(|p| p.as_block_parameters_node()) else { return };
        let count = args_count(cx, bp.parameters().as_ref(), bp.locals().iter().count());
        if count <= max {
            return;
        }
        if cx.parent().and_then(|p| p.as_call_node()).is_some_and(|c| lambda_or_proc(&c)) {
            return;
        }
        let l = bp.location();
        cx.add(
            l.start_offset(),
            l.end_offset(),
            format!("Avoid parameter lists longer than {max} parameters. [{count}/{max}]"),
        );
    }
}
