use crate::helpers::sorbet_rbi_type::{Error, rbs_of};
use crate::helpers::sorbet_rbs_assertion::rbs_assertion_edits;
use crate::prelude::*;

pub const NAME: &str = "Sorbet/ForbidTLet";
pub static COP: ForbidTLet = ForbidTLet;
pub struct ForbidTLet;

const MSG: &str = "Do not use `T.let`.";

/// `(send (const nil? :T) :let _ _)` (a block-pass counts as an argument).
fn t_let(node: &Node) -> bool {
    node.as_call_node().is_some_and(|c| {
        !is_csend(&c)
            && method_name(&c) == "let"
            && c.receiver().is_some_and(|r| r.as_constant_read_node().is_some_and(|k| k.name().as_slice() == b"T"))
            && call_args(&c).len() + usize::from(c.block().is_some_and(|b| b.as_block_argument_node().is_some())) == 2
            && c.block().is_none_or(|b| b.as_block_node().is_none())
    })
}

impl Cop for ForbidTLet {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if !t_let(node) {
            return;
        }
        let call = node.as_call_node().unwrap();
        let r = send_range(&call);
        let mut args = call_args(&call);
        if let Some(b) = call.block() {
            args.push(b);
        }
        // autocorrect_t_let_to_rbs: not a nested T.let; a type RBI can parse.
        let mut raises = false;
        let fix = if t_let(&args[0]) {
            Vec::new()
        } else {
            rbs_assertion_edits(cx, &call, true, || match rbs_of(node_text(&args[1])) {
                Ok(ann) => Some((node_text(&args[0]).to_string(), ann)),
                Err(Error::Type) => None,
                Err(Error::Other) => {
                    raises = true;
                    None
                }
            })
        };
        if raises {
            return crate::runner::cop_error_at(NAME, cx.src, crate::runner::visiting());
        }
        cx.add_fix(r.0, r.1, MSG, |c| {
            for (s, e, t) in &fix {
                c.replace(*s, *e, t);
            }
        });
    }
}
