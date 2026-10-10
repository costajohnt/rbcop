use crate::prelude::*;
use ruby_prism::CallNode;

pub const NAME: &str = "Style/NestedFileDirname";
pub static COP: NestedFileDirname = NestedFileDirname;
pub struct NestedFileDirname;

/// `(send (const {cbase nil?} :File) :dirname ...)`
fn file_dirname<'pr>(node: &Node<'pr>) -> Option<CallNode<'pr>> {
    let c = node.as_call_node()?;
    let ok = method_name(&c) == "dirname"
        && !is_csend(&c)
        && c.receiver().is_some_and(|r| helpers::parser_parent::is_top_const(&r, "File"));
    ok.then_some(c)
}

fn first_arg<'pr>(c: &CallNode<'pr>) -> Option<Node<'pr>> {
    send_args(c).into_iter().next()
}

impl Cop for NestedFileDirname {
    fn correctable(&self) -> bool {
        true
    }

    fn ruby_range(&self) -> (f64, f64) {
        (3.1, f64::MAX)
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "dirname" || is_csend(&call) {
            return;
        }
        if helpers::parser_parent::parser_parent(cx.ancestors).is_some_and(|p| file_dirname(p).is_some()) {
            return;
        }
        let Some(mut inner) = first_arg(&call).and_then(|a| file_dirname(&a)) else { return };
        let mut level = 2;
        let path = loop {
            let Some(arg) = first_arg(&inner) else { return };
            match file_dirname(&arg) {
                Some(next) => {
                    level += 1;
                    inner = next;
                }
                None => break node_text(&arg),
            }
        };
        let Some(sel) = call.message_loc() else { return };
        let (s, e) = (sel.start_offset(), send_range(&call).1);
        let msg = format!("Use `dirname({path}, {level})` instead.");
        let repl = format!("dirname({path}, {level})");
        cx.add_fix(s, e, msg, |c| c.replace(s, e, &repl));
    }
}
