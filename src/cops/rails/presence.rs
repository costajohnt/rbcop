use crate::prelude::*;

pub const NAME: &str = "Rails/Presence";
pub static COP: Presence = Presence;
pub struct Presence;

fn recv_of<'pr>(n: &Node<'pr>, m: &str) -> Option<Option<Node<'pr>>> {
    let c = helpers::rails::send_node(n)?;
    (method_name(&c) == m && send_args(&c).is_empty()).then(|| c.receiver())
}

/// {(send $_recv :a) (send (send $_recv :b) :!)}: the receiver (None: receiver-less).
fn cond<'pr>(n: &Node<'pr>, a: &str, b: &str) -> Option<Option<Node<'pr>>> {
    recv_of(n, a).or_else(|| {
        let inner = recv_of(n, "!")??;
        recv_of(&inner, b)
    })
}

/// A parser `begin`: parentheses or a multi-statement body.
fn is_begin(n: &Option<Node>) -> bool {
    n.as_ref().is_some_and(|n| n.as_parentheses_node().is_some() || n.as_statements_node().is_some())
}

fn nilish(n: &Option<Node>) -> bool {
    n.as_ref().is_none_or(|n| n.as_nil_node().is_some())
}

fn ruby_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\r' | '\n' | '\x0b' | '\x0c')
}

/// `gsub(/^\s*|\n/, '')`
fn squash(s: &str) -> String {
    let mut out = String::new();
    let mut line_start = true;
    for c in s.chars() {
        if line_start && ruby_space(c) {
            continue;
        }
        line_start = c == '\n';
        if c != '\n' {
            out.push(c);
        }
    }
    out
}

/// `gsub(/\n\s*/, ' ')`
fn join_lines(s: &str) -> String {
    let mut out = String::new();
    let mut skipping = false;
    for c in s.chars() {
        if skipping && ruby_space(c) {
            continue;
        }
        skipping = false;
        if c == '\n' {
            out.push(' ');
            skipping = true;
        } else {
            out.push(c);
        }
    }
    out
}

impl Cop for Presence {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::IF_NODE, kind::UNLESS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if helpers::if_node::is_elsif(node) || is_pattern_guard(node, cx.ancestors) {
            return;
        }
        let Some((c, t, f)) = helpers::rails::if_children(node) else { return };
        let src = cx.src;
        let key = |n: &Node| helpers::rails::node_key(src, n);
        // `_recv` unification: equal nodes, or both absent.
        let eq = |a: &Option<Node>, r: &Option<Node>| match (a, r) {
            (None, None) => true,
            (Some(a), Some(r)) => key(a) == key(r),
            _ => false,
        };
        let blank = cond(&c, "blank?", "present?");
        let present = cond(&c, "present?", "blank?");
        if blank.is_none() && present.is_none() {
            return;
        }
        let s = node.location().start_offset();
        let e = helpers::if_node::parser_end(node);
        let ternary = helpers::parser_parent::is_ternary(node);
        let source = src.slice(s, e);
        let current = if !ternary && source.contains('\n') {
            let kw = node
                .as_if_node()
                .and_then(|n| n.if_keyword_loc())
                .or_else(|| node.as_unless_node().map(|n| n.keyword_loc()));
            let sel_end = helpers::rails::send_node(&c).and_then(|cc| cc.message_loc()).map(|m| m.end_offset());
            match (kw, sel_end) {
                (Some(k), Some(se)) => format!("{} ... end", src.slice(k.start_offset(), se)),
                _ => return,
            }
        } else {
            join_lines(source)
        };
        let current = squash(&current);
        // require_parentheses?: the parent is a send without parentheses, or an `and`.
        let anc = cx.ancestors;
        let parent_needs = helpers::rails::parent_index(anc).is_some_and(|i| {
            let p = &anc[i];
            p.as_and_node().is_some()
                || helpers::rails::send(p).is_some_and(|pc| pc.closing_loc().is_none_or(|l| l.as_slice() != b")"))
        });

        // redundant_receiver_and_other
        let hit = if let Some(r) = blank.as_ref().filter(|r| !is_begin(&t) && eq(&f, r)) {
            Some((r, &t))
        } else if let Some(r) = present.as_ref().filter(|r| eq(&t, r) && !is_begin(&f)) {
            Some((r, &f))
        } else {
            None
        };
        if let Some((recv, other)) = hit {
            let ignored = other.as_ref().is_some_and(|o| {
                matches!(o, Node::IfNode { .. } | Node::UnlessNode { .. } | Node::RescueModifierNode { .. })
                    || o.as_while_node().is_some_and(|w| !w.is_begin_modifier())
            });
            let Some(recv) = recv.as_ref().filter(|_| !ignored) else { return };
            let or_source = match other {
                None => String::new(),
                Some(o) if o.as_nil_node().is_some() => String::new(),
                Some(o) => match helpers::rails::send_node(o) {
                    Some(oc) => {
                        let args = send_args(&oc);
                        let name = method_name(&oc);
                        let parenthesized = oc.closing_loc().is_some_and(|l| l.as_slice() == b")");
                        if parenthesized || name == "[]" || matches!(name, "+" | "-" | "*" | "/" | "%" | "**") || args.is_empty() {
                            format!(" || {}", node_text(o))
                        } else {
                            let method = src.slice(o.location().start_offset(), args[0].location().start_offset() - 1);
                            let a: Vec<&str> = args.iter().map(|x| node_text(x)).collect();
                            format!(" || {method}({})", a.join(", "))
                        }
                    }
                    None => format!(" || {}", node_text(o)),
                },
            };
            let replaced = format!("{}.presence{or_source}", node_text(recv));
            let replacement = if !or_source.is_empty() && parent_needs { format!("({replaced})") } else { replaced };
            let msg = format!("Use `{}` instead of `{current}`.", squash(&replacement));
            cx.add_fix(s, e, msg, |c| c.replace(s, e, &replacement));
        }

        // redundant_receiver_and_chain
        let chain_of = |n: &Option<Node>, r: &Option<Node>| -> bool {
            n.as_ref()
                .and_then(helpers::rails::send_node)
                .is_some_and(|cc| eq(&cc.receiver(), r))
        };
        let hit = if let Some(r) = blank.as_ref().filter(|r| nilish(&t) && chain_of(&f, r)) {
            Some((r, &f))
        } else if let Some(r) = present.as_ref().filter(|r| chain_of(&t, r) && nilish(&f)) {
            Some((r, &t))
        } else {
            None
        };
        if let Some((recv, chain)) = hit {
            let chain = helpers::rails::send_node(chain.as_ref().unwrap()).unwrap();
            let name = method_name(&chain);
            // assignment? (a setter send) || operator_method?
            let setter = name.ends_with('=') && !matches!(name, "==" | "!=" | "===" | "<=" | ">=");
            if setter || helpers::node_types::is_operator_method(name) {
                return;
            }
            let Some(recv) = recv.as_ref() else { return };
            let mut replacement = format!("{}.presence&.{name}", node_text(recv));
            let args = send_args(&chain);
            if !args.is_empty() {
                let a: Vec<&str> = args.iter().map(|x| node_text(x)).collect();
                replacement += &format!("({})", a.join(", "));
            }
            let msg = format!("Use `{}` instead of `{current}`.", squash(&replacement));
            cx.add_fix(s, e, msg, |c| c.replace(s, e, &replacement));
        }
    }
}
