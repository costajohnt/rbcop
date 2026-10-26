use crate::prelude::*;
use helpers::capybara as capy;
use ruby_prism::CallNode;

pub const NAME: &str = "Capybara/ChainedFind";
pub static COP: ChainedFind = ChainedFind;
pub struct ChainedFind;

const MSG: &str = "Avoid chaining `find` methods. Combine the selectors into a single `find` call.";

/// A parser `call` named `find` (a call with a literal block is a parser `block`).
fn find_call<'pr>(node: Option<Node<'pr>>) -> Option<CallNode<'pr>> {
    node?.as_call_node().filter(|c| method_name(c) == "find" && helpers::rspec::block_of(c).is_none())
}

/// single_string_find?: `(call _ :find (str $_))`.
fn single_string(c: &CallNode) -> Option<String> {
    let args = send_args(c);
    let [a] = &args[..] else { return None };
    helpers::rspec::is_str(a).then(|| String::from_utf8_lossy(a.as_string_node().unwrap().unescaped()).into_owned())
}

/// single_string_argument?: one string argument and no block.
fn single_string_argument(c: &CallNode) -> Option<String> {
    if helpers::rspec::block_of(c).is_some() { None } else { single_string(c) }
}

/// selector_literal
fn selector_literal(s: &str) -> String {
    if s.contains(['\'', '\\']) { helpers::ruby_string::inspect(s.as_bytes()) } else { format!("'{s}'") }
}

impl Cop for ChainedFind {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "find" {
            return;
        }
        // find_receiver: a `find` call, or the send of a `find` block
        let Some(receiver) = call.receiver().and_then(|r| r.as_call_node()).filter(|r| method_name(r) == "find") else { return };
        let str_like = |n: Option<&Node>| n.is_some_and(|n| helpers::rspec::is_str(n) || helpers::rspec::is_dstr(n));
        if !str_like(send_args(&call).first()) || !str_like(send_args(&receiver).first()) {
            return;
        }
        let receiver_node = receiver.as_node();
        let mut chain: Vec<&Node> = cx.ancestors.iter().collect();
        chain.push(node);
        chain.push(&receiver_node);
        if !capy::capybara_receiver(receiver.receiver().as_ref(), &chain) {
            return;
        }
        let (s, e) = send_range(&call);
        // safe_to_autocorrect?
        let safe = capy::default_selector(cx) == "css"
            && single_string_argument(&call).is_some()
            && single_string_argument(&receiver).is_some()
            && is_csend(&call) == is_csend(&receiver)
            && {
                // safe_selectors?: every find of the chain takes one safe string
                let mut current = node.as_call_node();
                let mut ok = true;
                while let Some(c) = current {
                    match single_string_argument(&c) {
                        Some(v) if !(unescaped_comma(&v) || v.starts_with("//") || v.starts_with(".//")) => {}
                        _ => {
                            ok = false;
                            break;
                        }
                    }
                    current = find_call(c.receiver());
                }
                ok
            };
        if !safe {
            return cx.add_with(s, e, MSG, false);
        }
        // combined_selector: collect_selectors(receiver) and this find's string
        let mut selectors = Vec::new();
        let mut current = Some(receiver);
        while let Some(c) = current {
            let Some(v) = single_string(&c) else { break };
            selectors.insert(0, v);
            current = find_call(c.receiver());
        }
        let combined = format!("{} {}", selectors.join(" "), single_string(&call).unwrap());
        let literal = selector_literal(&combined);
        let msg = format!("Use `find({literal})` instead of chaining `find` methods.");
        // nested_find?: the receiver of another find
        let nested = helpers::parser_parent::parser_parent(cx.ancestors).and_then(|p| p.as_call_node()).is_some_and(|p| {
            method_name(&p) == "find" && p.receiver().is_some_and(|r| r.location().start_offset() == node.location().start_offset())
        });
        if nested {
            return cx.add_with(s, e, msg, false);
        }
        // replacement: the chain root's receiver, its dot, and the combined find
        let mut root = call;
        while let Some(r) = find_call(root.receiver()) {
            root = r;
        }
        let find = format!("find({literal})");
        let replacement = match root.receiver() {
            Some(r) => {
                let dot = root.call_operator_loc().map_or(".", |d| text(&d));
                format!("{}{dot}{find}", node_text(&r))
            }
            None => find,
        };
        cx.add_fix(s, e, msg, |c| c.replace(s, e, &replacement));
    }
}

/// `/(?<!\\),/`: a comma not preceded by a backslash.
fn unescaped_comma(s: &str) -> bool {
    let b = s.as_bytes();
    (0..b.len()).any(|i| b[i] == b',' && (i == 0 || b[i - 1] != b'\\'))
}
