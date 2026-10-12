use crate::prelude::*;

pub const NAME: &str = "RSpec/VoidExpect";
pub static COP: VoidExpect = VoidExpect;
pub struct VoidExpect;

impl Cop for VoidExpect {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // on_send (send nil? :expect ...), or on_block (block #expect? (args) _body)
        let call = node.as_call_node().unwrap();
        if call.receiver().is_some() || method_name(&call) != "expect" {
            return;
        }
        // on_send for the send of `expect ... { body }`: its parent block's body `==` it when the
        // body is the same send (nodes compare structurally)
        if let Some(b) = helpers::rspec::block_of(&call)
            && let Some(body) = helpers::rspec::parser_body(b.body())
            && let Some(inner) = helpers::rspec::send_call(&body).filter(|c| !is_csend(c) && c.receiver().is_none())
            && method_name(&inner) == "expect"
            && helpers::rspec::skeys(cx.src, &send_args(&inner)) == helpers::rspec::skeys(cx.src, &send_args(&call))
        {
            let l = helpers::rspec::lang(cx);
            if cx.ancestors.iter().any(|a| helpers::rspec::example(&l, a)) || helpers::rspec::example(&l, node) {
                let (s, e) = send_range(&call);
                cx.add(s, e, "Do not use `expect()` without `.to` or `.not_to`. Chain the methods or remove it.");
            }
        }
        if let Some(b) = helpers::rspec::block_of(&call) {
            let no_params = helpers::rspec::is_plain(&b)
                && b.parameters().is_none_or(|p| {
                    p.as_block_parameters_node().is_some_and(|bp| bp.parameters().is_none() && bp.locals().iter().next().is_none())
                });
            if !no_params {
                return;
            }
        }
        let l = helpers::rspec::lang(cx);
        if !cx.ancestors.iter().any(|a| helpers::rspec::example(&l, a)) {
            return;
        }
        // void?: the parent is a `begin`, or a block whose body is the expectation
        let anc = cx.ancestors;
        let void = match helpers::parser_parent::parser_parent_index(anc, anc.len()) {
            Some(i) => {
                let p = &anc[i];
                // a plain `begin ... end` holds its statements directly (kwbegin, no `begin`)
                let kwbegin = i > 0
                    && anc[i - 1].as_begin_node().is_some_and(|b| {
                        b.begin_keyword_loc().is_some()
                            && b.rescue_clause().is_none()
                            && b.ensure_clause().is_none()
                            && b.else_clause().is_none()
                    });
                p.as_parentheses_node().is_some()
                    || (p.as_statements_node().is_some_and(|s| s.body().iter().count() > 1) && !kwbegin)
                    || matches!(kind::of(p), kind::BLOCK_NODE | kind::LAMBDA_NODE)
            }
            None => false,
        };
        if void {
            let (s, e) = helpers::rspec::node_range(node);
            cx.add(s, e, "Do not use `expect()` without `.to` or `.not_to`. Chain the methods or remove it.");
        }
    }
}
