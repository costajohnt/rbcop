use crate::prelude::*;
use helpers::capybara as css;
use ruby_prism::Visit;

pub const NAME: &str = "Capybara/RSpec/SpecificMatcher";
pub static COP: SpecificMatcher = SpecificMatcher;
pub struct SpecificMatcher;

impl Cop for SpecificMatcher {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send nil? _ (str $_) ...)
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if call.receiver().is_some() || !matches!(name, "have_selector" | "have_no_selector" | "have_css" | "have_no_css") {
            return;
        }
        let Some(first) = send_args(&call).into_iter().next().filter(helpers::rspec::is_str) else { return };
        let arg = String::from_utf8_lossy(first.as_string_node().unwrap().unescaped()).into_owned();
        // arg[/^\w+/, 0]: `^` is any line start
        static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^[A-Za-z0-9_]+").unwrap());
        let matcher = match WORD.find(&arg).map(|m| m.as_str()) {
            Some("button") => "button",
            Some("a") => "link",
            Some("table") => "table",
            Some("select") => "select",
            Some("input") => "field",
            _ => return,
        };
        if css::multiple_selectors(&arg) || !css::replaceable_attributes(&css::attributes(&arg)) {
            return;
        }
        // text_with_regexp?: (pair (sym {:text :exact_text}) (regexp ...)) in the send
        struct T(bool, f64);
        impl<'pr> Visit<'pr> for T {
            fn visit_assoc_node(&mut self, n: &ruby_prism::AssocNode<'pr>) {
                let k = n.key();
                let v = n.value();
                self.0 |= k.as_symbol_node().is_some_and(|s| matches!(s.unescaped(), b"text" | b"exact_text"))
                    && !is_parser_dsym(&k, self.1)
                    && (v.as_regular_expression_node().is_some() || v.as_interpolated_regular_expression_node().is_some());
                ruby_prism::visit_assoc_node(self, n);
            }
        }
        let mut t = T(false, cx.src.target_ruby);
        css::visit_send(node, &mut t);
        if t.0 || !css::replaceable_option(cx, node, &arg, matcher) || !css::replaceable_pseudo_classes(&arg) {
            return;
        }
        static SEL: LazyLock<Regex> = LazyLock::new(|| Regex::new("selector|css").unwrap());
        let good = SEL.replace_all(name, matcher);
        let (s, e) = send_range(&call);
        cx.add(s, e, format!("Prefer `{good}` over `{name}`."));
    }
}
