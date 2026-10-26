use crate::prelude::*;
use helpers::capybara::{RegexpExpr, regexp_from_string};
use ruby_prism::CallNode;

pub const NAME: &str = "Capybara/RSpec/CurrentUrlExpectation";
pub static COP: CurrentUrlExpectation = CurrentUrlExpectation;
pub struct CurrentUrlExpectation;

const MSG: &str = "Use `have_current_path` on `page` to wait for the URL.";

/// `(send nil? name ...)`: a receiver-less call without a literal block.
fn nil_send<'pr>(node: &Node<'pr>, name: &str) -> Option<CallNode<'pr>> {
    let c = node.as_call_node()?;
    (!is_csend(&c) && c.receiver().is_none() && method_name(&c) == name && helpers::rspec::block_of(&c).is_none()).then_some(c)
}

/// `(send {(send nil? :page) nil?} :current_url)`
fn url_actual(node: &Node) -> bool {
    node.as_call_node().is_some_and(|c| {
        !is_csend(&c)
            && method_name(&c) == "current_url"
            && send_args(&c).is_empty()
            && helpers::rspec::block_of(&c).is_none()
            && c.receiver().is_none_or(|r| nil_send(&r, "page").is_some_and(|p| send_args(&p).is_empty()))
    })
}

impl Cop for CurrentUrlExpectation {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = nil_send(node, "expect") else { return };
        let [actual] = &send_args(&call)[..] else { return };
        if !url_actual(actual) {
            return;
        }
        // expectation_runner?: (send (send nil? :expect _) {:to :not_to :to_not} _)
        let Some(runner) = helpers::parser_parent::parser_parent(cx.ancestors).and_then(|p| p.as_call_node()) else { return };
        let to = method_name(&runner);
        if is_csend(&runner)
            || !matches!(to, "to" | "not_to" | "to_not")
            || !runner.receiver().is_some_and(|r| r.location().start_offset() == node.location().start_offset())
        {
            return;
        }
        let [matcher] = &send_args(&runner)[..] else { return };
        // supported_matcher?
        let Some(m) = matcher.as_call_node().filter(|m| {
            !is_csend(m) && m.receiver().is_none() && helpers::rspec::block_of(m).is_none() && send_args(m).len() == 1
        }) else {
            return;
        };
        let mname = method_name(&m);
        let first = send_args(&m).remove(0);
        let supported = match mname {
            "eq" | "match" => true,
            "include" | "start_with" | "end_with" => helpers::rspec::is_str(&first),
            _ => false,
        };
        if !supported {
            return;
        }
        let sel = call.message_loc().unwrap();
        let (s, e) = (sel.start_offset(), sel.end_offset());
        // regexp_source / correctable_matcher?
        let mut regexp = None;
        if mname == "match" {
            let is_regexp = first.as_regular_expression_node().is_some() || first.as_interpolated_regular_expression_node().is_some();
            if !is_regexp {
                if !helpers::rspec::is_str(&first) {
                    return cx.add_with(s, e, MSG, false);
                }
                match regexp_from_string(&String::from_utf8_lossy(first.as_string_node().unwrap().unescaped())) {
                    RegexpExpr::Literal(r) => regexp = Some(r),
                    RegexpExpr::Uncorrectable => return cx.add_with(s, e, MSG, false),
                    RegexpExpr::Withheld => return cx.add_withheld_fix(s, e, MSG),
                }
            }
        } else if mname != "eq" {
            return cx.add_with(s, e, MSG, false);
        }
        let (as_, ae) = helpers::rspec::node_range(actual);
        let rsel = runner.message_loc().unwrap();
        let msel = m.message_loc().unwrap();
        let (fs, fe) = helpers::rspec::node_range(&first);
        let path_matcher = if to == "to" { "have_current_path" } else { "have_no_current_path" };
        let is_match = mname == "match";
        cx.add_fix(s, e, MSG, |c| {
            c.replace(as_, ae, "page");
            if to != "to" {
                c.replace(rsel.start_offset(), rsel.end_offset(), "to");
            }
            c.replace(msel.start_offset(), msel.end_offset(), path_matcher);
            if is_match {
                if let Some(r) = &regexp {
                    c.replace(fs, fe, r);
                }
                c.insert_after(fs, fe, ", url: true");
            }
        });
    }
}
