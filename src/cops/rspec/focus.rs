use crate::prelude::*;

pub const NAME: &str = "RSpec/Focus";
pub static COP: Focus = Focus;
pub struct Focus;

impl Cop for Focus {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if !helpers::rspec::rspec_recv(&call) {
            return;
        }
        // node.chained?: the receiver of its parser parent call (a send in a block is not)
        if helpers::rspec::block_of(&call).is_none()
            && cx.ancestors.last().and_then(|p| p.as_call_node()).is_some_and(|p| {
                p.receiver().is_some_and(|r| helpers::rspec::node_range(&r) == helpers::rspec::node_range(node))
            })
        {
            return;
        }
        if cx.ancestors.iter().any(|a| a.as_def_node().is_some()) {
            return;
        }
        let l = helpers::rspec::lang(cx);
        let name = method_name(&call);
        let has = |list: &[String], n: &str| list.iter().any(|x| x == n);
        // focused_block?: (send #rspec? {#ExampleGroups.focused #Examples.focused} ...)
        if has(&l.eg_focused, name) || has(&l.ex_focused, name) {
            let sel = call.message_loc().unwrap();
            let (ss, se) = (sel.start_offset(), sel.end_offset());
            let unfocused = name.strip_prefix('f').unwrap_or(name);
            let fix = (has(&l.ex_regular, unfocused) || has(&l.eg_regular, unfocused))
                .then(|| cx.src.slice(ss, se).replacen(name, unfocused, 1));
            let (s, e) = send_range(&call);
            cx.add_fix(s, e, "Focused spec found.", |c| {
                if let Some(f) = &fix {
                    c.replace(ss, se, f);
                }
            });
            return;
        }
        // metadata: focusable selector with a `:focus` argument or a `focus: true` pair
        let focusable = has(&l.eg_regular, name)
            || has(&l.eg_skipped, name)
            || has(&l.ex_regular, name)
            || has(&l.ex_skipped, name)
            || has(&l.ex_pending, name)
            || l.shared_group(name);
        if !focusable {
            return;
        }
        let args = send_args(&call);
        let sym_focus = |n: &Node| helpers::rspec::is_sym(cx, n) && helpers::rails::sym_value(n).as_deref() == Some("focus");
        let focus = args.iter().find(|a| sym_focus(a)).map(helpers::rspec::node_range).or_else(|| {
            let hash = args.last()?;
            let pairs = helpers::rails::hash_elements(hash)?;
            pairs.iter().filter_map(|p| p.as_assoc_node()).find_map(|p| {
                (sym_focus(&p.key()) && p.value().as_true_node().is_some()).then(|| helpers::rspec::node_range(&p.as_node()))
            })
        });
        let Some((s, e)) = focus else { return };
        let (rs, re) = helpers::rails::left_space_and_comma(cx.src.text, s, e);
        cx.add_fix(s, e, "Focused spec found.", |c| c.remove(rs, re));
    }
}
