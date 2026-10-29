use crate::prelude::*;
use ruby_prism::CallNode;

pub const NAME: &str = "Lint/RedundantSplatExpansion";
pub static COP: RedundantSplatExpansion = RedundantSplatExpansion;
pub struct RedundantSplatExpansion;

const MSG: &str = "Replace splat expansion with comma separated values.";
const ARRAY_PARAM_MSG: &str = "Pass array contents as separate arguments.";

/// `(send (const {nil? cbase} :Array) :new ...)`
fn array_new_send(call: &CallNode) -> bool {
    if is_csend(call) || method_name(call) != "new" {
        return false;
    }
    let Some(r) = call.receiver() else { return false };
    if let Some(c) = r.as_constant_read_node() {
        return c.name().as_slice() == b"Array";
    }
    r.as_constant_path_node().is_some_and(|p| p.parent().is_none() && p.name().is_some_and(|n| n.as_slice() == b"Array"))
}

/// A splat child that is `Array.new(...)`, plain (Some(false)) or with a `{ |args| }` block (Some(true)).
fn array_new(node: &Node) -> Option<bool> {
    let call = node.as_call_node().filter(array_new_send)?;
    match call.block() {
        Some(b) if b.as_block_node().is_some() => {
            // parser numblock/itblock do not match `(block ...)`
            let params = b.as_block_node().unwrap().parameters();
            params.is_none_or(|p| p.as_block_parameters_node().is_some()).then_some(true)
        }
        _ => Some(false),
    }
}

/// What the parser gem has above the splat.
enum Parent {
    /// array literal: bracketed (or %-literal), source range, element count
    Array { bracketed: bool, range: (usize, usize), count: usize },
    Call,
    When,
    Other,
}

use helpers::parser_parent::parser_parent_index as parser_parent;

/// Grandparent type classes RuboCop checks.
#[derive(PartialEq)]
enum Grand {
    None,
    Assignment,
    Resbody,
    Other,
}

fn classify(node: &Node) -> Grand {
    match kind::of(node) {
        kind::LOCAL_VARIABLE_WRITE_NODE
        | kind::INSTANCE_VARIABLE_WRITE_NODE
        | kind::CLASS_VARIABLE_WRITE_NODE
        | kind::GLOBAL_VARIABLE_WRITE_NODE
        | kind::CONSTANT_WRITE_NODE
        | kind::CONSTANT_PATH_WRITE_NODE => Grand::Assignment,
        _ => Grand::Other,
    }
}

fn is_string_like(node: &Node) -> bool {
    matches!(
        crate::helpers::literal_node::folded_kind(node),
        kind::STRING_NODE | kind::INTERPOLATED_STRING_NODE | kind::SOURCE_FILE_NODE | kind::INTEGER_NODE | kind::FLOAT_NODE
    )
}

impl Cop for RedundantSplatExpansion {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::SPLAT_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let splat = node.as_splat_node().unwrap();
        let Some(child) = splat.expression() else { return };
        let array = child.as_array_node();
        let new = array_new(&child);
        if !(is_string_like(&child) || array.is_some() || new.is_some()) {
            return;
        }
        let anc = cx.ancestors;
        // Parser parent and grandparent of the splat.
        let n = anc.len();
        let (parent, grand) = match anc.last().map(kind::of) {
            Some(kind::RESCUE_NODE) => {
                let r = anc[n - 1].as_rescue_node().unwrap();
                let loc = splat.location();
                (Parent::Array { bracketed: false, range: (loc.start_offset(), loc.end_offset()), count: r.exceptions().iter().count() }, Grand::Resbody)
            }
            _ => {
                let Some(pi) = parser_parent(anc, n) else { return };
                let p = &anc[pi];
                let parent = if let Some(a) = p.as_array_node() {
                    let loc = p.location();
                    Parent::Array {
                        bracketed: a.opening_loc().is_some(),
                        range: (loc.start_offset(), loc.end_offset()),
                        count: a.elements().iter().count(),
                    }
                } else if p.as_call_node().is_some() {
                    Parent::Call
                } else if p.as_when_node().is_some() {
                    Parent::When
                } else {
                    Parent::Other
                };
                // A call owning a block has the parser block as its parent.
                let grand = if p.as_call_node().is_some_and(|c| c.block().is_some_and(|b| b.as_block_node().is_some())) {
                    Grand::Other
                } else {
                    parser_parent(anc, pi).map_or(Grand::None, |g| classify(&anc[g]))
                };
                (parent, grand)
            }
        };
        // redundant_splat_expansion
        if let Some(block) = new {
            // redundant_array_new_splat?: as a method argument the splat provides the arguments.
            if !matches!(parent, Parent::Array { .. }) {
                return;
            }
            if !block && matches!(parent, Parent::Array { count, .. } if count > 1) {
                return; // array_new_inside_array_literal?
            }
            if !matches!(grand, Grand::None | Grand::Assignment) {
                return;
            }
        } else if array.as_ref().is_some_and(|a| a.elements().iter().next().is_none()) {
            return;
        }
        let method_argument = matches!(parent, Parent::Call);
        let part_of_an_array = matches!(parent, Parent::Array { bracketed: true, .. });
        let percent = array.as_ref().and_then(|a| a.opening_loc()).map(|o| text(&o));
        let msg = if array.is_some() && (method_argument || part_of_an_array) {
            let allow = cx.cfg.bool("AllowPercentLiteralArrayArgument").unwrap_or(true);
            if allow && method_argument && percent.is_some_and(|p| ["%w", "%W", "%i", "%I"].iter().any(|x| p.starts_with(x))) {
                return;
            }
            ARRAY_PARAM_MSG
        } else {
            MSG
        };
        // replacement_range_and_content
        let loc = splat.location();
        let (start, end) = (loc.start_offset(), loc.end_offset());
        let child_text = node_text(&child);
        let (range, content) = if new.is_some() {
            let range = match parent {
                Parent::Array { range, .. } => range,
                _ => (start, end),
            };
            (range, child_text.to_string())
        } else if let Some(a) = &array {
            let redundant_brackets =
                matches!(parent, Parent::When) || method_argument || part_of_an_array || grand == Grand::Resbody;
            if redundant_brackets {
                let elems: Vec<&str> = a.elements().iter().map(|e| node_text(&e)).collect();
                let p = percent.unwrap_or("[");
                let content = if p.starts_with("%w") {
                    format!("'{}'", elems.join("', '"))
                } else if p.starts_with("%W") {
                    format!("\"{}\"", elems.join("\", \""))
                } else if p.starts_with("%i") {
                    format!(":{}", elems.join(", :"))
                } else if p.starts_with("%I") {
                    format!(":\"{}\"", elems.join("\", :\""))
                } else {
                    elems.join(", ")
                };
                ((start, end), content)
            } else {
                let op = splat.operator_loc();
                ((op.start_offset(), op.end_offset()), String::new())
            }
        } else {
            let wrap = matches!(parent, Parent::Array { bracketed: false, .. });
            ((start, end), if wrap { format!("[{child_text}]") } else { child_text.to_string() })
        };
        cx.add_fix(start, end, msg, |c| c.replace(range.0, range.1, &content));
    }
}
