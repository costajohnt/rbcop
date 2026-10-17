use crate::prelude::*;
use helpers::parser_parent::parser_parent_index;

pub const NAME: &str = "Rails/StrongParametersExpect";
pub static COP: StrongParametersExpect = StrongParametersExpect;
pub struct StrongParametersExpect;

#[rustfmt::skip]
const IGNORED_METHODS: &[&str] = &[
    "!", "blank?", "compact", "compact!", "compact_blank", "compact_blank!", "deep_merge", "deep_merge!",
    "delete", "delete_if", "dig", "each", "except", "exclude?", "extract!", "fetch", "has_key?", "has_value?",
    "include?", "inspect", "instance_of?", "is_a?", "keep_if", "key?", "keys", "kind_of?", "member?", "merge", "merge!",
    "nil?", "presence", "present?", "reverse_merge", "reverse_merge!", "slice", "stringify_keys",
    "to_a", "to_f", "to_h", "to_hash", "to_i", "to_s", "to_unsafe_h", "to_unsafe_hash",
    "transform_keys", "transform_keys!", "transform_values", "transform_values!", "try", "try!",
    "value?", "values", "values_at", "with_defaults", "with_defaults!", "without",
];
const RAISING_FINDER_METHODS: &[&str] = &["find", "find_by!", "find_sole_by"];

/// Ranges of the nodes already reported (ignore_node); their descendants are skipped.
#[derive(Default)]
struct Ignored(Vec<(usize, usize)>);

fn range(n: &Node) -> (usize, usize) {
    (n.location().start_offset(), n.location().end_offset())
}

/// (send nil? :params)
fn is_params(n: &Node) -> bool {
    helpers::rails::send_node(n).is_some_and(|c| helpers::rails::is_command(&c, "params") && send_args(&c).is_empty())
}

/// require_key: `key: ` for a literal key, `source => ` otherwise; and whether the text is safe to
/// splice in as a label.
fn require_key(require: &ruby_prism::CallNode) -> (String, bool) {
    let first = &send_args(require)[0];
    match helpers::rails::node_value(first) {
        Some(v) => {
            static IDENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A[A-Za-z_][A-Za-z0-9_]*\z").unwrap());
            let safe = IDENT.is_match(&v);
            (format!("{v}: "), safe)
        }
        None => (format!("{} => ", node_text(first)), true),
    }
}

/// (call _ :permit {call lvar ivar cvar gvar const}): one argument that is a variable, a constant
/// or a call without a literal block.
fn permit_single_value(permit: &ruby_prism::CallNode) -> bool {
    let args = send_args(permit);
    let [a] = args.as_slice() else { return false };
    a.as_call_node().is_some_and(|c| c.block().is_none_or(|b| b.as_block_node().is_none()))
        || matches!(
            a,
            Node::LocalVariableReadNode { .. }
                | Node::InstanceVariableReadNode { .. }
                | Node::ClassVariableReadNode { .. }
                | Node::GlobalVariableReadNode { .. }
                | Node::ConstantReadNode { .. }
                | Node::ConstantPathNode { .. }
        )
}

impl Cop for StrongParametersExpect {
    fn correctable(&self) -> bool {
        true
    }

