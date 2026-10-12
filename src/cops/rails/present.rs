use crate::prelude::*;

pub const NAME: &str = "Rails/Present";
pub static COP: Present = Present;
pub struct Present;

fn recv_of<'pr>(n: &Node<'pr>, m: &str) -> Option<Option<Node<'pr>>> {
    let c = helpers::rails::send_node(n)?;
    (method_name(&c) == m && send_args(&c).is_empty()).then(|| c.receiver())
}

/// (send (send $_ :m) :!)
fn not_of<'pr>(n: &Node<'pr>, m: &str) -> Option<Option<Node<'pr>>> {
    let inner = recv_of(n, "!")??;
    recv_of(&inner, m)
}

fn replacement(r: &Option<Node>) -> String {
    match r {
        Some(r) => format!("{}.present?", node_text(r)),
        None => "present?".to_string(),
    }
}

/// exists_and_not_empty?: (and {!x.nil? !!x x!=nil $_} (send (send $_ :empty?) :!))
fn exists_and_not_empty<'pr>(n: &Node<'pr>) -> Option<(Option<Node<'pr>>, Option<Node<'pr>>)> {
    let a = n.as_and_node()?;
    let (l, r) = (a.left(), a.right());
    let v2 = not_of(&r, "empty?")?;
    let v1 = not_of(&l, "nil?")
        .or_else(|| not_of(&l, "!"))
        .or_else(|| {
            let c = helpers::rails::send_node(&l)?;
            let args = send_args(&c);
            (method_name(&c) == "!=" && args.len() == 1 && args[0].as_nil_node().is_some()).then(|| c.receiver())
        })
        .unwrap_or(Some(l));
    Some((v1, v2))
}

fn same(src: &crate::source::Source, a: &Option<Node>, b: &Option<Node>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => helpers::rails::node_key(src, a) == helpers::rails::node_key(src, b),
        _ => false,
    }
}

impl Cop for Present {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE, kind::AND_NODE, kind::UNLESS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let truthy = |k: &str| cx.cfg.get(k).is_some_and(|v| !matches!(v, Value::Null | Value::Bool(false)));
        let (s, e) = (node.location().start_offset(), node.location().end_offset());
        match node {
            Node::CallNode { .. } => {
                if !truthy("NotBlank") {
                    return;
                }
                let Some(r) = not_of(node, "blank?") else { return };
                let (s, e) = send_range(&node.as_call_node().unwrap());
                let rep = replacement(&r);
                let msg = format!("Use `{rep}` instead of `{}`.", cx.src.slice(s, e));
                cx.add_fix(s, e, msg, |c| c.replace(s, e, &rep));
            }
            Node::AndNode { .. } => {
                if !truthy("NotNilAndNotEmpty") {
                    return;
                }
                let Some((v1, v2)) = exists_and_not_empty(node) else { return };
                if !same(cx.src, &v1, &v2) {
                    return;
                }
                let rep = replacement(&v1);
                let msg = format!("Use `{rep}` instead of `{}`.", cx.src.slice(s, e));
                cx.add_fix(s, e, msg, |c| c.replace(s, e, &rep));
            }
            _ => {
                if !truthy("UnlessBlank") {
                    return;
                }
                let u = node.as_unless_node().unwrap();
                if u.else_clause().is_some() && cx.config.cop_enabled("Style/UnlessElse") {
                    return;
                }
                let cond = u.predicate();
                let Some(r) = recv_of(&cond, "blank?") else { return };
                let kw = u.keyword_loc();
                let (cs, ce) = (cond.location().start_offset(), cond.location().end_offset());
                let range = if u.end_keyword_loc().is_none() { (kw.start_offset(), e) } else { (s, ce) };
                let rep = replacement(&r);
                let msg = format!("Use `if {rep}` instead of `{}`.", cx.src.slice(range.0, range.1));
                let (ks, ke) = (kw.start_offset(), kw.end_offset());
                cx.add_fix(range.0, range.1, msg, |c| {
                    c.replace(ks, ke, "if");
                    c.replace(cs, ce, &rep);
                });
            }
        }
    }
}
