use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Rails/I18nLocaleTexts";
pub static COP: I18nLocaleTexts = I18nLocaleTexts;
pub struct I18nLocaleTexts;

const MSG: &str = "Move locale texts to the locale files in the `config/locales` directory.";

/// def_node_search (pair (sym {keys}) $str) over a send (receiver and arguments): the str ranges,
/// in pre-order.
fn pair_strs(call: &ruby_prism::CallNode, keys: &[&str]) -> Vec<(usize, usize)> {
    struct F<'k>(&'k [&'k str], Vec<(usize, usize)>);
    impl<'pr> Visit<'pr> for F<'_> {
        fn visit_assoc_node(&mut self, a: &ruby_prism::AssocNode<'pr>) {
            if helpers::rails::sym_value(&a.key()).is_some_and(|k| self.0.contains(&k.as_str()))
                && helpers::rails::str_value(&a.value()).is_some()
            {
                let v = a.value();
                self.1.push((v.location().start_offset(), v.location().end_offset()));
            }
            ruby_prism::visit_assoc_node(self, a);
        }
    }
    let mut f = F(keys, Vec::new());
    if let Some(r) = call.receiver() {
        f.visit(&r);
    }
    for a in send_args(call) {
        f.visit(&a);
    }
    f.1
}

/// (send nil? :flash) / (send (send nil? :flash) :now)
fn flash(n: &Node) -> bool {
    let Some(c) = helpers::rails::send_node(n) else { return false };
    if !send_args(&c).is_empty() {
        return false;
    }
    match method_name(&c) {
        "flash" => c.receiver().is_none(),
        "now" => c.receiver().and_then(|r| helpers::rails::send_node(&r)).is_some_and(|f| {
            helpers::rails::is_command(&f, "flash") && send_args(&f).is_empty()
        }),
        _ => false,
    }
}

impl Cop for I18nLocaleTexts {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        let target = match method_name(&call) {
            "validates" => {
                for (s, e) in pair_strs(&call, &["message"]) {
                    cx.add(s, e, MSG);
                }
                return;
            }
            "redirect_to" | "redirect_back" | "redirect_back_or_to" => pair_strs(&call, &["notice", "alert"]).pop(),
            "[]=" => {
                let args = send_args(&call);
                match args.as_slice() {
                    [_, v] if call.receiver().is_some_and(|r| flash(&r)) && helpers::rails::str_value(v).is_some() => {
                        Some((v.location().start_offset(), v.location().end_offset()))
                    }
                    _ => None,
                }
            }
            "mail" => pair_strs(&call, &["subject"]).pop(),
            _ => None,
        };
        if let Some((s, e)) = target {
            cx.add(s, e, MSG);
        }
    }
}
