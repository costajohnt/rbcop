use crate::prelude::*;

pub const NAME: &str = "Rails/AssertNot";
pub static COP: AssertNot = AssertNot;
pub struct AssertNot;

static ASSERT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^assert(\(| ) *! *").unwrap());

impl Cop for AssertNot {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send nil? :assert (send ... :!) ...)
        let Some(call) = helpers::rails::send(node) else { return };
        if !helpers::rails::is_command(&call, "assert") {
            return;
        }
        let args = send_args(&call);
        let Some(first) = args.first() else { return };
        if !helpers::rails::send(first).is_some_and(|c| method_name(&c) == "!") {
            return;
        }
        let (s, e) = send_range(&call);
        let corrected = ASSERT.replace_all(cx.src.slice(s, e), "assert_not$1").into_owned();
        cx.add_fix(s, e, "Prefer `assert_not` over `assert !`.", |c| c.replace(s, e, &corrected));
    }
}
