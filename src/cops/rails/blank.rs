use crate::prelude::*;

pub const NAME: &str = "Rails/Blank";
pub static COP: Blank = Blank;
pub struct Blank;

/// `(send $_ :m)` with no arguments (a plain send): its receiver (None when receiver-less).
fn recv_of<'pr>(n: &Node<'pr>, m: &str) -> Option<Option<Node<'pr>>> {
    let c = helpers::rails::send_node(n)?;
    (method_name(&c) == m && send_args(&c).is_empty()).then(|| c.receiver())
}

/// (send (send $_ :present?) :!)
fn not_present<'pr>(n: &Node<'pr>) -> Option<Option<Node<'pr>>> {
    let inner = recv_of(n, "!")??;
    recv_of(&inner, "present?")
}

fn replacement(r: &Option<Node>) -> String {
    match r {
        Some(r) => format!("{}.blank?", node_text(r)),
        None => "blank?".to_string(),
    }
}

/// nil_or_empty?: (or {!x x.nil? x==nil nil==x} {x.empty? !!x.empty?}) -> (var1, var2)
fn nil_or_empty<'pr>(n: &Node<'pr>) -> Option<(Option<Node<'pr>>, Option<Node<'pr>>)> {
    let o = n.as_or_node()?;
    let (l, r) = (o.left(), o.right());
    let v1 = recv_of(&l, "!").or_else(|| recv_of(&l, "nil?")).or_else(|| {
        let c = helpers::rails::send_node(&l)?;
        let a = send_args(&c);
        if method_name(&c) != "==" || a.len() != 1 {
            return None;
        }
        if a[0].as_nil_node().is_some() {
            return Some(c.receiver());
        }
        c.receiver().filter(|r| r.as_nil_node().is_some()).map(|_| send_args(&c).into_iter().next())
    })?;
    let v2 = recv_of(&r, "empty?").or_else(|| {
        let a = recv_of(&r, "!")??;
        let b = recv_of(&a, "!")??;
        recv_of(&b, "empty?")
    })?;
    Some((v1, v2))
}

fn same(src: &crate::source::Source, a: &Option<Node>, b: &Option<Node>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => helpers::rails::node_key(src, a) == helpers::rails::node_key(src, b),
        _ => false,
    }
}

impl Cop for Blank {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE, kind::OR_NODE, kind::UNLESS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let truthy = |k: &str| cx.cfg.get(k).is_some_and(|v| !matches!(v, Value::Null | Value::Bool(false)));
        let (s, e) = (node.location().start_offset(), node.location().end_offset());
        match node {
            Node::CallNode { .. } => {
                if !truthy("NotPresent") {
                    return;
                }
                let Some(r) = not_present(node) else { return };
                // defining_blank?(node.parent): the body of `def blank?` without arguments.
                let anc = cx.ancestors;
                if let Some(i) = helpers::rails::parent_index(anc)
                    && let Some(d) = anc[i].as_def_node()
                    && d.receiver().is_none()
                    && d.name().as_slice() == b"blank?"
                    && d.parameters().is_none()
                {
                    return;
                }
                let (s, e) = send_range(&node.as_call_node().unwrap());
                let rep = replacement(&r);
                let msg = format!("Use `{rep}` instead of `{}`.", cx.src.slice(s, e));
                cx.add_fix(s, e, msg, |c| c.replace(s, e, &rep));
            }
            Node::OrNode { .. } => {
                if !truthy("NilOrEmpty") {
                    return;
                }
                let Some((v1, v2)) = nil_or_empty(node) else { return };
                if !same(cx.src, &v1, &v2) {
                    return;
                }
                let rep = replacement(&v1);
                let msg = format!("Use `{rep}` instead of `{}`.", cx.src.slice(s, e));
                cx.add_fix(s, e, msg, |c| c.replace(s, e, &rep));
            }
            _ => {
                if !truthy("UnlessPresent") {
                    return;
                }
                let u = node.as_unless_node().unwrap();
                if u.else_clause().is_some() && cx.config.cop_enabled("Style/UnlessElse") {
                    return;
                }
                let cond = u.predicate();
                let Some(r) = recv_of(&cond, "present?") else { return };
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
