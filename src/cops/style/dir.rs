use crate::helpers::code_length::global_const;
use crate::prelude::*;

pub const NAME: &str = "Style/Dir";
pub static COP: Dir = Dir;
pub struct Dir;

/// `(send (const {nil? cbase} :File) method _)`: the single argument.
fn file_call<'pr>(node: &Node<'pr>, method: &str) -> Option<Node<'pr>> {
    let call = node.as_call_node()?;
    if is_csend(&call)
        || method_name(&call) != method
        || call.block().is_some_and(|b| b.as_block_node().is_some())
        || !call.receiver().is_some_and(|r| global_const(&r, &["File"]))
    {
        return None;
    }
    let mut args = send_args(&call);
    if args.len() == 1 { args.pop() } else { None }
}

impl Cop for Dir {
    fn correctable(&self) -> bool {
        true
    }

    fn ruby_range(&self) -> (f64, f64) {
        (2.0, f64::MAX)
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let inner = match method_name(&call) {
            "expand_path" => "dirname",
            "dirname" => "realpath",
            _ => return,
        };
        if is_csend(&call) || !call.receiver().is_some_and(|r| global_const(&r, &["File"])) {
            return;
        }
        let args = send_args(&call);
        if args.len() != 1 {
            return;
        }
        // #file_keyword?: parser makes `__FILE__` a str
        if !file_call(&args[0], inner).is_some_and(|a| a.as_source_file_node().is_some()) {
            return;
        }
        let (s, e) = send_range(&call);
        cx.add_fix(s, e, "Use `__dir__` to get an absolute path to the current file's directory.", |c| {
            c.replace(s, e, "__dir__")
        });
    }
}
