use crate::prelude::*;
use helpers::capybara::{negative_matcher, positive_matcher};

pub const NAME: &str = "Capybara/RSpec/NegationMatcher";
pub static COP: NegationMatcher = NegationMatcher;
pub struct NegationMatcher;

impl Cop for NegationMatcher {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let matcher = method_name(&call);
        if is_csend(&call) || !(positive_matcher(matcher) || negative_matcher(matcher)) || send_args(&call).is_empty() {
            return;
        }
        if helpers::rspec::block_of(&call).is_some() {
            return;
        }
        let Some(parent) = helpers::parser_parent::parser_parent(cx.ancestors).and_then(|p| p.as_call_node()) else { return };
        if is_csend(&parent) {
            return;
        }
        // (send ... runner (send nil? MATCHERS ...)): one argument, a matcher call
        let [arg] = &send_args(&parent)[..] else { return };
        let arg_matcher = |pred: fn(&str) -> bool| {
            arg.as_call_node()
                .is_some_and(|c| !is_csend(&c) && c.receiver().is_none() && pred(method_name(&c)) && helpers::rspec::block_of(&c).is_none())
        };
        let runner = method_name(&parent);
        let (new_runner, new_matcher) = match cx.cfg.str("EnforcedStyle") {
            Some("have_no") if matches!(runner, "not_to" | "to_not") && arg_matcher(positive_matcher) => {
                ("to", matcher.replacen("have_", "have_no_", 1))
            }
            Some("not_to") if runner == "to" && arg_matcher(negative_matcher) => ("not_to", matcher.replacen("have_no_", "have_", 1)),
            _ => return,
        };
        let psel = parent.message_loc().unwrap();
        let sel = call.message_loc().unwrap();
        let (s, e) = (psel.start_offset(), sel.end_offset());
        cx.add_fix(s, e, format!("Use `expect(...).{new_runner} {new_matcher}`."), |c| {
            c.replace(psel.start_offset(), psel.end_offset(), new_runner);
            c.replace(sel.start_offset(), sel.end_offset(), &new_matcher);
        });
    }
}
