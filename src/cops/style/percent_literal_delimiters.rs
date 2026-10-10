use crate::prelude::*;
use helpers::percent_array::delimiters;
use ruby_prism::Location;

pub const NAME: &str = "Style/PercentLiteralDelimiters";
pub static COP: PercentLiteralDelimiters = PercentLiteralDelimiters;
pub struct PercentLiteralDelimiters;

/// String sources of the parser node's `str`/`sym` children (`node.children.filter_map { string_source }`).
fn children<'pr>(node: &Node<'pr>) -> Vec<Vec<u8>> {
    let src_of = |l: Location| l.as_slice().to_vec();
    let parts = |parts: ruby_prism::NodeList<'pr>| -> Vec<Vec<u8>> {
        parts.iter().filter(|p| p.as_string_node().is_some()).map(|p| src_of(p.location())).collect()
    };
    match kind::of(node) {
        kind::ARRAY_NODE => node
            .as_array_node()
            .unwrap()
            .elements()
            .iter()
            .filter(|e| e.as_string_node().is_some() || e.as_symbol_node().is_some())
            .map(|e| src_of(e.location()))
            .collect(),
        // A parser `dstr` (multi-line literal) has one `str` child per line: their sources.
        kind::STRING_NODE if is_parser_dstr(node) => vec![src_of(node.as_string_node().unwrap().content_loc())],
        kind::STRING_NODE => vec![node.as_string_node().unwrap().unescaped().to_vec()],
        kind::SYMBOL_NODE => vec![node.as_symbol_node().unwrap().unescaped().to_vec()],
        kind::INTERPOLATED_STRING_NODE => parts(node.as_interpolated_string_node().unwrap().parts()),
        kind::INTERPOLATED_X_STRING_NODE => parts(node.as_interpolated_x_string_node().unwrap().parts()),
        kind::INTERPOLATED_REGULAR_EXPRESSION_NODE => {
            parts(node.as_interpolated_regular_expression_node().unwrap().parts())
        }
        kind::X_STRING_NODE => vec![src_of(node.as_x_string_node().unwrap().content_loc())],
        kind::REGULAR_EXPRESSION_NODE => vec![src_of(node.as_regular_expression_node().unwrap().content_loc())],
        _ => vec![],
    }
}

fn contains(children: &[Vec<u8>], delims: &[&str]) -> bool {
    children.iter().any(|c| delims.iter().any(|d| c.windows(d.len()).any(|w| w == d.as_bytes())))
}

impl Cop for PercentLiteralDelimiters {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[
            kind::ARRAY_NODE,
            kind::REGULAR_EXPRESSION_NODE,
            kind::INTERPOLATED_REGULAR_EXPRESSION_NODE,
            kind::STRING_NODE,
            kind::INTERPOLATED_STRING_NODE,
            kind::SYMBOL_NODE,
            kind::X_STRING_NODE,
            kind::INTERPOLATED_X_STRING_NODE,
        ]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let (open, close) = match kind::of(node) {
            kind::ARRAY_NODE => {
                let n = node.as_array_node().unwrap();
                (n.opening_loc(), n.closing_loc())
            }
            kind::REGULAR_EXPRESSION_NODE => {
                let n = node.as_regular_expression_node().unwrap();
                (Some(n.opening_loc()), Some(n.closing_loc()))
            }
            kind::INTERPOLATED_REGULAR_EXPRESSION_NODE => {
                let n = node.as_interpolated_regular_expression_node().unwrap();
                (Some(n.opening_loc()), Some(n.closing_loc()))
            }
            kind::STRING_NODE => {
                let n = node.as_string_node().unwrap();
                (n.opening_loc(), n.closing_loc())
            }
            kind::INTERPOLATED_STRING_NODE => {
                let n = node.as_interpolated_string_node().unwrap();
                (n.opening_loc(), n.closing_loc())
            }
            kind::SYMBOL_NODE => {
                let n = node.as_symbol_node().unwrap();
                (n.opening_loc(), n.closing_loc())
            }
            kind::X_STRING_NODE => {
                let n = node.as_x_string_node().unwrap();
                (Some(n.opening_loc()), Some(n.closing_loc()))
            }
            _ => {
                let n = node.as_interpolated_x_string_node().unwrap();
                (Some(n.opening_loc()), Some(n.closing_loc()))
            }
        };
        let (Some(open), Some(close)) = (open, close) else { return };
        let Ok(begin) = std::str::from_utf8(open.as_slice()) else { return };
        if !begin.starts_with('%') {
            return;
        }
        let last = begin.chars().next_back().unwrap();
        let ty = &begin[..begin.len() - last.len_utf8()];
        let types: &[&str] = match kind::of(node) {
            kind::ARRAY_NODE => &["%w", "%W", "%i", "%I"],
            kind::REGULAR_EXPRESSION_NODE | kind::INTERPOLATED_REGULAR_EXPRESSION_NODE => &["%r"],
            kind::STRING_NODE | kind::INTERPOLATED_STRING_NODE => &["%", "%Q", "%q"],
            kind::SYMBOL_NODE => &["%s"],
            _ => &["%x"],
        };
        if !types.contains(&ty) {
            return;
        }
        let (d0, d1) = delimiters(cx, ty);
        if d0.is_empty() {
            return; // RuboCop raises on a missing preferred delimiter.
        }
        if d0.starts_with(last) {
            return;
        }
        let kids = children(node);
        if contains(&kids, &[&d0, &d1]) {
            return;
        }
        if matches!(ty, "%w" | "%i") {
            let used: Vec<String> = match last {
                '(' => vec!["(".into(), ")".into()],
                '[' => vec!["[".into(), "]".into()],
                '{' => vec!["{".into(), "}".into()],
                '<' => vec!["<".into(), ">".into()],
                c => vec![c.to_string()],
            };
            if contains(&kids, &used.iter().map(String::as_str).collect::<Vec<_>>()) {
                return;
            }
        }
        let msg = format!("`{ty}`-literals should be delimited by `{d0}` and `{d1}`.");
        let (os, oe) = (open.start_offset(), open.end_offset());
        // Prism's regexp closing includes the options; parser `loc.end` is the delimiter only.
        let cs = close.start_offset();
        let ce = cs + cx.src.text[cs..].chars().next().map_or(0, char::len_utf8);
        let loc = node.location();
        let open_text = format!("{ty}{d0}");
        cx.add_fix(loc.start_offset(), loc.end_offset(), msg, |c| {
            c.replace(os, oe, &open_text);
            c.replace(cs, ce, &d1);
        });
    }
}
