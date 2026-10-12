use crate::prelude::*;
use helpers::parser_parent::parser_parent_index;

pub const NAME: &str = "Performance/RedundantMerge";
pub static COP: RedundantMerge = RedundantMerge;
pub struct RedundantMerge;

impl Cop for RedundantMerge {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send $!nil? :merge! [(hash $...) !kwsplat_type?])
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "merge!" {
            return;
        }
        let Some(recv) = call.receiver() else { return };
        let args = send_args(&call);
        let [hash] = args.as_slice() else { return };
        let pairs: Vec<Node> = match hash {
            Node::HashNode { .. } => hash.as_hash_node().unwrap().elements().iter().collect(),
            Node::KeywordHashNode { .. } => hash.as_keyword_hash_node().unwrap().elements().iter().collect(),
            _ => return,
        };
        // non_redundant_merge?
        let max = match cx.cfg.get("MaxKeyValuePairs") {
            None | Some(Value::Null | Value::Bool(false)) => 2,
            Some(Value::Int(i)) => *i as usize,
            Some(v) => match v.as_string().and_then(|s| s.trim().parse::<i64>().ok()) {
                Some(i) => i as usize,
                None => return,
            },
        };
        if pairs.is_empty()
            || (pairs.len() > 1 && !helpers::pure::pure(&recv))
            || pairs.len() > max
            || pairs.iter().any(|p| p.as_assoc_splat_node().is_some())
        {
            return;
        }
        let start = node.location().start_offset();
        let anc = cx.ancestors;
        // The send of its own block is value-used, and that block is not an each_with_object one.
        let own_block = call.block().is_some_and(|b| b.as_block_node().is_some());
        if own_block || (helpers::value_used::value_used(anc, start) && !each_with_object_memo(anc, &recv)) {
            return;
        }
        let src = cx.src;
        let recv_src = node_text(&recv);
        let assignments: Vec<String> = pairs
            .iter()
            .map(|p| {
                let pair = p.as_assoc_node().unwrap();
                let key = pair.key();
                let colon = pair.operator_loc().is_none();
                let key_src = if colon {
                    // A label: parser's sym excludes the colon.
                    node_text(&key).strip_suffix(':').unwrap_or(node_text(&key))
                } else {
                    node_text(&key)
                };
                let key_src = if colon && key.as_symbol_node().is_some() { format!(":{key_src}") } else { key_src.to_string() };
                let value = pair.value();
                let value_src = if value.as_implicit_node().is_some() {
                    node_text(&key).strip_suffix(':').unwrap_or(node_text(&key))
                } else {
                    node_text(&value)
                };
                format!("{recv_src}[{key_src}] = {value_src}")
            })
            .collect();
        let (s, e) = send_range(&call);
        let msg = format!("Use `{}` instead of `{}`.", assignments.join("; "), src.slice(s, e));
        let leading: String =
            src.lines[src.line_of(s) - 1].chars().take_while(|c| crate::helpers::ruby_string::is_space(*c)).collect();
        let parent = parser_parent_index(anc, anc.len()).map(|i| &anc[i]);
        let (rs, re, replacement) = match parent {
            Some(p) if pairs.len() > 1 => match modifier(p) {
                Some((keyword, condition)) => {
                    let width = indentation_width(cx);
                    let indent = " ".repeat(width);
                    let body = assignments.join(&format!("\n{indent}{leading}"));
                    let text = format!("{keyword} {condition}\n{leading}{indent}{body}\n{leading}end");
                    (p.location().start_offset(), p.location().end_offset(), text)
                }
                None => (s, e, assignments.join(&format!("\n{leading}"))),
            },
            _ => (s, e, assignments.join("\n")),
        };
        cx.add_fix(s, e, msg, |c| c.replace(rs, re, &replacement));
    }
}

