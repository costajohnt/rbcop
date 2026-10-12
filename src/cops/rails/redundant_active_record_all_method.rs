use crate::prelude::*;

pub const NAME: &str = "Rails/RedundantActiveRecordAllMethod";
pub static COP: RedundantActiveRecordAllMethod = RedundantActiveRecordAllMethod;
pub struct RedundantActiveRecordAllMethod;

const QUERYING: &[&str] = &[
    "and", "annotate", "any?", "async_average", "async_count", "async_ids", "async_maximum", "async_minimum",
    "async_pick", "async_pluck", "async_sum", "average", "calculate", "count", "create_or_find_by",
    "create_or_find_by!", "create_with", "delete_all", "delete_by", "destroy_all", "destroy_by", "distinct",
    "eager_load", "except", "excluding", "exists?", "extending", "extract_associated", "fifth", "fifth!", "find",
    "find_by", "find_by!", "find_each", "find_in_batches", "find_or_create_by", "find_or_create_by!",
    "find_or_initialize_by", "find_sole_by", "first", "first!", "first_or_create", "first_or_create!",
    "first_or_initialize", "forty_two", "forty_two!", "fourth", "fourth!", "from", "group", "having", "ids",
    "in_batches", "in_order_of", "includes", "invert_where", "joins", "last", "last!", "left_joins",
    "left_outer_joins", "limit", "lock", "many?", "maximum", "merge", "minimum", "none", "none?", "offset", "one?",
    "only", "optimizer_hints", "or", "order", "pick", "pluck", "preload", "readonly", "references", "regroup",
    "reorder", "reselect", "rewhere", "second", "second!", "second_to_last", "second_to_last!", "select", "sole",
    "strict_loading", "sum", "take", "take!", "third", "third!", "third_to_last", "third_to_last!", "touch_all",
    "unscope", "update_all", "where", "with", "without",
];

impl Cop for RedundantActiveRecordAllMethod {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        if method_name(&call) != "all" || !send_args(&call).is_empty() || call.block().is_some() {
            return;
        }
        // followed_by_query_method?(node.parent): (send (send _ :all) QUERYING_METHODS ...)
        let anc = cx.ancestors;
        let Some(pi) = helpers::rails::parent_index(anc) else { return };
        let Some(parent) = helpers::rails::send(&anc[pi]) else { return };
        let is_recv = parent.receiver().is_some_and(|r| r.location().start_offset() == node.location().start_offset() && r.location().end_offset() == node.location().end_offset());
        let pname = method_name(&parent);
        if !is_recv || !QUERYING.contains(&pname) {
            return;
        }
        // possible_enumerable_block_method?
        if matches!(pname, "any?" | "count" | "find" | "none?" | "one?" | "select" | "sum")
            && (parent.block().is_some_and(|b| b.as_block_node().is_some())
                || send_args(&parent).first().is_some_and(|a| a.as_block_argument_node().is_some()))
        {
            return;
        }
        // sensitive_association_method?
        let const_recv = call.receiver().is_some_and(|r| r.as_constant_read_node().is_some() || r.as_constant_path_node().is_some());
        if !const_recv && matches!(pname, "delete_all" | "destroy_all") {
            return;
        }
        match call.receiver() {
            Some(r) => {
                if helpers::allowed_receivers::allowed_receiver(cx, &r) {
                    return;
                }
            }
            None => {
                if !helpers::rails::inherit_active_record_base(anc) {
                    return;
                }
            }
        }
        let sel = call.message_loc().unwrap();
        let (s, e) = (sel.start_offset(), send_range(&call).1);
        let dot = parent.call_operator_loc().map(|d| (d.start_offset(), d.end_offset()));
        cx.add_fix(s, e, "Redundant `all` detected.", |c| {
            c.remove(s, e);
            if let Some((a, b)) = dot {
                c.remove(a, b);
            }
        });
    }
}
