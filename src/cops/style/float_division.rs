use crate::prelude::*;

pub const NAME: &str = "Style/FloatDivision";
pub static COP: FloatDivision = FloatDivision;
pub struct FloatDivision;

/// (send !nil? :to_f)
fn to_f_method(node: &Node) -> bool {
    node.as_call_node().is_some_and(|c| {
        !is_csend(&c)
            && c.receiver().is_some()
            && method_name(&c) == "to_f"
            && c.arguments().is_none()
            && c.block().is_none()
    })
}

/// rubocop-ast `receiver` (nil for anything but a send or block).
fn receiver_of<'pr>(node: &Node<'pr>) -> Option<Node<'pr>> {
    node.as_call_node().and_then(|c| c.receiver())
}

/// (send (const {nil? cbase} :Regexp) :last_match int) / (nth_ref _)
fn regexp_last_match(node: Option<Node>) -> bool {
    let Some(node) = node else { return false };
    if node.as_numbered_reference_read_node().is_some() {
        return true;
    }
    let Some(c) = node.as_call_node() else { return false };
    if is_csend(&c) || method_name(&c) != "last_match" || c.block().is_some() {
        return false;
    }
    let args = call_args(&c);
    let const_ok = c.receiver().is_some_and(|r| match &r {
        Node::ConstantReadNode { .. } => node_text(&r) == "Regexp",
        Node::ConstantPathNode { .. } => {
            let p = r.as_constant_path_node().unwrap();
            p.parent().is_none() && text(&p.name_loc()) == "Regexp"
        }
        _ => false,
    });
    const_ok && args.len() == 1 && helpers::literal_node::folded_kind(&args[0]) == kind::INTEGER_NODE
}

/// The receiver end and the selector start and end of a `x.to_f` call.
fn to_f_parts(node: &Node) -> (usize, usize, usize) {
    let c = node.as_call_node().unwrap();
    let s = c.message_loc().unwrap();
    (c.receiver().unwrap().location().end_offset(), s.start_offset(), s.end_offset())
}

impl Cop for FloatDivision {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "/" {
            return;
        }
        let Some(recv) = call.receiver() else { return };
        let args = send_args(&call);
        let Some(arg) = args.first() else { return };
        if regexp_last_match(receiver_of(&recv)) || regexp_last_match(receiver_of(arg)) {
            return;
        }
        let one_arg = args.len() == 1;
        let (l, r) = (to_f_method(&recv), one_arg && to_f_method(arg));
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("single_coerce");
        let (hit, msg) = match style {
            "left_coerce" => (r, "Prefer using `.to_f` on the left side."),
            "right_coerce" => (l && one_arg, "Prefer using `.to_f` on the right side."),
            "single_coerce" => (l && r, "Prefer using `.to_f` on one side only."),
            "fdiv" => ((l && one_arg) || r, "Prefer using `fdiv` for float divisions."),
            _ => return,
        };
        if !hit {
            return;
        }
        // comment_before_to_f?: a comment on the lines between the receiver and a `to_f` the
        // correction removes (`contains_comment?` is line based).
        let losing: Vec<(&Node, bool)> = match style {
            "left_coerce" | "single_coerce" => vec![(arg, to_f_method(arg))],
            "right_coerce" => vec![(&recv, l)],
            _ => vec![(&recv, l), (arg, to_f_method(arg))],
        };
        if losing.iter().any(|&(n, is_to_f)| {
            is_to_f && {
                let (a, b, _) = to_f_parts(n);
                helpers::comments_help::contains_comments(cx, cx.src.line_of(a), cx.src.line_of(b) + 1)
            }
        }) {
            return;
        }
        let rl = recv.location();
        let al = arg.location();
        let (recv_range, arg_range) = ((rl.start_offset(), rl.end_offset()), (al.start_offset(), al.end_offset()));
        // add_to_f_method skips `send` nodes named to_f (any receiver or arguments).
        let is_to_f_send =
            |n: &Node| n.as_call_node().is_some_and(|c| !is_csend(&c) && c.block().is_none() && method_name(&c) == "to_f");
        let (recv_send_to_f, arg_send_to_f) = (is_to_f_send(&recv), is_to_f_send(arg));
        let recv_parts = l.then(|| to_f_parts(&recv));
        let arg_parts = r.then(|| to_f_parts(arg));
        let arg_paren = arg.as_parentheses_node().is_some();
        let (s, e) = send_range(&call);
        cx.add_fix(s, e, msg, |c| {
            let add = |c: &mut Corrector, range: (usize, usize), skip: bool| {
                if !skip {
                    c.insert_after(range.0, range.1, ".to_f");
                }
            };
            // remove_to_f_method: everything from the receiver's end through `to_f`.
            let remove = |c: &mut Corrector, parts: Option<(usize, usize, usize)>| {
                if let Some((a, _, b)) = parts {
                    c.remove(a, b);
                }
            };
            match style {
                "left_coerce" | "single_coerce" => {
                    add(c, recv_range, recv_send_to_f);
                    remove(c, arg_parts);
                }
                "right_coerce" => {
                    remove(c, recv_parts);
                    add(c, arg_range, arg_send_to_f);
                }
                _ => {
                    remove(c, recv_parts);
                    c.replace(recv_range.1, arg_range.0, if arg_paren { ".fdiv" } else { ".fdiv(" });
                    remove(c, arg_parts);
                    if !arg_paren {
                        c.insert_after(arg_range.0, arg_range.1, ")");
                    }
                }
            }
        });
    }
}
