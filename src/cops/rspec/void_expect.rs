use crate::prelude::*;

pub const NAME: &str = "RSpec/VoidExpect";
pub static COP: VoidExpect = VoidExpect;
pub struct VoidExpect;

const MSG: &str = "Do not use `expect()` without `.to` or `.not_to`. Chain the methods or remove it.";

/// InsideExample#inside_example?: a `block` ancestor that is an example, nearest first (`any?`
/// stops at the first one, so a nil `Language` list is asked only that far).
fn inside_example(cx: &Cx, l: &helpers::rspec::Language) -> bool {
    cx.ancestors.iter().rev().any(|a| helpers::rspec::example(l, a))
}

/// void?: the parent is a `begin`, or a block whose body is the expectation.
fn void(cx: &Cx) -> bool {
    let anc = cx.ancestors;
    let Some(i) = helpers::parser_parent::parser_parent_index(anc, anc.len()) else { return false };
    let p = &anc[i];
    // a plain `begin ... end` holds its statements directly (kwbegin, no `begin`)
    let kwbegin = i > 0
        && anc[i - 1].as_begin_node().is_some_and(|b| {
            b.begin_keyword_loc().is_some() && b.rescue_clause().is_none() && b.ensure_clause().is_none() && b.else_clause().is_none()
        });
    p.as_parentheses_node().is_some()
        || (p.as_statements_node().is_some_and(|s| s.body().iter().count() > 1) && !kwbegin)
        // `parent.block_type?`: not a numblock / itblock
        || match p {
            Node::BlockNode { .. } => helpers::node_types::plain_block_params(p.as_block_node().unwrap().parameters()),
            Node::LambdaNode { .. } => helpers::node_types::plain_block_params(p.as_lambda_node().unwrap().parameters()),
            _ => false,
        }
}

impl Cop for VoidExpect {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // expect?: (send nil? :expect ...)
        let call = node.as_call_node().unwrap();
        if call.receiver().is_some() || method_name(&call) != "expect" {
            return;
        }
        let l = helpers::rspec::lang(cx);
        let block = helpers::rspec::block_of(&call);
        // on_block, visited before its send: expect_block? `(block #expect? (args) _body)`
        if let Some(b) = &block
            && helpers::rspec::is_plain(b)
            && b.parameters().is_none_or(|p| {
                p.as_block_parameters_node().is_some_and(|bp| bp.parameters().is_none() && bp.locals().iter().next().is_none())
            })
        {
            let checked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if inside_example(cx, &l) && void(cx) {
                    let (s, e) = helpers::rspec::node_range(node);
                    cx.add(s, e, MSG);
                }
            }));
            // a raise there is a cop error of its own, and on_send still runs
            if let Err(payload) = checked {
                if !(payload.is::<String>() || payload.is::<&str>()) {
                    std::panic::resume_unwind(payload);
                }
                crate::runner::cop_error_at(cx.cop, cx.src, crate::runner::visiting());
            }
        }
        // on_send: the send's nearest `block` ancestor is its own block
        if !(helpers::rspec::example(&l, node) || inside_example(cx, &l)) {
            return;
        }
        match &block {
            // its parent is its block: void when that is a `block` whose body is the send (nodes
            // compare structurally)
            Some(b) => {
                if helpers::rspec::is_plain(b)
                    && let Some(body) = helpers::rspec::parser_body(b.body())
                    && let Some(inner) = helpers::rspec::send_call(&body).filter(|c| !is_csend(c) && c.receiver().is_none())
                    && method_name(&inner) == "expect"
                    && helpers::rspec::skeys(cx.src, &send_args(&inner)) == helpers::rspec::skeys(cx.src, &send_args(&call))
                {
                    let (s, e) = send_range(&call);
                    cx.add(s, e, MSG);
                }
            }
            None => {
                if void(cx) {
                    let (s, e) = helpers::rspec::node_range(node);
                    cx.add(s, e, MSG);
                }
            }
        }
    }
}
