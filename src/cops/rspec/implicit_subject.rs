use crate::prelude::*;

pub const NAME: &str = "RSpec/ImplicitSubject";
pub static COP: ImplicitSubject = ImplicitSubject;
pub struct ImplicitSubject;

impl Cop for ImplicitSubject {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if is_csend(&call) || call.receiver().is_some() || !matches!(name, "expect" | "is_expected" | "should" | "should_not") {
            return;
        }
        let l = helpers::rspec::lang(cx);
        // example_of(node): the nearest ancestor that is an example
        let example = || cx.ancestors.iter().rev().find(|a| helpers::rspec::example(&l, a));
        let implicit = matches!(name, "should" | "should_not" | "is_expected");
        let its = || example().is_some_and(|e| method_name(&e.as_call_node().unwrap()) == "its");
        let style = cx.cfg.str("EnforcedStyle");
        let invalid = match style {
            Some("require_implicit") => {
                // (send nil? :expect (send nil? :subject))
                name == "expect"
                    && matches!(&send_args(&call)[..], [a] if helpers::rspec::bare_send(a, "subject").is_some())
            }
            Some("disallow") => implicit && !its(),
            Some("single_line_only") => {
                implicit && !its() && !example().is_some_and(|e| helpers::rspec::single_line(cx.src, e))
            }
            Some("single_statement_only") => {
                implicit
                    && !its()
                    && example().is_some_and(|e| {
                        helpers::rspec::is_begin_body(&helpers::rspec::block_body(&e.as_call_node().unwrap()))
                    })
            }
            // `:noop` is truthy
            _ => true,
        };
        if !invalid {
            return;
        }
        let msg = if style == Some("require_implicit") { "Don't use explicit subject." } else { "Don't use implicit subject." };
        let (s, e) = send_range(&call);
        let sel = call.message_loc().unwrap();
        let (ss, se) = (sel.start_offset(), sel.end_offset());
        let edit: (usize, usize, &str) = match name {
            "expect" => (s, e, "is_expected"),
            "is_expected" => (ss, se, "expect(subject)"),
            "should" => (ss, se, "expect(subject).to"),
            _ => (ss, se, "expect(subject).not_to"),
        };
        cx.add_fix(s, e, msg, |c| c.replace(edit.0, edit.1, edit.2));
    }
}
