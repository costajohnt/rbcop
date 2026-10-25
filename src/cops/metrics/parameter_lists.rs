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
                kind::BLOCK_PARAMETER_NODE => false,
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

/// `max_params` for `count > max_params`, at the `args` node (`range`). A non-Integer `Max` raises
/// there: a cop error at the args, and None.
// ponytail: RuboCop then crashes (`undefined method 'line' for nil`) at an args node without a
// location (a def without parentheses or parameters, a block without `|...|`); that is a cop
// error at the def or block here.
fn args_max(cx: &Cx, range: Option<(usize, usize)>) -> Option<i64> {
    let mut max = None;
    crate::runner::own_callback(cx.cop, cx.src, range.map(|r| r.0).or_else(crate::runner::visiting), || {
        max = Some(cx.cfg.int("Max").unwrap_or(0));
    });
    max
}

impl Cop for ParameterLists {
    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE, kind::BLOCK_NODE, kind::LAMBDA_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let (cop, src) = (cx.cop, cx.src);
        if let Some(def) = node.as_def_node() {
            let params = def.parameters();
            // on_def
            crate::runner::own_callback(cop, src, crate::runner::visiting(), || {
                let optargs = params.as_ref().map_or(0, |p| p.optionals().iter().count()) as i64;
                let max_opt = cx.cfg.int("MaxOptionalParameters").unwrap_or(0);
                if optargs > max_opt {
                    let l = node.location();
                    cx.add_with_limit(
                        l.start_offset(),
                        l.end_offset(),
                        format!("Method has too many optional parameters. [{optargs}/{max_opt}]"),
                        "MaxOptionalParameters",
                        optargs,
                    );
                }
            });
            // on_args, at the parser `args` node (the parentheses, else the parameters)
            if struct_initialize(cx, &def) {
                return;
            }
            let range = match (def.lparen_loc(), def.rparen_loc(), &params) {
                (Some(l), Some(r), _) => Some((l.start_offset(), r.end_offset())),
                (_, _, Some(p)) => Some((p.location().start_offset(), p.location().end_offset())),
                _ => None,
            };
            let count = args_count(cx, params.as_ref(), 0);
            let Some(max) = args_max(cx, range) else { return };
            let Some(range) = range.filter(|_| count > max) else { return };
            // The offense is on_args': so is its cop error.
            crate::runner::visit_at(range.0);
            let msg = format!("Avoid parameter lists longer than {max} parameters. [{count}/{max}]");
            cx.add_with_limit(range.0, range.1, msg, "Max", count);
            return;
        }
        // a block's or lambda's `args` (none for numbered or `it` parameters)
        let (bp, lambda) = match node.as_lambda_node() {
            Some(l) => (l.parameters(), true),
            None => (node.as_block_node().unwrap().parameters(), false),
        };
        let bp = match bp {
            None => None,
            Some(p) => match p.as_block_parameters_node() {
                Some(bp) => Some(bp),
                None => return,
            },
        };
        let range = bp.as_ref().map(|b| (b.location().start_offset(), b.location().end_offset()));
        let count = bp.as_ref().map_or(0, |b| args_count(cx, b.parameters().as_ref(), b.locals().iter().count()));
        let Some(max) = args_max(cx, range) else { return };
        if count <= max {
            return;
        }
        // arguments of a lambda literal or a lambda / proc block are exempt
        if lambda || cx.parent().and_then(|p| p.as_call_node()).is_some_and(|c| lambda_or_proc(&c)) {
            return;
        }
        let (s, e) = range.unwrap();
        crate::runner::visit_at(s);
        cx.add_with_limit(s, e, format!("Avoid parameter lists longer than {max} parameters. [{count}/{max}]"), "Max", count);
    }
}
