use crate::prelude::*;

pub const NAME: &str = "RSpec/RedundantPredicateMatcher";
pub static COP: RedundantPredicateMatcher = RedundantPredicateMatcher;
pub struct RedundantPredicateMatcher;

impl Cop for RedundantPredicateMatcher {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if is_csend(&call)
            || !matches!(
                name,
                "be_all"
                    | "be_cover"
                    | "be_end_with"
                    | "be_eql"
                    | "be_equal"
                    | "be_exist"
                    | "be_exists"
                    | "be_include"
                    | "be_match"
                    | "be_respond_to"
                    | "be_start_with"
            )
        {
            return;
        }
        // node.parent.block_type? (RuboCop raises without a parent)
        if helpers::rspec::block_of(&call).is_some() {
            return;
        }
        match helpers::parser_parent::parser_parent(cx.ancestors) {
            None => return,
            Some(p) if p.as_block_node().is_some() || p.as_lambda_node().is_some() => return,
            _ => {}
        }
        let args = send_args(&call);
        let Some(first) = args.first() else { return };
        // be_all: the argument must be a send
        if name == "be_all" && helpers::rspec::send_call(first).is_none_or(|c| is_csend(&c)) {
            return;
        }
        let good = match name.strip_prefix("be_").unwrap_or(name) {
            "exists" => "exist",
            n => n,
        };
        let msg = format!("Use `{good}` instead of `{name}`.");
        let (s, e) = send_range(&call);
        let sel = call.message_loc().unwrap();
        let (ss, se) = (sel.start_offset(), sel.end_offset());
        let fix = name != "be_all";
        cx.add_fix(s, e, msg, |c| {
            if fix {
                c.replace(ss, se, good);
            }
        });
    }
}
