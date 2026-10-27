use crate::prelude::*;

pub const NAME: &str = "Style/NegativeArrayIndex";
pub static COP: NegativeArrayIndex = NegativeArrayIndex;
pub struct NegativeArrayIndex;

/// rubocop-ast `receiver` on any node: a call's receiver, nil elsewhere.
fn receiver<'pr>(n: &Node<'pr>) -> Option<Node<'pr>> {
    n.as_call_node().and_then(|c| c.receiver())
}

/// `(send (send $_ {:length :size :count}) :- (int $_))`: (length receiver, index).
fn length_subtraction<'pr>(n: &Node<'pr>) -> Option<(Option<Node<'pr>>, i128)> {
    let minus = n.as_call_node()?;
    if method_name(&minus) != "-" || is_csend(&minus) || minus.block().is_some() {
        return None;
    }
    let args = call_args(&minus);
    let [arg] = args.as_slice() else { return None };
    let index = int_literal_value(arg)?;
    let len = minus.receiver()?.as_call_node()?;
    let ok = matches!(method_name(&len), "length" | "size" | "count")
        && !is_csend(&len)
        && len.arguments().is_none()
        && len.block().is_none()
        && len.message_loc().is_some();
    ok.then(|| (len.receiver(), index))
}

/// `preserving_method?`: no receiver, or a chain of sort/reverse/shuffle/rotate down to one.
fn preserving(n: &Node) -> bool {
    let Some(recv) = receiver(n) else { return true };
    let name = method_name(&n.as_call_node().unwrap());
    matches!(name, "sort" | "reverse" | "shuffle" | "rotate") && preserving(&recv)
}

/// `extract_base_receiver`: the innermost receiver of a call chain.
fn extract_base_receiver<'pr>(n: &Node<'pr>) -> Option<Node<'pr>> {
    let r = receiver(n)?;
    if receiver(&r).is_none() { Some(r) } else { extract_base_receiver(&r) }
}

/// Parser `begin` (parentheses) unwrapped to its first child.
fn first_child<'pr>(n: &Node<'pr>) -> Option<Node<'pr>> {
    let body = n.as_parentheses_node()?.body()?;
    match body.as_statements_node() {
        Some(s) => s.body().iter().next(),
        None => Some(body),
    }
}

fn single_child<'pr>(n: &Node<'pr>) -> Option<Node<'pr>> {
    let body = n.as_parentheses_node()?.body()?;
    match body.as_statements_node() {
        Some(s) => {
            let v: Vec<Node> = s.body().iter().collect();
            (v.len() == 1).then(|| v.into_iter().next().unwrap())
        }
        None => Some(body),
    }
}

impl Cop for NegativeArrayIndex {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "[]" {
            return;
        }
        let Some(index_arg) = send_args(&call).into_iter().next() else { return };
        let Some(array_recv) = call.receiver() else { return };
        let recv_src = node_text(&array_recv);
        let parens = index_arg.as_parentheses_node().is_some();
        let range_node = if parens { first_child(&index_arg) } else { send_args(&call).into_iter().next() };
        if let Some(range) = range_node.as_ref().and_then(|r| r.as_range_node())
            && let (Some(start), Some(end)) = (range.left(), range.right()) {
                let Some(inner) = single_child(&end).or_else(|| range.right()) else { return };
                if preserving(&start)
                    && let Some((len_recv, index)) = length_subtraction(&inner).filter(|&(_, i)| i > 0) {
                        // receivers_match_strict? (a missing length receiver crashes RuboCop)
                        let Some(len_recv) = len_recv else { return };
                        if preserving(&array_recv) && node_text(&len_recv) == recv_src {
                            let op = if range.is_exclude_end() { "..." } else { ".." };
                            let start_src = node_text(&start);
                            let end_src = if end.as_parentheses_node().is_some() { node_text(&end) } else { node_text(&inner) };
                            let without = format!("{start_src}{op}{end_src}");
                            let (current, s, i, repl) = if parens {
                                (
                                    format!("{recv_src}[({without})]"),
                                    format!("({start_src}"),
                                    format!("{index})"),
                                    format!("({start_src}{op}-{index})"),
                                )
                            } else {
                                (format!("{recv_src}[{without}]"), start_src.to_string(), index.to_string(), format!("{start_src}{op}-{index}"))
                            };
                            let msg = format!("Use `{recv_src}[{s}{op}-{i}]` instead of `{current}`.");
                            let (es, ee) = (end.location().start_offset(), end.location().end_offset());
                            let (is, ie) = (index_arg.location().start_offset(), index_arg.location().end_offset());
                            cx.add_fix(es, ee, msg, |c| c.replace(is, ie, &repl));
                            return;
                        }
                    }
            }
        let Some((len_recv, index)) = length_subtraction(&index_arg) else { return };
        if index <= 0 {
            return;
        }
        let matched = match len_recv {
            None => array_recv.as_self_node().is_some(),
            Some(l) => {
                preserving(&array_recv)
                    && preserving(&l)
                    && (node_text(&l) == recv_src
                        || extract_base_receiver(&array_recv).is_some_and(|base| {
                            node_text(&base) == node_text(&extract_base_receiver(&l).unwrap_or(l))
                        }))
            }
        };
        if !matched {
            return;
        }
        let (s, e) = (index_arg.location().start_offset(), index_arg.location().end_offset());
        let msg = format!("Use `{recv_src}[-{index}]` instead of `{recv_src}[{}]`.", node_text(&index_arg));
        cx.add_fix(s, e, msg, |c| c.replace(s, e, &format!("-{index}")));
    }
}
