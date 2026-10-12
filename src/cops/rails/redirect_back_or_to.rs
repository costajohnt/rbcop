use crate::prelude::*;

pub const NAME: &str = "Rails/RedirectBackOrTo";
pub static COP: RedirectBackOrTo = RedirectBackOrTo;
pub struct RedirectBackOrTo;

impl Cop for RedirectBackOrTo {
    fn correctable(&self) -> bool {
        true
    }

    fn min_rails(&self) -> f64 {
        7.0
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send nil? :redirect_back (hash <$(pair (sym :fallback_location) $_) $...>))
        let Some(call) = helpers::rails::send(node) else { return };
        if !helpers::rails::is_command(&call, "redirect_back") {
            return;
        }
        let args = send_args(&call);
        let [hash] = args.as_slice() else { return };
        let Some(elems) = helpers::rails::hash_elements(hash) else { return };
        let Some(fi) = elems.iter().position(|e| {
            e.as_assoc_node().is_some_and(|p| helpers::rails::sym_value(&p.key()).as_deref() == Some("fallback_location"))
        }) else {
            return;
        };
        let src = cx.src;
        let range = |n: &Node| (n.location().start_offset(), n.location().end_offset());
        let fallback = range(&elems[fi]);
        let value = node_text(&elems[fi].as_assoc_node().unwrap().value()).to_string();
        let options: Vec<&str> = elems.iter().enumerate().filter(|(i, _)| *i != fi).map(|(_, e)| node_text(e)).collect();
        let pairs: Vec<(usize, usize)> = elems.iter().filter(|e| e.as_assoc_node().is_some()).map(range).collect();
        let hash_range = range(hash);
        let sel = call.message_loc().unwrap();
        let (ss, se) = (sel.start_offset(), sel.end_offset());
        let parenthesized = call.opening_loc().is_some();
        let (_, send_end) = send_range(&call);
        // pairs.index(fallback_pair): the first structurally equal pair.
        let index = pairs.iter().position(|&p| helpers::node_equality::same(src, p, fallback)).unwrap_or(0);
        let first_other = pairs.iter().copied().find(|&p| !helpers::node_equality::same(src, p, fallback));
        let msg = "Use `redirect_back_or_to` instead of `redirect_back` with `:fallback_location` keyword argument.";
        cx.add_fix(ss, se, msg, |c| {
            c.replace(ss, se, "redirect_back_or_to");
            if pairs.len() == 1 {
                let mut a = vec![value.clone()];
                a.extend(options.iter().map(|s| s.to_string()));
                c.replace(hash_range.0, hash_range.1, &a.join(", "));
            } else {
                if index == 0 {
                    c.remove(fallback.0, pairs[1].0);
                } else {
                    c.remove(pairs[index - 1].1, fallback.1);
                }
                if let Some(fp) = first_other {
                    c.insert_before(fp.0, fp.1, &format!("{value}, "));
                }
            }
            if !parenthesized {
                c.replace(se, hash_range.0, "(");
                c.insert_after(send_end, send_end, ")");
            }
        });
    }
}
