use crate::prelude::*;

pub const NAME: &str = "Layout/ArgumentAlignment";
pub static COP: ArgumentAlignment = ArgumentAlignment;
pub struct ArgumentAlignment;

const ALIGN_PARAMS_MSG: &str = "Align the arguments of a method call if they span more than one line.";
const FIXED_INDENT_MSG: &str =
    "Use one level of indentation for arguments following the first line of a multi-line method call.";

/// `hash.pairs`: the `key: value` / `key => value` elements (not `**splat`).
fn pairs(hash: &Node) -> Vec<(usize, usize)> {
    let elements = match (hash.as_keyword_hash_node(), hash.as_hash_node()) {
        (Some(h), _) => h.elements(),
        (_, Some(h)) => h.elements(),
        _ => return Vec::new(),
    };
    elements.iter().filter(|e| kind::of(e) == kind::ASSOC_NODE).map(|e| range(&e)).collect()
}

fn range(node: &Node) -> (usize, usize) {
    let loc = node.location();
    (loc.start_offset(), loc.end_offset())
}

fn is_hash(node: &Node) -> bool {
    matches!(kind::of(node), kind::HASH_NODE | kind::KEYWORD_HASH_NODE)
}

/// `enforce_hash_argument_with_separator?`
fn hash_separator_style(cx: &Cx) -> bool {
    let cfg = cx.config.for_cop("Layout/HashAlignment");
    cfg.truthy("Enabled")
        && ["EnforcedColonStyle", "EnforcedHashRocketStyle"].iter().any(|k| match cfg.get(k) {
            Some(Value::Str(s)) => s.contains("separator"),
            Some(Value::Seq(items)) => items.iter().any(|v| v.as_string().as_deref() == Some("separator")),
            _ => false,
        })
}

impl ArgumentAlignment {
    /// `on_send` body: `args` are the parser send's arguments (block pass included), `selector`
    /// the offset whose line is the method's line (selector, or `(` of `foo.()`).
    fn check(&self, cx: &mut Cx, args: Vec<Node>, selector: usize) {
        let multiple = args.len() >= 2 || args.first().is_some_and(|a| is_hash(a) && pairs(a).len() >= 2);
        if !multiple {
            return;
        }
        let style = cx.cfg.str("EnforcedStyle");
        let fixed = style == Some("with_fixed_indentation");
        let separator = hash_separator_style(cx);
        if style == Some("with_first_argument") && separator {
            return;
        }
        let items: Vec<(usize, usize)> = if fixed {
            let (last, init) = args.split_last().unwrap();
            let mut items: Vec<_> = init.iter().map(range).collect();
            if kind::of(last) == kind::KEYWORD_HASH_NODE {
                if !separator {
                    items.extend(pairs(last));
                }
            } else {
                items.push(range(last));
            }
            items
        } else if kind::of(&args[0]) == kind::KEYWORD_HASH_NODE {
            pairs(&args[0])
        } else {
            args.iter().map(range).collect()
        };
        let src = cx.src;
        let base = if fixed {
            let line = src.lines[src.line_of(selector) - 1];
            let indent = line.chars().take_while(|c| c.is_ascii_whitespace()).count() as i64;
            indent + helpers::alignment::configured_indentation_width(cx)
        } else {
            helpers::alignment::display_column(src, args[0].location().start_offset()) as i64
        };
        let msg = if fixed { FIXED_INDENT_MSG } else { ALIGN_PARAMS_MSG };
        let mut prev_line = 0;
        for &(start, end) in &items {
            let line = src.line_of(start);
            if line > prev_line && src.begins_its_line(start) {
                let delta = base - helpers::alignment::display_column(src, start) as i64;
                if delta != 0 {
                    let reported = cx.state::<Vec<(usize, usize)>>();
                    let nested = reported.iter().any(|&(s, e)| start >= s && end <= e);
                    reported.push((start, end));
                    let correctable = !nested && helpers::alignment::corrects(cx, start, end, delta);
                    cx.add_with(start, end, msg, correctable);
                }
            }
            prev_line = line;
        }
    }
}

impl Cop for ArgumentAlignment {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[
            kind::CALL_NODE,
            kind::INDEX_OPERATOR_WRITE_NODE,
            kind::INDEX_OR_WRITE_NODE,
            kind::INDEX_AND_WRITE_NODE,
            kind::INDEX_TARGET_NODE,
        ]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // `a[1, 2] += x` etc.: parser wraps a `send :[]` that on_send sees.
        fn index<'pr>(args: Option<ruby_prism::ArgumentsNode<'pr>>, block: Option<Node<'pr>>) -> Vec<Node<'pr>> {
            let mut v: Vec<Node> = args.map(|a| a.arguments().iter().collect()).unwrap_or_default();
            v.extend(block);
            v
        }
        let (args, selector) = match kind::of(node) {
            kind::CALL_NODE => {
                let call = node.as_call_node().unwrap();
                if method_name(&call) == "[]=" {
                    return;
                }
                let Some(sel) = call.message_loc().or(call.opening_loc()) else { return };
                (send_args(&call), sel.start_offset())
            }
            kind::INDEX_OPERATOR_WRITE_NODE => {
                let n = node.as_index_operator_write_node().unwrap();
                (index(n.arguments(), n.block().map(|b| b.as_node())), n.opening_loc().start_offset())
            }
            kind::INDEX_OR_WRITE_NODE => {
                let n = node.as_index_or_write_node().unwrap();
                (index(n.arguments(), n.block().map(|b| b.as_node())), n.opening_loc().start_offset())
            }
            kind::INDEX_AND_WRITE_NODE => {
                let n = node.as_index_and_write_node().unwrap();
                (index(n.arguments(), n.block().map(|b| b.as_node())), n.opening_loc().start_offset())
            }
            _ => {
                let n = node.as_index_target_node().unwrap();
                (index(n.arguments(), n.block().map(|b| b.as_node())), n.opening_loc().start_offset())
            }
        };
        self.check(cx, args, selector);
    }
}
