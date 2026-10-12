use crate::prelude::*;

pub const NAME: &str = "RSpec/ImplicitBlockExpectation";
pub static COP: ImplicitBlockExpectation = ImplicitBlockExpectation;
pub struct ImplicitBlockExpectation;

/// `(block {(send (const nil? :Proc) :new) (send nil? {:proc :lambda})} ...)`
fn lambda_subject(node: &Node) -> bool {
    // `-> {}` is parser's (block (send nil :lambda) ...)
    if let Some(l) = node.as_lambda_node() {
        return !l.parameters().is_some_and(|p| {
            matches!(kind::of(&p), kind::NUMBERED_PARAMETERS_NODE | kind::IT_PARAMETERS_NODE)
        });
    }
    let Some(c) = helpers::rspec::plain_block_call(node) else { return false };
    if is_csend(&c) || !send_args(&c).is_empty() {
        return false;
    }
    match c.receiver() {
        None => matches!(method_name(&c), "proc" | "lambda"),
        Some(r) => {
            method_name(&c) == "new" && r.as_constant_read_node().is_some_and(|k| k.name().as_slice() == b"Proc")
        }
    }
}

impl Cop for ImplicitBlockExpectation {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if call.receiver().is_some() || !matches!(method_name(&call), "is_expected" | "should" | "should_not") {
            return;
        }
        let l = helpers::rspec::lang(cx);
        // nearest_subject: the first subject in the nearest multi-statement example group around
        let subject = cx.ancestors.iter().rev().find_map(|a| {
            if !helpers::rspec::example_group_with_body(&l, a) {
                return None;
            }
            let body = helpers::rspec::block_body(&a.as_call_node().unwrap());
            if !helpers::rspec::is_begin_body(&body) {
                return None;
            }
            helpers::rspec::body_children(body).into_iter().find(|s| helpers::rspec::subject(&l, s))
        });
        let Some(subject) = subject else { return };
        let body = helpers::rspec::parser_body(helpers::rspec::block_body(&subject.as_call_node().unwrap()));
        if body.is_some_and(|b| lambda_subject(&b)) {
            let (s, e) = send_range(&call);
            cx.add(s, e, "Avoid implicit block expectations.");
        }
    }
}
