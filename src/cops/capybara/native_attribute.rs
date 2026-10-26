use crate::prelude::*;
use helpers::capybara::capybara_receiver;

pub const NAME: &str = "Capybara/NativeAttribute";
pub static COP: NativeAttribute = NativeAttribute;
pub struct NativeAttribute;

const FINDERS: &[&str] = &["ancestor", "find", "find_button", "find_by_id", "find_field", "find_link", "first", "sibling"];

/// element_finder?: `{#capybara_receiver? (call #capybara_receiver? FINDERS ...) (any_block
/// #element_finder? ...) (begin #element_finder?)}`; `ancestors` are the Prism ancestors of `node`.
/// A call with a block stands for its send, as in parser's `any_block`.
fn element_finder(node: Option<&Node>, ancestors: &[&Node]) -> bool {
    if capybara_receiver(node, ancestors) {
        return true;
    }
    let Some(node) = node else { return false };
    let mut chain = ancestors.to_vec();
    chain.push(node);
    if let Some(p) = node.as_parentheses_node() {
        let Some(body) = p.body() else { return false };
        let Some(s) = body.as_statements_node() else { return false };
        let stmts: Vec<Node> = s.body().iter().collect();
        if let [only] = &stmts[..] {
            chain.push(&body);
            return element_finder(Some(only), &chain);
        }
        return false;
    }
    let Some(c) = node.as_call_node() else { return false };
    FINDERS.contains(&method_name(&c)) && capybara_receiver(c.receiver().as_ref(), &chain)
}

impl Cop for NativeAttribute {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (call $(call #element_finder? :native) :attribute $({str sym} _))
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "attribute" {
            return;
        }
        let args = send_args(&call);
        let [attribute] = &args[..] else { return };
        if !(helpers::rspec::is_str(attribute) || helpers::rspec::is_sym(cx, attribute)) {
            return;
        }
        let Some(native) = call.receiver().and_then(|r| r.as_call_node()) else { return };
        if method_name(&native) != "native" || !send_args(&native).is_empty() || helpers::rspec::block_of(&native).is_some() {
            return;
        }
        // `next unless native.receiver`
        let Some(recv) = native.receiver() else { return };
        let native_node = native.as_node();
        let mut chain: Vec<&Node> = cx.ancestors.iter().collect();
        chain.push(node);
        chain.push(&native_node);
        if !element_finder(Some(&recv), &chain) {
            return;
        }
        let (s, e) = send_range(&call);
        let heredoc = helpers::rspec::is_str(attribute) && is_heredoc(attribute);
        // correct_attribute: from native's dot to the end, unless a comment is inside
        let dot = native.call_operator_loc().map(|d| d.start_offset());
        let commented = dot.is_some_and(|d| cx.src.comments.iter().any(|c| c.start >= d && c.end <= e));
        let safe_navigation = is_csend(&native) && is_csend(&call);
        let source = node_text(attribute);
        let replacement = if safe_navigation { format!("&.[]({source})") } else { format!("[{source}]") };
        cx.add_fix(s, e, "Use `Element#[]` instead of driver-specific `native.attribute`.", |c| {
            if let Some(d) = dot
                && !heredoc
                && !commented
            {
                c.replace(d, e, &replacement);
            }
        });
    }
}
