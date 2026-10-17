use crate::prelude::*;
use ruby_prism::CallNode;

pub const NAME: &str = "RSpec/StubbedMock";
pub static COP: StubbedMock = StubbedMock;
pub struct StubbedMock;

/// message_expectation?: `{(send nil? {:receive :receive_message_chain} ...) (send (send nil? :receive ...) :with ...)}`
fn message_expectation(node: &Node) -> bool {
    let Some(c) = helpers::rspec::send_call(node).filter(|c| !is_csend(c)) else { return false };
    match c.receiver() {
        None => matches!(method_name(&c), "receive" | "receive_message_chain"),
        Some(r) => method_name(&c) == "with" && helpers::rspec::nil_send(&r, "receive").is_some(),
    }
}

fn flagged(matcher: &Node) -> bool {
    if let Some(c) = helpers::rspec::plain_block_call(matcher) {
        // matcher_with_return_block: (block #message_expectation? (args) _)
        let block = helpers::rspec::block_of(&c).unwrap();
        let no_params = block.parameters().is_none_or(|p| {
            p.as_block_parameters_node().is_some_and(|b| b.parameters().is_none() && b.locals().iter().next().is_none())
        });
        // the send part of the block, as its own node
        return no_params && send_part_is_expectation(&c);
    }
    let Some(c) = helpers::rspec::send_call(matcher).filter(|c| !is_csend(c)) else { return false };
    let args = send_args(&c);
    let name = method_name(&c);
    // matcher_with_configured_response: (send #message_expectation? #configured_response? _)
    if args.len() == 1
        && matches!(name, "and_return" | "and_raise" | "and_throw" | "and_yield" | "and_call_original" | "and_wrap_original")
        && c.receiver().is_some_and(|r| message_expectation(&r))
    {
        return true;
    }
    let last_hash = args.last().is_some_and(|a| a.as_hash_node().is_some() || a.as_keyword_hash_node().is_some());
    let last_pass = c.block().is_some_and(|b| b.as_block_argument_node().is_some());
    match c.receiver() {
        None => {
            // matcher_with_hash, matcher_with_blockpass
            (name == "receive_messages" && args.len() == 1 && last_hash)
                || (name == "receive_message_chain" && last_hash)
                || (matches!(name, "receive" | "receive_message_chain") && last_pass)
        }
        Some(r) => name == "with" && last_pass && helpers::rspec::nil_send(&r, "receive").is_some(),
    }
}

/// message_expectation? on the send of a block.
fn send_part_is_expectation(c: &CallNode) -> bool {
    if is_csend(c) {
        return false;
    }
    match c.receiver() {
        None => matches!(method_name(c), "receive" | "receive_message_chain"),
        Some(r) => method_name(c) == "with" && helpers::rspec::nil_send(&r, "receive").is_some(),
    }
}

impl Cop for StubbedMock {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send $(send nil? $#Expectations.all ...) :to $_)
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "to" {
            return;
        }
        let Some(expectation) = call.receiver().and_then(|r| helpers::rspec::send_call(&r)) else { return };
        let l = helpers::rspec::lang(cx);
        let name = method_name(&expectation);
        if expectation.receiver().is_some() || !l.expectation(name) {
            return;
        }
        let [matcher] = &send_args(&call)[..] else { return };
        if !flagged(matcher) {
            return;
        }
        let replacement = match name {
            "expect" => "`allow`",
            "is_expected" => "`allow(subject)`",
            "expect_any_instance_of" => "`allow_any_instance_of`",
            _ => "an allow statement",
        };
        let (s, e) = send_range(&expectation);
        cx.add(s, e, format!("Prefer {replacement} over `{name}` when configuring a response."));
    }
}
