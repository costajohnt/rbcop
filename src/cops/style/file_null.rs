use crate::prelude::*;
use ruby_prism::{RegularExpressionNode, StringNode, Visit, XStringNode};

pub const NAME: &str = "Style/FileNull";
pub static COP: FileNull = FileNull;
pub struct FileNull;

static REGEXP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\A(?:/dev/null|NUL:?)\z").unwrap());

fn valid(value: &[u8]) -> bool {
    !value.is_empty() && std::str::from_utf8(value).is_ok()
}

fn is_dev_null(value: &[u8]) -> bool {
    valid(value) && value.eq_ignore_ascii_case(b"/dev/null")
}

/// `ast.each_descendant(:str).any? { valid_string? && downcase == '/dev/null' }`.
#[derive(Default)]
struct Finder {
    found: bool,
}

impl<'pr> Visit<'pr> for Finder {
    fn visit_string_node(&mut self, node: &StringNode<'pr>) {
        // A multi-line literal is a parser dstr with one str per line.
        let v = node.unescaped();
        if is_parser_dstr(&node.as_node()) {
            self.found |= v.split_inclusive(|&b| b == b'\n').any(is_dev_null);
        } else {
            self.found |= is_dev_null(v);
        }
    }
    fn visit_x_string_node(&mut self, node: &XStringNode<'pr>) {
        self.found |= is_dev_null(node.unescaped());
    }
    fn visit_regular_expression_node(&mut self, node: &RegularExpressionNode<'pr>) {
        self.found |= is_dev_null(node.content_loc().as_slice());
    }
}

impl Cop for FileNull {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::STRING_NODE, kind::X_STRING_NODE, kind::REGULAR_EXPRESSION_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // The parser `str` node: (start, end, value).
        let (start, end, value): (usize, usize, Vec<u8>) = match node {
            Node::StringNode { .. } => {
                if is_parser_dstr(node) {
                    return;
                }
                let s = node.as_string_node().unwrap();
                let l = node.location();
                // A part of an interpolated regexp keeps its source text.
                let in_regexp = cx.parent().is_some_and(|p| p.as_interpolated_regular_expression_node().is_some());
                let v = if in_regexp { s.content_loc().as_slice().to_vec() } else { s.unescaped().to_vec() };
                (l.start_offset(), l.end_offset(), v)
            }
            Node::XStringNode { .. } => {
                let x = node.as_x_string_node().unwrap();
                let c = x.content_loc();
                (c.start_offset(), c.end_offset(), x.unescaped().to_vec())
            }
            _ => {
                let c = node.as_regular_expression_node().unwrap().content_loc();
                (c.start_offset(), c.end_offset(), c.as_slice().to_vec())
            }
        };
        if !valid(&value) {
            return;
        }
        if kind::of(node) == kind::STRING_NODE
            && helpers::parser_parent::parser_parent(cx.ancestors).is_some_and(|p| {
                matches!(
                    kind::of(p),
                    kind::ARRAY_NODE | kind::ASSOC_NODE | kind::INTERPOLATED_STRING_NODE
                )
            })
        {
            return;
        }
        if value.eq_ignore_ascii_case(b"nul") {
            let anc = cx.ancestors;
            let program = &anc[0];
            let found = *cx.state::<Option<bool>>().get_or_insert_with(|| {
                let mut f = Finder::default();
                f.visit(program);
                f.found
            });
            if !found {
                return;
            }
        }
        let value = std::str::from_utf8(&value).unwrap();
        if !REGEXP.is_match(value) {
            return;
        }
        cx.add_fix(start, end, format!("Use `File::NULL` instead of `{value}`."), |c| {
            c.replace(start, end, "File::NULL")
        });
    }
}
