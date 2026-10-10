use crate::helpers::preceding_following_alignment::{Alignment, Data, Range};
use crate::helpers::range_help::with_surrounding_space;
use crate::prelude::*;
use ruby_prism::Location;

pub const NAME: &str = "Layout/SpaceAroundOperators";
pub static COP: SpaceAroundOperators = SpaceAroundOperators;
pub struct SpaceAroundOperators;

const IRREGULAR_METHODS: &[&str] = &["[]", "!", "[]="];

static SURROUNDED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^[ \t\r\n\x0b\x0c].*[ \t\r\n\x0b\x0c]$").unwrap());

#[derive(Clone, Copy, PartialEq, Eq)]
enum Type {
    Assignment,
    SpecialAsgn,
    Other,
}

fn range(loc: &Location) -> (usize, usize) {
    (loc.start_offset(), loc.end_offset())
}

fn node_range(node: &Node) -> (usize, usize) {
    range(&node.location())
}

fn is_space(b: u8) -> bool {
    crate::helpers::parser_tokens::is_ruby_space(b)
}

fn strip(s: &str) -> &str {
    s.trim_matches(|c: char| c == '\0' || (c.is_ascii() && is_space(c as u8)))
}

impl SpaceAroundOperators {
    /// check_operator(type, operator, right_operand); `rational` = right_operand.rational_type?.
    fn check(&self, cx: &mut Cx, ty: Type, op: (usize, usize), right: (usize, usize), rational: bool) {
        let src = cx.src;
        let (ws, we) = with_surrounding_space(src, op.0, op.1, true, true, true);
        let with_space = src.slice(ws, we);
        if with_space.starts_with('\n') {
            return;
        }
        if let Some(c) = src.comment_on_line(src.line_of(op.0))
            && src.column(we) == src.column(c.start)
        {
            return;
        }
        let op_src = src.slice(op.0, op.1);
        let exp_space = cx.cfg.str("EnforcedStyleForExponentOperator") == Some("space");
        let slash_space = !rational || cx.cfg.str("EnforcedStyleForRationalLiterals") == Some("space");
        let should_not = match op_src {
            "**" => !exp_space,
            "/" => !slash_space,
            _ => false,
        };
        let allow = cx.cfg.bool("AllowForAlignment").unwrap_or(true);
        let force_equal = cx.config.for_cop("Layout/ExtraSpacing").bool("ForceEqualSignAlignment").unwrap_or(false);
        let msg = if should_not {
            if (ws, we) == op {
                return;
            }
            format!("Space around operator `{op_src}` detected.")
        } else if !SURROUNDED.is_match(with_space) {
            format!("Surrounding space missing for operator `{op_src}`.")
        } else {
            let leading = allow && with_space.starts_with("  ");
            let trailing = with_space.ends_with("  ");
            if !leading && !trailing {
                return;
            }
            let program = &cx.ancestors[0];
            let data = cx.state::<Option<Data>>().get_or_insert_with(|| Data::new(src, program));
            let al = Alignment { src, d: data };
            let r = Range { start: op.0, end: op.1 };
            let excess_leading = leading
                && if ty == Type::Assignment || (ty == Type::SpecialAsgn && force_equal) {
                    if al.aligned_with_equals_sign(r, true) == Some(true) {
                        false
                    } else {
                        match al.aligned_with_equals_sign(r, false) {
                            None => false,
                            Some(yes) => !yes,
                        }
                    }
                } else {
                    !al.aligned_with_operator(r)
                };
            let excess_trailing =
                trailing && (!allow || !al.aligned_with_something(Range { start: right.0, end: right.1 }));
            if !excess_leading && !excess_trailing {
                return;
            }
            format!("Operator `{op_src}` should be surrounded by a single space.")
        };
        let stripped = strip(with_space).to_string();
        let ends_nl = with_space.ends_with('\n');
        let ends_space = with_space.ends_with(' ');
        cx.add_fix(op.0, op.1, msg, |c| {
            if stripped == "**" && !exp_space {
                c.replace(ws, we, "**");
            } else if stripped == "/" && !slash_space {
                c.replace(ws, we, "/");
            } else if ends_nl {
                c.replace(ws, we, &format!(" {stripped}\n"));
            } else if force_equal && !ends_space {
                c.insert_after(ws, we, " ");
            } else {
                c.replace(ws, we, &format!(" {stripped} "));
            }
        });
    }

