use crate::prelude::*;
use helpers::capybara as capy;

pub const NAME: &str = "Capybara/AbsoluteXPathInElementScope";
pub static COP: AbsoluteXPathInElementScope = AbsoluteXPathInElementScope;
pub struct AbsoluteXPathInElementScope;

/// element_finder?: `(call #capybara_receiver? :find ...)`, a call without a literal block.
/// `ancestors` are the Prism ancestors of `node`.
fn element_finder(node: Option<&Node>, ancestors: &[&Node]) -> bool {
    let Some(node) = node else { return false };
    let Some(c) = node.as_call_node() else { return false };
    if method_name(&c) != "find" || helpers::rspec::block_of(&c).is_some() {
        return false;
    }
    let mut chain = ancestors.to_vec();
    chain.push(node);
    capy::capybara_receiver(c.receiver().as_ref(), &chain)
}

/// rspec_group?
fn rspec_group(call: &ruby_prism::CallNode) -> bool {
    matches!(method_name(call), "describe" | "context" | "feature" | "shared_examples" | "shared_context")
        && call.receiver().is_none_or(|r| helpers::parser_parent::is_top_const(&r, "RSpec"))
}

/// A literal block that parser makes a `block` (not numblock/itblock).
fn plain_block<'pr>(call: &ruby_prism::CallNode<'pr>) -> Option<ruby_prism::BlockNode<'pr>> {
    helpers::rspec::block_of(call).filter(|b| b.parameters().is_none_or(|p| p.as_block_parameters_node().is_some()))
}

impl Cop for AbsoluteXPathInElementScope {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if !matches!(method_name(&call), "find" | "all" | "find_all" | "first") {
            return;
        }
        // (call _ _ (sym :xpath) $(str _) ...)
        let args = send_args(&call);
        let [first, xpath, ..] = &args[..] else { return };
        if capy::sym(cx, first).as_deref() != Some("xpath") || !helpers::rspec::is_str(xpath) {
            return;
        }
        if !xpath.as_string_node().unwrap().unescaped().starts_with(b"//") {
            return;
        }
        let Some(recv) = call.receiver() else { return };
        let mut chain: Vec<&Node> = cx.ancestors.iter().collect();
        chain.push(node);
        let plain_call = recv.as_call_node().filter(|c| helpers::rspec::block_of(c).is_none());
        let element = if plain_call.is_some() && element_finder(Some(&recv), &chain) {
            true
        } else if recv.as_local_variable_read_node().is_some() {
            capy::local_variable_value_in(&recv, &chain).is_some_and(|(v, i, w)| {
                let mut va = chain[..=i].to_vec();
                va.push(&w);
                element_finder(Some(&v), &va)
            })
        } else if let Some(rc) = plain_call {
            rspec_helper(cx, &rc, &chain)
        } else {
            false
        };
        if element {
            let (s, e) = helpers::rspec::node_range(xpath);
            cx.add(s, e, "Use `.//` for an XPath search within an element.");
        }
    }
}

/// element_finder?(rspec_helper_value(node)): the body of the nearest RSpec group's last
/// `let`/`let!`/`subject` named after the receiver-less, argument-less call `node`. `chain` are
/// the Prism ancestors of `node`.
fn rspec_helper(cx: &Cx, node: &ruby_prism::CallNode, chain: &[&Node]) -> bool {
    if node.receiver().is_some() || !send_args(node).is_empty() {
        return false;
    }
    let name = method_name(node);
    // each_ancestor(:block), nearest first; a call owning a block is one when we are in its
    // receiver or arguments too
    for i in (0..chain.len()).rev() {
        let Some(group) = chain[i].as_call_node() else { continue };
        let Some(block) = plain_block(&group) else { continue };
        if !rspec_group(&group) {
            continue;
        }
        // group.body: its statements (one, or a parser `begin` of several)
        let Some(body) = block.body() else { continue };
        let stmts: Vec<Node> = match body.as_statements_node() {
            Some(s) => s.body().iter().collect(),
            None => Vec::new(),
        };
        let def = stmts.iter().rev().find_map(|st| {
            let c = st.as_call_node()?;
            let b = plain_block(&c)?;
            let ok = c.receiver().is_none()
                && matches!(method_name(&c), "let" | "let!" | "subject")
                && send_args(&c).first().and_then(|a| capy::sym(cx, a)).as_deref() == Some(name);
            ok.then_some((st, b))
        });
        let Some((st, b)) = def else { continue };
        // definition.body: a single statement (several are a parser `begin`)
        let Some(lb) = b.body() else { return false };
        let Some(s) = lb.as_statements_node() else { return false };
        let inner: Vec<Node> = s.body().iter().collect();
        let [only] = &inner[..] else { return false };
        let block_node = block.as_node();
        let let_block = b.as_node();
        let mut va = chain[..=i].to_vec();
        va.extend([&block_node, &body, st, &let_block, &lb]);
        return element_finder(Some(only), &va);
    }
    false
}
