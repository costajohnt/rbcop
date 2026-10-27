use crate::prelude::*;

pub const NAME: &str = "Style/NilComparison";
pub static COP: NilComparison = NilComparison;
pub struct NilComparison;

impl Cop for NilComparison {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || call.receiver().is_none() {
            return;
        }
        let name = method_name(&call);
        let args = call_args(&call);
        let (hit, msg) = if cx.cfg.str("EnforcedStyle") == Some("comparison") {
            // (send _ :nil?)
            (name == "nil?" && args.is_empty(), "Prefer the use of the `==` comparison.")
        } else {
            // (send _ {:== :===} nil)
            ((name == "==" || name == "===") && args.len() == 1 && args[0].as_nil_node().is_some(), "Prefer the use of the `nil?` predicate.")
        };
        let comparison = cx.cfg.str("EnforcedStyle") == Some("comparison");
        let recv = call.receiver().unwrap();
        let recv_end = recv.location().end_offset();
        // comment_before_dot?: `processed_source.contains_comment?` is line-based.
        if comparison
            && let Some(dot) = call.call_operator_loc()
            && helpers::comments_help::contains_comments(cx, cx.src.line_of(recv_end), cx.src.line_of(dot.start_offset()) + 1)
        {
            return;
        }
        let Some(sel) = call.message_loc().filter(|_| hit) else { return };
        let (start, end) = send_range(&call);
        let parent = helpers::parser_parent::parser_parent(cx.ancestors);
        // receiver_of_parent?: `node.parent&.call_type? && node.parent.receiver.equal?(node)`
        let receiver_of_parent = parent.and_then(|p| p.as_call_node()).and_then(|p| p.receiver()).is_some_and(|r| {
            let l = r.location();
            (l.start_offset(), l.end_offset()) == (start, end)
        });
        let wrap = comparison && (parent.is_some_and(operator_expression) || receiver_of_parent);
        let recv_is_operator = operator_expression(&recv);
        let recv_text = node_text(&recv);

        cx.add_fix(sel.start_offset(), sel.end_offset(), msg, |c| {
            if comparison {
                c.replace(recv_end, sel.end_offset(), " == nil");
                if wrap {
                    c.wrap(start, end, "(", ")");
                }
            } else if recv_is_operator {
                c.replace(start, end, &format!("({recv_text}).nil?"));
            } else {
                c.replace(recv_end, end, ".nil?");
            }
        });
    }
}

const OPERATOR_METHODS: &[&str] = &[
    "|", "^", "&", "<=>", "==", "===", "=~", ">", ">=", "<", "<=", "<<", ">>", "+", "-", "*", "/", "%", "**", "~",
    "+@", "-@", "!@", "~@", "!", "!=", "!~", "`",
];

/// NilComparison#operator_expression?
fn operator_expression(node: &Node) -> bool {
    match kind::of(node) {
        kind::CALL_NODE => {
            let call = node.as_call_node().unwrap();
            !is_csend(&call) && OPERATOR_METHODS.contains(&method_name(&call))
        }
        kind::AND_NODE | kind::OR_NODE | kind::RANGE_NODE | kind::FLIP_FLOP_NODE => true,
        kind::IF_NODE => node.as_if_node().unwrap().if_keyword_loc().is_none(),
        kind::MATCH_WRITE_NODE => false,
        k => ASSIGNMENTS.contains(&k),
    }
}

/// rubocop-ast `assignment?` (lvasgn ... casgn, masgn, op_asgn, or_asgn, and_asgn).
const ASSIGNMENTS: &[usize] = &[
    kind::CALL_AND_WRITE_NODE,
    kind::CALL_OPERATOR_WRITE_NODE,
    kind::CALL_OR_WRITE_NODE,
    kind::CLASS_VARIABLE_AND_WRITE_NODE,
    kind::CLASS_VARIABLE_OPERATOR_WRITE_NODE,
    kind::CLASS_VARIABLE_OR_WRITE_NODE,
    kind::CLASS_VARIABLE_WRITE_NODE,
    kind::CONSTANT_AND_WRITE_NODE,
    kind::CONSTANT_OPERATOR_WRITE_NODE,
    kind::CONSTANT_OR_WRITE_NODE,
    kind::CONSTANT_PATH_AND_WRITE_NODE,
    kind::CONSTANT_PATH_OPERATOR_WRITE_NODE,
    kind::CONSTANT_PATH_OR_WRITE_NODE,
    kind::CONSTANT_PATH_WRITE_NODE,
    kind::CONSTANT_WRITE_NODE,
    kind::GLOBAL_VARIABLE_AND_WRITE_NODE,
    kind::GLOBAL_VARIABLE_OPERATOR_WRITE_NODE,
    kind::GLOBAL_VARIABLE_OR_WRITE_NODE,
    kind::GLOBAL_VARIABLE_WRITE_NODE,
    kind::INDEX_AND_WRITE_NODE,
    kind::INDEX_OPERATOR_WRITE_NODE,
    kind::INDEX_OR_WRITE_NODE,
    kind::INSTANCE_VARIABLE_AND_WRITE_NODE,
    kind::INSTANCE_VARIABLE_OPERATOR_WRITE_NODE,
    kind::INSTANCE_VARIABLE_OR_WRITE_NODE,
    kind::INSTANCE_VARIABLE_WRITE_NODE,
    kind::LOCAL_VARIABLE_AND_WRITE_NODE,
    kind::LOCAL_VARIABLE_OPERATOR_WRITE_NODE,
    kind::LOCAL_VARIABLE_OR_WRITE_NODE,
    kind::LOCAL_VARIABLE_WRITE_NODE,
    kind::MULTI_WRITE_NODE,
];
