use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Lint/ImplicitStringConcatenation";
pub static COP: ImplicitStringConcatenation = ImplicitStringConcatenation;
pub struct ImplicitStringConcatenation;

/// `str_content`: the values of all `str` nodes in the subtree, joined.
#[derive(Default)]
struct StrContent(Vec<u8>);

impl<'pr> Visit<'pr> for StrContent {
    fn visit_string_node(&mut self, node: &ruby_prism::StringNode<'pr>) {
        self.0.extend_from_slice(node.unescaped());
    }
}

fn display_str(n: &Node) -> String {
    let src = node_text(n);
    if !src.contains('\n') {
        return src.to_string();
    }
    let mut v = StrContent::default();
    v.visit(n);
    helpers::ruby_string::inspect(&v.0)
}

fn is_string(n: &Node) -> bool {
    n.as_string_node().is_some() || n.as_interpolated_string_node().is_some()
}

impl Cop for ImplicitStringConcatenation {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::INTERPOLATED_STRING_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let parts: Vec<Node> = node.as_interpolated_string_node().unwrap().parts().iter().collect();
        // node.parent: nearest ancestor that exists in parser's AST.
        let parent = cx.ancestors.iter().rev().find(|a| frame(a) != Frame::Transparent);
        let suffix = match parent {
            Some(p) if p.as_array_node().is_some() => {
                " Or, if they were intended to be separate array elements, separate them with a comma."
            }
            Some(p) if p.as_call_node().is_some_and(|c| !is_csend(&c)) => {
                " Or, if they were intended to be separate method arguments, separate them with a comma."
            }
            _ => "",
        };
        for w in parts.windows(2) {
            let (lhs, rhs) = (&w[0], &w[1]);
            if !is_string(lhs) || !is_string(rhs) {
                continue;
            }
            let (l, r) = (lhs.location(), rhs.location());
            if cx.src.line_of(l.end_offset()) != cx.src.line_of(r.start_offset()) {
                continue;
            }
            let src = node_text(lhs);
            let quote = match src.as_bytes().first() {
                Some(b'\'') => b'\'',
                Some(b'"') => b'"',
                _ => continue,
            };
            if src.as_bytes().last() != Some(&quote) {
                continue;
            }
            let msg = format!(
                "Combine {} and {} into a single string literal, rather than using implicit string concatenation.{suffix}",
                display_str(lhs),
                display_str(rhs)
            );
            let empty = |n: &Node| n.as_string_node().is_some_and(|s| s.unescaped().is_empty());
            let (lhs_empty, rhs_empty) = (empty(lhs), empty(rhs));
            let (ls, le, rs, re) = (l.start_offset(), l.end_offset(), r.start_offset(), r.end_offset());
            cx.add_fix(ls, re, msg, |c| {
                if lhs_empty {
                    c.remove(ls, le);
                } else if rhs_empty {
                    c.remove(rs, re);
                } else {
                    c.replace(le, rs, " + ");
                }
            });
        }
    }
}
