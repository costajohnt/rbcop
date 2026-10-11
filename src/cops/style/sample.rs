use crate::prelude::*;

pub const NAME: &str = "Style/Sample";
pub static COP: Sample = Sample;
pub struct Sample;

/// `sample_size`: `:unknown`, nil or an Integer.
#[derive(PartialEq)]
enum Size {
    Unknown,
    Nil,
    Int(i128),
}

/// Parser `send` arguments: positional args plus a `&blk` block pass.
fn parser_args<'pr>(call: &ruby_prism::CallNode<'pr>) -> Vec<Node<'pr>> {
    let mut args = call_args(call);
    if let Some(b) = call.block().filter(|b| b.as_block_argument_node().is_some()) {
        args.push(b);
    }
    args
}

fn sample_size(args: &[Node]) -> Size {
    match args {
        [arg] => {
            if let Some(r) = arg.as_range_node() {
                range_size(&r)
            } else if helpers::literal_node::folded_kind(arg) == kind::INTEGER_NODE {
                match int_literal_value(arg) {
                    Some(0 | -1) => Size::Nil,
                    _ => Size::Unknown,
                }
            } else {
                Size::Unknown
            }
        }
        [first, second] => {
            if int_literal_value(first) != Some(0) {
                return Size::Unknown;
            }
            match int_literal_value(second) {
                Some(v) => Size::Int(v),
                None => Size::Unknown,
            }
        }
        _ => Size::Nil,
    }
}

fn range_size(r: &ruby_prism::RangeNode) -> Size {
    let val = |n: Option<Node>| -> Option<i128> {
        match n {
            None => Some(0),
            Some(n) => int_literal_value(&n),
        }
    };
    let (Some(low), Some(high)) = (val(r.left()), val(r.right())) else { return Size::Unknown };
    if low != 0 || high < 0 {
        return Size::Unknown;
    }
    Size::Int(if r.is_exclude_end() { high - low } else { high - low + 1 })
}

impl Cop for Sample {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let method = method_name(&call);
        if !matches!(method, "first" | "last" | "[]" | "at" | "slice") {
            return;
        }
        let Some(shuffle) = call.receiver().and_then(|r| r.as_call_node()) else { return };
        if method_name(&shuffle) != "shuffle" || shuffle.block().is_some_and(|b| b.as_block_node().is_some()) {
            return;
        }
        let method_args = parser_args(&call);
        let shuffle_args = parser_args(&shuffle);
        let size = match method {
            "first" | "last" => Size::Nil,
            _ => sample_size(&method_args),
        };
        if size == Size::Unknown {
            return;
        }
        let Some(sel) = shuffle.message_loc() else { return };
        let (start, end) = (sel.start_offset(), send_range(&call).1);
        let sample_arg = match method {
            "first" | "last" => method_args.first().map(|a| node_text(a).to_string()),
            "[]" | "slice" => match size {
                Size::Int(v) => Some(v.to_string()),
                _ => None,
            },
            _ => None,
        };
        let args: Vec<String> =
            [sample_arg, shuffle_args.first().map(|a| node_text(a).to_string())].into_iter().flatten().collect();
        let correction = if args.is_empty() { "sample".to_string() } else { format!("sample({})", args.join(", ")) };
        let incorrect = &cx.src.text[start..end];
        let msg = format!("Use `{correction}` instead of `{incorrect}`.");
        if shuffle_args.is_empty() {
            cx.add_fix(start, end, msg, |c| c.replace(start, end, &correction));
        } else {
            cx.add_with(start, end, msg, false);
        }
    }
}