    fn hash_table_style(cx: &Cx) -> bool {
        match cx.config.for_cop("Layout/HashAlignment").get("EnforcedHashRocketStyle") {
            Some(Value::Str(s)) => s == "table",
            Some(Value::Seq(l)) => l.iter().any(|v| matches!(v, Value::Str(s) if s == "table")),
            _ => false,
        }
    }
}

impl Cop for SpaceAroundOperators {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[
            kind::SINGLETON_CLASS_NODE,
            kind::ASSOC_NODE,
            kind::IF_NODE,
            kind::RESCUE_NODE,
            kind::CALL_NODE,
            kind::CLASS_NODE,
            kind::DEF_NODE,
            kind::AND_NODE,
            kind::OR_NODE,
            kind::MATCH_REQUIRED_NODE,
            kind::ALTERNATION_PATTERN_NODE,
            kind::CAPTURE_PATTERN_NODE,
            kind::LOCAL_VARIABLE_WRITE_NODE,
            kind::INSTANCE_VARIABLE_WRITE_NODE,
            kind::CLASS_VARIABLE_WRITE_NODE,
            kind::GLOBAL_VARIABLE_WRITE_NODE,
            kind::CONSTANT_WRITE_NODE,
            kind::CONSTANT_PATH_WRITE_NODE,
            kind::MULTI_WRITE_NODE,
            kind::LOCAL_VARIABLE_OPERATOR_WRITE_NODE,
            kind::INSTANCE_VARIABLE_OPERATOR_WRITE_NODE,
            kind::CLASS_VARIABLE_OPERATOR_WRITE_NODE,
            kind::GLOBAL_VARIABLE_OPERATOR_WRITE_NODE,
            kind::CONSTANT_OPERATOR_WRITE_NODE,
            kind::CONSTANT_PATH_OPERATOR_WRITE_NODE,
            kind::CALL_OPERATOR_WRITE_NODE,
            kind::INDEX_OPERATOR_WRITE_NODE,
            kind::LOCAL_VARIABLE_OR_WRITE_NODE,
            kind::INSTANCE_VARIABLE_OR_WRITE_NODE,
            kind::CLASS_VARIABLE_OR_WRITE_NODE,
            kind::GLOBAL_VARIABLE_OR_WRITE_NODE,
            kind::CONSTANT_OR_WRITE_NODE,
            kind::CONSTANT_PATH_OR_WRITE_NODE,
            kind::CALL_OR_WRITE_NODE,
            kind::INDEX_OR_WRITE_NODE,
            kind::LOCAL_VARIABLE_AND_WRITE_NODE,
            kind::INSTANCE_VARIABLE_AND_WRITE_NODE,
            kind::CLASS_VARIABLE_AND_WRITE_NODE,
            kind::GLOBAL_VARIABLE_AND_WRITE_NODE,
            kind::CONSTANT_AND_WRITE_NODE,
            kind::CONSTANT_PATH_AND_WRITE_NODE,
            kind::CALL_AND_WRITE_NODE,
            kind::INDEX_AND_WRITE_NODE,
        ]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let o = Type::Other;
        match kind::of(node) {
            kind::SINGLETON_CLASS_NODE => {
                let n = node.as_singleton_class_node().unwrap();
                self.check(cx, o, range(&n.operator_loc()), node_range(node), false);
            }
            kind::ASSOC_NODE => {
                let n = node.as_assoc_node().unwrap();
                let Some(op) = n.operator_loc().filter(|l| l.as_slice() == b"=>") else { return };
                if Self::hash_table_style(cx) {
                    let pairs: Vec<Node> = match cx.parent() {
                        Some(p) if p.as_hash_node().is_some() => p.as_hash_node().unwrap().elements().iter().collect(),
                        Some(p) if p.as_keyword_hash_node().is_some() => {
                            p.as_keyword_hash_node().unwrap().elements().iter().collect()
                        }
                        _ => Vec::new(),
                    };
                    let pairs: Vec<(usize, usize)> = pairs
                        .iter()
                        .filter(|p| p.as_assoc_node().is_some())
                        .map(|p| (cx.src.line_of(p.location().start_offset()), cx.src.line_of(p.location().end_offset())))
                        .collect();
                    let same_line = pairs.windows(2).any(|w| w[0].1 == w[1].0 || w[0].0 == w[1].1);
                    if !same_line {
                        return;
                    }
                }
                self.check(cx, o, range(&op), node_range(node), false);
            }
            kind::IF_NODE => {
                let n = node.as_if_node().unwrap();
                if n.if_keyword_loc().is_some() || is_pattern_guard(node, cx.ancestors) {
                    return;
                }
                let (Some(q), Some(then), Some(els)) = (n.then_keyword_loc(), n.statements(), n.subsequent()) else {
                    return;
                };
                let Some(els) = els.as_else_node() else { return };
                self.check(cx, o, range(&q), range(&then.location()), false);
                let right = els.statements().map_or_else(|| range(&els.location()), |s| range(&s.location()));
                self.check(cx, o, range(&els.else_keyword_loc()), right, false);
            }
            kind::RESCUE_NODE => {
                let n = node.as_rescue_node().unwrap();
                if let (Some(op), Some(var)) = (n.operator_loc(), n.reference()) {
                    self.check(cx, o, range(&op), node_range(&var), false);
                }
            }
            kind::CALL_NODE => {
                let call = node.as_call_node().unwrap();
                if is_csend(&call) {
                    return;
                }
                let args = call_args(&call);
                let first = args.first();
                let name = method_name(&call);
                if name == "/"
                    && call.receiver().is_some_and(|r| r.as_integer_node().is_some())
                    && args.len() == 1
                    && first.is_some_and(|a| a.as_rational_node().is_some())
                {
                    return;
                }
                // `/re/ =~ x` is parser's match_with_lvasgn, not a send.
                if name == "=~" && call.receiver().is_some_and(|r| r.as_regular_expression_node().is_some()) {
                    return;
                }
                if let Some(eq) = call.equal_loc() {
                    // node.first_argument: the index for `a[i] = v`, as in RuboCop.
                    if let Some(rhs) = first {
                        self.check(cx, Type::SpecialAsgn, range(&eq), node_range(rhs), false);
                    }
                    return;
                }
                let Some(sel) = call.message_loc() else { return };
                let operator = OPERATOR_METHODS.contains(&name)
                    && !IRREGULAR_METHODS.contains(&name);
                let unary = node.location().start_offset() == sel.start_offset();
                let dotted = call.call_operator_loc().is_some();
                if !operator || unary || dotted {
                    return;
                }
                let Some(arg) = first else { return };
                self.check(cx, o, range(&sel), node_range(arg), arg.as_rational_node().is_some());
            }
            kind::CLASS_NODE => {
                let n = node.as_class_node().unwrap();
                if let (Some(op), Some(sup)) = (n.inheritance_operator_loc(), n.superclass()) {
                    self.check(cx, o, range(&op), node_range(&sup), false);
                }
            }
            kind::DEF_NODE => {
                let n = node.as_def_node().unwrap();
                if let (Some(eq), Some(body)) = (n.equal_loc(), n.body()) {
                    self.check(cx, Type::Assignment, range(&eq), node_range(&body), false);
                }
            }
            kind::AND_NODE => {
                let n = node.as_and_node().unwrap();
                self.check(cx, o, range(&n.operator_loc()), node_range(&n.right()), false);
            }
            kind::OR_NODE => {
                let n = node.as_or_node().unwrap();
                self.check(cx, o, range(&n.operator_loc()), node_range(&n.right()), false);
            }
            kind::MATCH_REQUIRED_NODE => {
                if cx.config.target_ruby < 3.0 {
                    return;
                }
                let n = node.as_match_required_node().unwrap();
                self.check(cx, o, range(&n.operator_loc()), node_range(node), false);
            }
            kind::ALTERNATION_PATTERN_NODE => {
                let n = node.as_alternation_pattern_node().unwrap();
                self.check(cx, o, range(&n.operator_loc()), node_range(node), false);
            }
            kind::CAPTURE_PATTERN_NODE => {
                let n = node.as_capture_pattern_node().unwrap();
                self.check(cx, o, range(&n.operator_loc()), node_range(node), false);
            }
            _ => {
                let Some((op, value)) = assignment_parts(node) else { return };
                let special = matches!(
                    kind::of(node),
                    kind::LOCAL_VARIABLE_OPERATOR_WRITE_NODE
                        | kind::INSTANCE_VARIABLE_OPERATOR_WRITE_NODE
                        | kind::CLASS_VARIABLE_OPERATOR_WRITE_NODE
                        | kind::GLOBAL_VARIABLE_OPERATOR_WRITE_NODE
                        | kind::CONSTANT_OPERATOR_WRITE_NODE
                        | kind::CONSTANT_PATH_OPERATOR_WRITE_NODE
                        | kind::CALL_OPERATOR_WRITE_NODE
                        | kind::INDEX_OPERATOR_WRITE_NODE
                );
                let ty = if special { Type::SpecialAsgn } else { Type::Assignment };
                self.check(cx, ty, range(&op), node_range(&value), false);
            }
        }
    }
}
