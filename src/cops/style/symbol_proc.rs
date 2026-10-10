use crate::helpers::range_help::{Side, Surrounding, range_with_surrounding_space};
use crate::prelude::*;

pub const NAME: &str = "Style/SymbolProc";
pub static COP: SymbolProc = SymbolProc;
pub struct SymbolProc;

/// The block body when it is `(send (lvar var) name)` with `var` read via `is_var`: the method name.
fn body_method<'pr>(body: Option<Node<'pr>>, is_var: impl Fn(&Node) -> bool) -> Option<&'pr str> {
    let stmts = body?.as_statements_node()?;
    let mut it = stmts.body().iter();
    let only = it.next()?;
    if it.next().is_some() {
        return None;
    }
    let call = only.as_call_node()?;
    if is_csend(&call) || call.arguments().is_some() || call.block().is_some() {
        return None;
    }
    is_var(&call.receiver()?).then(|| method_name(&call))
}

/// The dispatch node of a block: (name, receiver, parser send args, parenthesized `(`, end of args).
struct Dispatch<'pr> {
    name: &'pr str,
    receiver: Option<Node<'pr>>,
    args: Vec<Node<'pr>>,
    open_paren: Option<usize>,
    lambda_literal: bool,
}

impl Cop for SymbolProc {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::BLOCK_NODE, kind::LAMBDA_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let (params, body, open, close) = if let Some(b) = node.as_block_node() {
            (b.parameters(), b.body(), b.opening_loc(), b.closing_loc())
        } else {
            let l = node.as_lambda_node().unwrap();
            (l.parameters(), l.body(), l.opening_loc(), l.closing_loc())
        };
        let Some(params) = params else { return };
        let mut plain_block = false;
        let method = if let Some(bp) = params.as_block_parameters_node() {
            plain_block = true;
            let Some(p) = bp.parameters() else { return };
            if bp.locals().iter().count() != 0 {
                return;
            }
            let all = parameter_nodes(&p);
            let [only] = all.as_slice() else { return };
            let Some(req) = only.as_required_parameter_node() else { return };
            if p.rest().is_some() {
                return;
            }
            let var = req.name().as_slice();
            body_method(body, |n| n.as_local_variable_read_node().is_some_and(|l| l.name().as_slice() == var))
        } else if let Some(np) = params.as_numbered_parameters_node() {
            if np.maximum() != 1 {
                return;
            }
            body_method(body, |n| n.as_local_variable_read_node().is_some_and(|l| l.name().as_slice() == b"_1"))
        } else if params.as_it_parameters_node().is_some() {
            body_method(body, |n| n.as_it_local_variable_read_node().is_some())
        } else {
            None
        };
        let Some(method) = method else { return };

        // `|a,|` has an ImplicitRestNode parameter, so it never reaches here; parser's
        // destructuring_block_argument? check is covered by the single-parameter match above.
        let dispatch = if node.as_lambda_node().is_some() {
            Dispatch { name: "lambda", receiver: None, args: Vec::new(), open_paren: None, lambda_literal: true }
        } else {
            let Some(parent) = cx.ancestors.last() else { return };
            if let Some(call) = parent.as_call_node() {
                Dispatch {
                    name: method_name(&call),
                    receiver: call.receiver(),
                    args: call_args(&call),
                    open_paren: call.opening_loc().map(|l| l.start_offset()),
                    lambda_literal: false,
                }
            } else if let Some(s) = parent.as_super_node() {
                Dispatch {
                    name: "super",
                    receiver: None,
                    args: s.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default(),
                    open_paren: s.lparen_loc().map(|l| l.start_offset()),
                    lambda_literal: false,
                }
            } else if parent.as_forwarding_super_node().is_some() {
                Dispatch { name: "super", receiver: None, args: Vec::new(), open_paren: None, lambda_literal: false }
            } else {
                return;
            }
        };

        if cx.config.all_cops().truthy("ActiveSupportExtensionsEnabled") {
            let proc_new = dispatch.name == "new"
                && dispatch.receiver.as_ref().is_some_and(|r| helpers::parser_parent::is_top_const(r, "Proc"));
            if proc_new || matches!(dispatch.name, "lambda" | "proc") {
                return;
            }
        }
        let recv_kind = dispatch.receiver.as_ref().map(kind::of);
        if recv_kind == Some(kind::HASH_NODE) && matches!(dispatch.name, "reject" | "select") {
            return;
        }
        if recv_kind == Some(kind::ARRAY_NODE) && matches!(dispatch.name, "min" | "max") {
            return;
        }
        let cfg = cx.cfg;
        if helpers::allowed::allowed_method(cfg, dispatch.name)
            || helpers::allowed::matches_allowed_pattern(cfg, dispatch.name)
        {
            return;
        }
        if cfg.bool("AllowMethodsWithArguments").unwrap_or(false) && !dispatch.args.is_empty() {
            return;
        }
        if plain_block && false {
            return;
        }
        let src = cx.src;
        let block_start =
            if let Some(n) = node.as_lambda_node() { n.location().start_offset() } else { cx.ancestors.last().unwrap().location().start_offset() };
        let (start_line, end_line) = (src.line_of(block_start), src.line_of(close.start_offset()));
        if helpers::comments_help::allow_comments(cx, start_line, end_line) {
            return;
        }

        let (start, end) = (open.start_offset(), close.end_offset());
        let msg = format!("Pass `&:{method}` as an argument to `{}` instead of a block.", dispatch.name);
        let text = src.text;
        let node_end = node.location().end_offset();
        cx.add_fix(start, end, msg, |c| {
            let left = Surrounding { side: Side::Left, ..Default::default() };
            if let Some(last) = dispatch.args.last() {
                let mut arg_end = last.location().end_offset();
                while text.as_bytes().get(arg_end) == Some(&b',') {
                    arg_end += 1;
                }
                let arg_start = last.location().start_offset();
                let mut replacement = format!(" &:{method}");
                if !text[arg_start..arg_end].ends_with(',') {
                    replacement.insert(0, ',');
                }
                c.insert_after(arg_start, arg_end, &replacement);
                let (bs, be) = range_with_surrounding_space(text, start, end, left);
                c.remove(bs, be);
            } else if dispatch.lambda_literal {
                c.replace(block_start, node_end, &format!("lambda(&:{method})"));
            } else {
                let from = dispatch.open_paren.unwrap_or(start);
                let (bs, be) = range_with_surrounding_space(text, from, end, left);
                c.replace(bs, be, &format!("(&:{method})"));
            }
        });
    }
}