/// modifier_flow_control?: a modifier if/unless/while/until, as (keyword, condition source).
fn modifier<'pr>(p: &Node<'pr>) -> Option<(&'pr str, &'pr str)> {
    match p {
        Node::IfNode { .. } => {
            let n = p.as_if_node().unwrap();
            let kw = n.if_keyword_loc()?;
            (n.end_keyword_loc().is_none() && kw.as_slice() == b"if").then(|| (text(&kw), node_text(&n.predicate())))
        }
        Node::UnlessNode { .. } => {
            let n = p.as_unless_node().unwrap();
            n.end_keyword_loc().is_none().then(|| (text(&n.keyword_loc()), node_text(&n.predicate())))
        }
        Node::WhileNode { .. } => {
            let n = p.as_while_node().unwrap();
            (n.closing_loc().is_none() && !n.is_begin_modifier()).then(|| (text(&n.keyword_loc()), node_text(&n.predicate())))
        }
        Node::UntilNode { .. } => {
            let n = p.as_until_node().unwrap();
            (n.closing_loc().is_none() && !n.is_begin_modifier()).then(|| (text(&n.keyword_loc()), node_text(&n.predicate())))
        }
        _ => None,
    }
}

/// Alignment#configured_indentation_width
fn indentation_width(cx: &Cx) -> usize {
    if let Some(w) = cx.cfg.int("IndentationWidth") {
        return w.max(0) as usize;
    }
    cx.config.for_cop("Layout/IndentationWidth").int("Width").map_or(2, |w| w.max(0) as usize)
}

/// EachWithObjectInspector#value_used?: the merge is the body (or last statement) of an
/// `each_with_object(x) { |_, memo| }` block and its receiver chain starts at that memo.
fn each_with_object_memo(anc: &[Node], recv: &Node) -> bool {
    let Some(name) = unwind_lvar(recv) else { return false };
    // node.parent (a `begin` parent's parent): an each_with_object block.
    let Some(mut i) = parser_parent_index(anc, anc.len()) else { return false };
    if anc[i].as_statements_node().is_some_and(|s| s.body().iter().count() > 1) {
        let Some(j) = parser_parent_index(anc, i) else { return false };
        i = j;
    }
    let Some(block) = anc[i].as_block_node() else { return false };
    if block.parameters().is_some_and(|p| p.as_block_parameters_node().is_none()) {
        return false;
    }
    let Some(owner) = i.checked_sub(1).and_then(|k| anc[k].as_call_node()) else { return false };
    if is_csend(&owner) || method_name(&owner) != "each_with_object" || send_args(&owner).len() != 1 {
        return false;
    }
    let Some(ps) = block.parameters().and_then(|p| p.as_block_parameters_node()).and_then(|b| b.parameters()) else {
        return false;
    };
    let mut params = parameter_nodes(&ps);
    params.retain(|n| n.as_implicit_rest_node().is_none());
    let locals = block.parameters().unwrap().as_block_parameters_node().unwrap().locals().iter().count();
    let [_, second] = params.as_slice() else { return false };
    if locals != 0 {
        return false;
    }
    // `loc.name.source` of the second argument.
    let second_name = match second {
        Node::RequiredParameterNode { .. } => Some(second.as_required_parameter_node().unwrap().name().as_slice().to_vec()),
        Node::OptionalParameterNode { .. } => Some(second.as_optional_parameter_node().unwrap().name().as_slice().to_vec()),
        Node::RestParameterNode { .. } => second.as_rest_parameter_node().unwrap().name().map(|n| n.as_slice().to_vec()),
        Node::BlockParameterNode { .. } => second.as_block_parameter_node().unwrap().name().map(|n| n.as_slice().to_vec()),
        _ => None,
    };
    second_name.as_deref() == Some(name.as_slice())
}

/// unwind (receivers of `send`s down to the first non-send), when that is an lvar: its name.
fn unwind_lvar(node: &Node) -> Option<Vec<u8>> {
    match node.as_call_node() {
        Some(c) if !is_csend(&c) && !c.block().is_some_and(|b| b.as_block_node().is_some()) => unwind_lvar(&c.receiver()?),
        _ => node.as_local_variable_read_node().map(|l| l.name().as_slice().to_vec()),
    }
}