    fn min_rails(&self) -> f64 {
        8.0
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = node.as_call_node() else { return };
        let name = method_name(&call);
        if !matches!(name, "[]" | "require" | "permit") {
            return;
        }
        let me = range(node);
        // part_of_ignored_node?
        if cx.state::<Ignored>().0.iter().any(|&(s, e)| s <= me.0 && me.1 <= e && (s, e) != me) {
            return;
        }
        let end = send_range(&call).1;
        if name == "[]" {
            return bracket_access(cx, node, &call, end);
        }
        let args = send_args(&call);
        let (require, permit, replace_argument) = if name == "permit" && !args.is_empty() && !permit_single_value(&call) {
            // params_require_permit
            let Some(require) = call.receiver().as_ref().and_then(|r| r.as_call_node()) else { return };
            let rargs = send_args(&require);
            if method_name(&require) != "require"
                || !require.receiver().is_some_and(|r| is_params(&r))
                || rargs.len() != 1
                || rargs[0].as_array_node().is_some()
                || require.block().is_some_and(|b| b.as_block_node().is_some())
            {
                return;
            }
            (require, call, true)
        } else if name == "require" {
            // params_permit_require
            let Some(permit) = call.receiver().as_ref().and_then(|r| r.as_call_node()) else { return };
            let pargs = send_args(&permit);
            if method_name(&permit) != "permit"
                || !permit.receiver().is_some_and(|r| is_params(&r))
                || pargs.len() != 1
                || args.len() != 1
                || permit.block().is_some_and(|b| b.as_block_node().is_some())
            {
                return;
            }
            let Some(elems) = helpers::rails::hash_elements(&pargs[0]) else { return };
            let [pair] = elems.as_slice() else { return };
            let Some(pair) = pair.as_assoc_node() else { return };
            let src = cx.src;
            if helpers::rails::node_key(src, &pair.key()) != helpers::rails::node_key(src, &args[0]) {
                return;
            }
            (call, permit, false)
        } else {
            return;
        };
        let pargs = send_args(&permit);
        let (os, oe) = (if replace_argument { require.message_loc() } else { permit.message_loc() })
            .map(|l| l.start_offset())
            .map_or((me.0, end), |s| (s, end));
        let joined: Vec<&str> = pargs.iter().map(|a| node_text(a)).collect();
        let joined = joined.join(", ");
        let (key, safe) = if replace_argument { require_key(&require) } else { (String::new(), true) };
        let prefer = if replace_argument { format!("expect({key}[{joined}])") } else { format!("expect({joined})") };
        let msg = format!("Use `{prefer}` instead.");
        // Deliberate difference (docs/known-gaps.md): a literal key becomes a label only when it is one.
        if !safe {
            cx.add_withheld_fix(os, oe, msg);
        } else {
            let remove = (require.receiver().map_or(me.0, |r| send_range_of(&r)), send_range(&require).1);
            let sel = permit.message_loc().map(|l| (l.start_offset(), l.end_offset()));
            let first = pargs.first().map(range);
            let last = pargs.last().map(range);
            cx.add_fix(os, oe, msg, |c| {
                c.remove(remove.0, remove.1);
                if let Some((a, b)) = sel {
                    c.replace(a, b, "expect");
                }
                if replace_argument && let (Some(f), Some(l)) = (first, last) {
                    c.insert_before(f.0, f.1, &format!("{key}["));
                    c.insert_after(l.0, l.1, "]");
                }
            });
        }
        cx.state::<Ignored>().0.push(me);
    }
}

/// The end of a receiver as parser sees it (a call's range stops before its literal block).
fn send_range_of(n: &Node) -> usize {
    n.as_call_node().map_or(n.location().end_offset(), |c| send_range(&c).1)
}

fn bracket_access(cx: &mut Cx, node: &Node, call: &ruby_prism::CallNode, end: usize) {
    // (send (send nil? :params) :[] $_)
    if is_csend(call) || !call.receiver().is_some_and(|r| is_params(&r)) {
        return;
    }
    let args = send_args(call);
    let [key] = args.as_slice() else { return };
    // offensive_bracket_access?
    let anc = cx.ancestors;
    let Some(pi) = parser_parent_index(anc, anc.len()) else { return };
    let parent = &anc[pi];
    if parent.as_or_node().is_some() {
        return;
    }
    let pcall = parent.as_call_node();
    let is_recv = pcall.as_ref().is_some_and(|p| p.receiver().is_some_and(|r| range(&r) == range(node)));
    if is_recv && pcall.as_ref().is_some_and(is_csend) {
        return;
    }
    // parent.each_ancestor(:call): parser send/csend ancestors above the parent (a call owning the
    // block the path goes through is not one).
    let raising = |c: &ruby_prism::CallNode| RAISING_FINDER_METHODS.contains(&method_name(c));
    let above = (0..pi).rev().any(|i| {
        anc[i].as_call_node().is_some_and(|c| anc[i + 1].as_block_node().is_none() && raising(&c))
    });
    let offensive = above
        || match &pcall {
            None => false,
            Some(p) if is_recv => {
                let m = method_name(p);
                !(matches!(m, "==" | "===" | "!=" | "<=" | ">=" | ">" | "<" | "[]")
                    || p.block().is_some()
                    || IGNORED_METHODS.contains(&m))
            }
            Some(p) => raising(p),
        };
    if !offensive {
        return;
    }
    let Some(sel) = call.message_loc() else { return };
    let (os, oe) = (sel.start_offset(), end);
    let replacement = format!(".expect({})", node_text(key));
    cx.add_fix(os, oe, format!("Use `expect({})` instead.", node_text(key)), |c| c.replace(os, oe, &replacement));
}
