use crate::helpers::parser_parent::is_top_const;
use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Style/SignalException";
pub static COP: SignalException = SignalException;
pub struct SignalException;

const FAIL_MSG: &str = "Use `fail` instead of `raise` to signal exceptions.";
const RAISE_MSG: &str = "Use `raise` instead of `fail` to rethrow exceptions.";

/// `custom_fail_methods`: `{(def :fail ...) (defs _ :fail ...)}` anywhere in the file.
#[derive(Default)]
struct CustomFail(bool);

impl<'pr> Visit<'pr> for CustomFail {
    fn visit_def_node(&mut self, node: &ruby_prism::DefNode<'pr>) {
        self.0 |= node.name().as_slice() == b"fail";
        ruby_prism::visit_def_node(self, node);
    }
}

/// Whether the parser `rescue` node nearest to the call has it inside one of its `resbody`s.
fn in_resbody(node: &Node, ancestors: &[Node]) -> bool {
    let start = node.location().start_offset();
    let within = |n: &Node| {
        let l = n.location();
        l.start_offset() <= start && start < l.end_offset()
    };
    for a in ancestors.iter().rev() {
        if a.as_rescue_node().is_some() {
            return true;
        }
        if let Some(m) = a.as_rescue_modifier_node() {
            return within(&m.rescue_expression());
        }
        if let Some(b) = a.as_begin_node().filter(|b| b.rescue_clause().is_some())
            && !b.ensure_clause().is_some_and(|e| within(&e.as_node()))
        {
            return false;
        }
    }
    false
}

impl Cop for SignalException {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if is_csend(&call) || !matches!(name, "raise" | "fail") {
            return;
        }
        // command?(name) || kernel_call?(node, name)
        if !call.receiver().is_none_or(|r| is_top_const(&r, "Kernel")) {
            return;
        }
        let Some(sel) = call.message_loc() else { return };
        let (msg, fixed) = match cx.cfg.str("EnforcedStyle").unwrap_or("only_raise") {
            "semantic" => {
                let resbody = in_resbody(node, cx.ancestors);
                match (name, resbody) {
                    ("raise", false) => (FAIL_MSG, "fail"),
                    ("fail", true) => (RAISE_MSG, "raise"),
                    _ => return,
                }
            }
            "only_fail" if name == "raise" => ("Always use `fail` to signal exceptions.", "fail"),
            "only_raise" if name == "fail" => {
                if cx.state::<Option<bool>>().is_none() {
                    let mut v = CustomFail::default();
                    v.visit(&cx.ancestors[0]);
                    *cx.state::<Option<bool>>() = Some(v.0);
                }
                if cx.state::<Option<bool>>() == &Some(true) {
                    return;
                }
                ("Always use `raise` to signal exceptions.", "raise")
            }
            _ => return,
        };
        let (s, e) = (sel.start_offset(), sel.end_offset());
        cx.add_fix(s, e, msg, |c| c.replace(s, e, fixed));
    }
}
