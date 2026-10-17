use crate::prelude::*;

pub const NAME: &str = "FactoryBot/RedundantFactoryOption";
pub static COP: RedundantFactoryOption = RedundantFactoryOption;
pub struct RedundantFactoryOption;

impl Cop for RedundantFactoryOption {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send nil? :association (sym $_name) ... (hash <$(pair (sym :factory) {(sym $_name) (array (sym $_name))}) ...>))
        let call = node.as_call_node().unwrap();
        if call.receiver().is_some() || method_name(&call) != "association" {
            return;
        }
        let args = send_args(&call);
        if args.len() < 2 {
            return;
        }
        let first = &args[0];
        if !helpers::rspec::is_sym(cx, first) {
            return;
        }
        let name = helpers::rails::sym_value(first);
        let Some(pairs) = helpers::rails::hash_elements(args.last().unwrap()) else { return };
        let sym_of = |n: &Node| helpers::rspec::is_sym(cx, n).then(|| helpers::rails::sym_value(n)).flatten();
        let option = pairs.iter().filter_map(|p| p.as_assoc_node()).find(|p| {
            if sym_of(&p.key()).as_deref() != Some("factory") {
                return false;
            }
            let v = p.value();
            let factory = match v.as_array_node() {
                Some(a) => match &a.elements().iter().collect::<Vec<_>>()[..] {
                    [s] => sym_of(s),
                    _ => None,
                },
                None => sym_of(&v),
            };
            factory.is_some()
        });
        let Some(option) = option else { return };
        let v = option.value();
        let factory = match v.as_array_node() {
            Some(a) => a.elements().iter().next().and_then(|s| sym_of(&s)),
            None => sym_of(&v),
        };
        if factory != name {
            return;
        }
        let (s, e) = helpers::rspec::node_range(&option.as_node());
        let (rs, re) = helpers::rails::left_space_and_comma(cx.src.text, s, e);
        cx.add_fix(s, e, "Remove redundant `factory` option.", |c| c.remove(rs, re));
    }
}
