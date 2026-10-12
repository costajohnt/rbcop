use crate::prelude::*;

pub const NAME: &str = "Performance/ConstantRegexp";
pub static COP: ConstantRegexp = ConstantRegexp;
pub struct ConstantRegexp;

impl Cop for ConstantRegexp {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::INTERPOLATED_REGULAR_EXPRESSION_NODE, kind::INTERPOLATED_MATCH_LAST_LINE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let (parts, closing) = match node.as_interpolated_regular_expression_node() {
            Some(r) => (r.parts(), r.closing_loc()),
            None => {
                let r = node.as_interpolated_match_last_line_node().unwrap();
                (r.parts(), r.closing_loc())
            }
        };
        // single_interpolation?: already `/o`.
        if text(&closing).contains('o') {
            return;
        }
        // within_allowed_assignment?: a casgn or or_asgn ancestor.
        if cx.ancestors.iter().any(|a| {
            matches!(
                kind::of(a),
                kind::CONSTANT_WRITE_NODE
                    | kind::CONSTANT_PATH_WRITE_NODE
                    | kind::LOCAL_VARIABLE_OR_WRITE_NODE
                    | kind::INSTANCE_VARIABLE_OR_WRITE_NODE
                    | kind::CLASS_VARIABLE_OR_WRITE_NODE
                    | kind::GLOBAL_VARIABLE_OR_WRITE_NODE
                    | kind::CONSTANT_OR_WRITE_NODE
                    | kind::CONSTANT_PATH_OR_WRITE_NODE
                    | kind::CALL_OR_WRITE_NODE
                    | kind::INDEX_OR_WRITE_NODE
            )
        }) {
            return;
        }
        // include_interpolated_const?: every `#{}` starts with a constant or Regexp.escape(CONST).
        let mut any = false;
        for part in parts.iter() {
            let Some(e) = part.as_embedded_statements_node() else { continue };
            any = true;
            let first = e.statements().and_then(|s| s.body().iter().next());
            if !first.is_some_and(|f| constant(&f) || regexp_escape(&f)) {
                return;
            }
        }
        if !any {
            return;
        }
        let loc = node.location();
        let end = loc.end_offset();
        cx.add_fix(
            loc.start_offset(),
            end,
            "Extract this regexp into a constant, memoize it, or append an `/o` option to its options.",
            |c| c.insert_after(loc.start_offset(), end, "o"),
        );
    }
}

fn constant(node: &Node) -> bool {
    node.as_constant_read_node().is_some() || node.as_constant_path_node().is_some()
}

/// (send (const nil? :Regexp) :escape const_type?)
fn regexp_escape(node: &Node) -> bool {
    let Some(call) = node.as_call_node() else { return false };
    !is_csend(&call)
        && method_name(&call) == "escape"
        && call.receiver().and_then(|r| r.as_constant_read_node()).is_some_and(|c| c.name().as_slice() == b"Regexp")
        && call.block().is_none()
        && matches!(call_args(&call).as_slice(), [a] if constant(a))
}
