use crate::prelude::*;

pub const NAME: &str = "Rails/SquishedSQLHeredocs";
pub static COP: SquishedSQLHeredocs = SquishedSQLHeredocs;
pub struct SquishedSQLHeredocs;

static OPENING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(<<[~-]?)['"`]?([^'"`]+)['"`]?"#).unwrap());
static MARKERS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(".+?")|('.+?')|(\[.+?\])"#).unwrap());

impl Cop for SquishedSQLHeredocs {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::STRING_NODE, kind::INTERPOLATED_STRING_NODE, kind::X_STRING_NODE, kind::INTERPOLATED_X_STRING_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let (opening, parts): (Option<ruby_prism::Location>, Vec<String>) = match node {
            Node::StringNode { .. } => {
                let n = node.as_string_node().unwrap();
                (n.opening_loc(), vec![String::from_utf8_lossy(n.unescaped()).into_owned()])
            }
            Node::XStringNode { .. } => {
                let n = node.as_x_string_node().unwrap();
                (Some(n.opening_loc()), vec![String::from_utf8_lossy(n.unescaped()).into_owned()])
            }
            Node::InterpolatedStringNode { .. } => {
                let n = node.as_interpolated_string_node().unwrap();
                (n.opening_loc(), parts(n.parts().iter()))
            }
            _ => {
                let n = node.as_interpolated_x_string_node().unwrap();
                (Some(n.opening_loc()), parts(n.parts().iter()))
            }
        };
        let Some(open) = opening.filter(|o| o.as_slice().starts_with(b"<<")) else { return };
        let src = text(&open);
        if OPENING.captures(src).map(|c| c[2].to_string()).as_deref() != Some("SQL") {
            return;
        }
        // using_squish?: the parent is a `squish` send.
        let anc = cx.ancestors;
        if helpers::rails::parent_index(anc).is_some_and(|i| {
            anc[i].as_call_node().is_some_and(|c| !is_csend(&c) && method_name(&c) == "squish")
        }) {
            return;
        }
        // singleline_comments_present?: children joined by a literal `\n`.
        let sql = parts.join("\\n");
        if MARKERS.replace_all(&sql, "").contains("--") {
            return;
        }
        let (s, e) = (open.start_offset(), open.end_offset());
        let msg = format!("Use `{src}.squish` instead of `{src}`.");
        cx.add_fix(s, e, msg, |c| c.insert_after(s, e, ".squish"));
    }
}

/// Parser dstr children: str parts by value (one per line), interpolations by source.
fn parts<'pr>(nodes: impl Iterator<Item = Node<'pr>>) -> Vec<String> {
    let mut out = Vec::new();
    for p in nodes {
        match p.as_string_node() {
            Some(s) => {
                let v = String::from_utf8_lossy(s.unescaped()).into_owned();
                out.extend(v.split_inclusive('\n').map(String::from));
            }
            None => out.push(node_text(&p).to_string()),
        }
    }
    out
}
