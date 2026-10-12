use crate::prelude::*;
use helpers::minitest::*;

pub const NAME: &str = "Minitest/RedundantMessageArgument";
pub static COP: RedundantMessageArgument = RedundantMessageArgument;
pub struct RedundantMessageArgument;

impl Cop for RedundantMessageArgument {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if call.receiver().is_some() {
            return;
        }
        let Some(&(_, before)) = ARITY.iter().find(|(m, _)| *m == method_name(&call)) else { return };
        let args = send_args(&call);
        if args.len() != before + 1 || args[before].as_nil_node().is_none() {
            return;
        }
        let nil = range(&args[before]);
        let r = if args.len() == 1 {
            nil
        } else {
            // node.arguments.index(nil) finds the first `nil` argument.
            let i = args.iter().position(|a| a.as_nil_node().is_some()).unwrap();
            let prev = range(&args[(i + args.len() - 1) % args.len()]).1;
            (prev.min(nil.1), prev.max(nil.1))
        };
        cx.add_fix(nil.0, nil.1, "Remove the redundant message argument.", |c| c.remove(r.0, r.1));
    }
}

/// The arguments before the message argument of each assertion the cop checks.
const ARITY: &[(&str, usize)] = &[
    ("assert", 1),
    ("assert_empty", 1),
    ("assert_equal", 2),
    ("assert_in_delta", 3),
    ("assert_in_epsilon", 3),
    ("assert_includes", 2),
    ("assert_instance_of", 2),
    ("assert_kind_of", 2),
    ("assert_match", 2),
    ("assert_nil", 1),
    ("assert_operator", 3),
    ("assert_path_exists", 1),
    ("assert_predicate", 2),
    ("assert_respond_to", 2),
    ("assert_same", 2),
    ("assert_throws", 1),
    ("flunk", 0),
    ("refute", 1),
    ("refute_empty", 1),
    ("refute_equal", 2),
    ("refute_in_delta", 3),
    ("refute_in_epsilon", 3),
    ("refute_includes", 2),
    ("refute_instance_of", 2),
    ("refute_kind_of", 2),
    ("refute_match", 2),
    ("refute_nil", 1),
    ("refute_operator", 3),
    ("refute_path_exists", 1),
    ("refute_predicate", 2),
    ("refute_respond_to", 2),
    ("refute_same", 2),
];
