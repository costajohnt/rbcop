use crate::helpers::sorbet_signature::{annotations_before, as_send, block_body, outermost_range};
use crate::prelude::*;
use ruby_prism::CallNode;

pub const NAME: &str = "Sorbet/AllowIncompatibleOverride";
pub static COP: AllowIncompatibleOverride = AllowIncompatibleOverride;
pub struct AllowIncompatibleOverride;

const MSG: &str = "Usage of `allow_incompatible` suggests a violation of the Liskov Substitution Principle. Instead, strive to write interfaces which respect subtyping principles and remove `allow_incompatible`";

static RBS_ALLOW_INCOMPATIBLE_OVERRIDE: LazyLock<Regex> = LazyLock::new(|| {
    let s = "[ \\t\\r\\n\\x0b\\x0c]*";
    Regex::new(&format!(r"\A#{s}@override\({s}(allow_incompatible{s}:{s}true){s}\){s}\z")).unwrap()
});

/// `(send _ :override (hash <$(pair (sym :allow_incompatible) true) ...>))`: the pair's range.
fn override_pair(call: &CallNode) -> Option<(usize, usize)> {
    if call.name().as_slice() != b"override" {
        return None;
    }
    let args = send_args(call);
    let [hash] = args.as_slice() else { return None };
    let elements: Vec<Node> = match (hash.as_hash_node(), hash.as_keyword_hash_node()) {
        (Some(h), _) => h.elements().iter().collect(),
        (_, Some(h)) => h.elements().iter().collect(),
        _ => return None,
    };
    elements.iter().find_map(|e| {
        let pair = e.as_assoc_node()?;
        let key = pair.key();
        let ok = key.as_symbol_node().is_some_and(|s| s.unescaped() == b"allow_incompatible") && pair.value().as_true_node().is_some();
        ok.then(|| helpers::node_types::range(e))
    })
}

/// `def_node_search :sig?, (send _ :sig ...)` over a send (receiver, arguments and itself).
fn has_sig(call: &CallNode) -> bool {
    let mut found = false;
    let mut f = |c: &CallNode| found |= !is_csend(c) && c.name().as_slice() == b"sig";
    f(call);
    if let Some(r) = call.receiver() {
        helpers::sorbet_signature::each_call(&r, &mut f);
    }
    for a in send_args(call) {
        helpers::sorbet_signature::each_call(&a, &mut f);
    }
    found
}

/// rubocop-ast `receiver`: a send's, or a block's send's (`.` or `&.`).
fn receiver<'pr>(node: &Node<'pr>) -> Option<Node<'pr>> {
    let call = node.as_call_node()?;
    let block = call.block().is_some_and(|b| b.as_block_node().is_some());
    if is_csend(&call) && !block {
        return None;
    }
    call.receiver()
}

fn check_rbs_annotations(cx: &mut Cx, range: (usize, usize)) {
    let src = cx.src;
    for c in annotations_before(cx, range) {
        if let Some(m) = RBS_ALLOW_INCOMPATIBLE_OVERRIDE.captures(src.slice(c.start, c.end)).and_then(|m| m.get(1)) {
            cx.add(c.start + m.start(), c.start + m.end(), MSG);
        }
    }
}

impl Cop for AllowIncompatibleOverride {
    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE, kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = node.as_call_node() else {
            let range = outermost_range(cx.ancestors, helpers::node_types::range(node));
            return check_rbs_annotations(cx, range);
        };
        // on_block
        if call.block().is_some_and(|b| b.as_block_node().is_some())
            && has_sig(&call)
            && let Some(body) = block_body(&call).as_ref().and_then(|b| as_send(b))
        {
            let mut cur = body.receiver();
            while let Some(r) = cur {
                if let Some(pair) = as_send(&r).and_then(|c| override_pair(&c)) {
                    cx.add(pair.0, pair.1, MSG);
                    break;
                }
                cur = receiver(&r);
            }
        }
        // on_send
        if is_csend(&call) {
            return;
        }
        match call.name().as_slice() {
            b"attr_reader" | b"attr_writer" | b"attr_accessor" => {
                let range = outermost_range(cx.ancestors, send_range(&call));
                check_rbs_annotations(cx, range);
            }
            b"override" if call.receiver().is_some_and(|r| {
                let mut found = false;
                helpers::sorbet_signature::each_call(&r, |c| found |= !is_csend(c) && c.name().as_slice() == b"sig");
                found
            }) =>
            {
                if let Some(pair) = override_pair(&call) {
                    cx.add(pair.0, pair.1, MSG);
                }
            }
            _ => {}
        }
    }
}
