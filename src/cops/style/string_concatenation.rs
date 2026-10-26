use crate::helpers::parser_parent::parser_parent;
use crate::helpers::ruby_string;
use crate::prelude::*;
use crate::source::Source;
use ruby_prism::Visit;
use std::cell::Cell;
use std::collections::HashSet;

pub const NAME: &str = "Style/StringConcatenation";
pub static COP: StringConcatenation = StringConcatenation;
pub struct StringConcatenation;

static ESCAPE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(\\|"|#\{|#@|#\$)"#).unwrap());
static LINE_END_PLUS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\+[ \t\n\x0b\x0c\r]*\n").unwrap());

/// Parser `str` (`__FILE__` is a `str` too).
fn is_str(node: &Node) -> bool {
    (node.as_string_node().is_some() && !is_parser_dstr(node)) || node.as_source_file_node().is_some()
}

/// A parser `send` of `+` (not csend, not wrapped in a block).
fn plus_node<'pr>(node: &Node<'pr>) -> Option<ruby_prism::CallNode<'pr>> {
    node.as_call_node().filter(|c| {
        !is_csend(c) && method_name(c) == "+" && !c.block().is_some_and(|b| b.as_block_node().is_some())
    })
}

fn collect_parts<'pr>(node: Node<'pr>, parts: &mut Vec<Node<'pr>>) {
    match plus_node(&node) {
        Some(call) => {
            if let Some(r) = call.receiver() {
                collect_parts(r, parts);
            }
            if let Some(a) = send_args(&call).into_iter().next() {
                collect_parts(a, parts);
            }
        }
        None => parts.push(node),
    }
}

/// Counts block and lambda nodes (parser `any_block`).
#[derive(Default)]
struct Blocks(usize);

impl<'pr> Visit<'pr> for Blocks {
    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode<'pr>) {
        self.0 += 1;
        ruby_prism::visit_block_node(self, node);
    }
    fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode<'pr>) {
        self.0 += 1;
        ruby_prism::visit_lambda_node(self, node);
    }
}

fn uncorrectable(src: &Source, part: &Node) -> bool {
    let l = part.location();
    if src.line_of(l.start_offset()) != src.line_of(l.end_offset()) {
        return true;
    }
    if matches!(kind::of(part), kind::STRING_NODE | kind::INTERPOLATED_STRING_NODE) && is_heredoc(part) {
        return true;
    }
    let mut v = Blocks::default();
    v.visit(part);
    // `each_descendant` excludes the node itself (a parser block wraps its call).
    let own = part.as_call_node().is_some_and(|c| c.block().is_some_and(|b| b.as_block_node().is_some()))
        || part.as_lambda_node().is_some();
    v.0 > usize::from(own)
}

fn adjust_str_literal(node: &Node) -> String {
    let src = node_text(node);
    // file_keyword?: `__FILE__` is interpolated as written.
    let Some(s) = node.as_string_node() else { return format!("#{{{src}}}") };
    let value = s.unescaped().to_vec();
    // single_quoted? / double_quoted?: only a literal with its own opening quote.
    let quoted = s.opening_loc().is_some();
    if quoted && src.starts_with('\'') {
        ESCAPE.replace_all(&String::from_utf8_lossy(&value), r"\$1").into_owned()
    } else if quoted && src.starts_with('"') {
        src[1..src.len() - 1].to_string()
    } else {
        let s = ruby_string::inspect(&value);
        s[1..s.len() - 1].to_string()
    }
}

fn adjust_str(node: &Node) -> String {
    let children: Vec<Node> = match kind::of(node) {
        kind::STRING_NODE | kind::SOURCE_FILE_NODE => return adjust_str_literal(node),
        kind::INTERPOLATED_STRING_NODE => node.as_interpolated_string_node().unwrap().parts().iter().collect(),
        kind::EMBEDDED_STATEMENTS_NODE => {
            let e = node.as_embedded_statements_node().unwrap();
            e.statements().map(|s| s.body().iter().collect()).unwrap_or_default()
        }
        kind::EMBEDDED_VARIABLE_NODE => vec![node.as_embedded_variable_node().unwrap().variable()],
        kind::PARENTHESES_NODE => match node.as_parentheses_node().unwrap().body() {
            None => Vec::new(),
            Some(b) => match b.as_statements_node() {
                Some(s) => s.body().iter().collect(),
                None => vec![b],
            },
        },
        _ => return format!("#{{{}}}", node_text(node)),
    };
    children.iter().map(adjust_str).collect()
}

fn replacement(parts: &[Node]) -> String {
    let inner: String = parts
        .iter()
        .map(adjust_str)
        .map(|s| if s == "\"" { "\\\"".to_string() } else { s })
        .collect();
    format!("\"{inner}\"")
}

/// Index in `ancestors` of the parser parent of the node whose ancestors are `ancestors`.
fn parent_index(ancestors: &[Node]) -> Option<usize> {
    let p = parser_parent(ancestors)?;
    ancestors.iter().rposition(|n| std::ptr::eq(n, p))
}

impl Cop for StringConcatenation {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "+" {
            return;
        }
        let args = send_args(&call);
        let Some(recv) = call.receiver() else { return };
        if args.len() != 1 || !(is_str(&recv) || is_str(&args[0])) {
            return;
        }
        let (s, e) = send_range(&call);
        if is_str(&recv)
            && is_str(&args[0])
            && cx.src.line_of(s) != cx.src.line_of(e)
            && LINE_END_PLUS.is_match(&cx.src.text[s..e])
        {
            return;
        }
        // find_topmost_plus_node
        let mut top: Option<usize> = None;
        let mut anc = cx.ancestors;
        while let Some(i) = parent_index(anc) {
            if plus_node(&anc[i]).is_none() {
                break;
            }
            top = Some(i);
            anc = &cx.ancestors[..i];
        }
        let top_node = top.map_or(node, |i| &cx.ancestors[i]);
        let mut parts = Vec::new();
        collect_parts(top_node.as_call_node().unwrap().as_node(), &mut parts);
        if cx.cfg.str("Mode") == Some("conservative") && !parts.first().is_some_and(is_str) {
            return;
        }
        let range = send_range(&top_node.as_call_node().unwrap());
        // corrected_ancestor?: a parser `send` ancestor of the topmost node already corrected.
        let above = &cx.ancestors[..top.unwrap_or(cx.ancestors.len())];
        let corrected = cx.state::<HashSet<(usize, usize)>>();
        let corrected_ancestor = (0..above.len()).any(|i| {
            let owns_block = above.get(i + 1).is_some_and(|n| n.as_block_node().is_some());
            above[i].as_call_node().is_some_and(|c| !is_csend(&c) && !owns_block && corrected.contains(&send_range(&c)))
        });
        let edit = !corrected_ancestor && !parts.iter().any(|p| uncorrectable(cx.src, p));
        let fixed = if edit { replacement(&parts) } else { String::new() };
        let done = Cell::new(false);
        cx.add_fix(range.0, range.1, "Prefer string interpolation to string concatenation.", |c| {
            if edit {
                c.replace(range.0, range.1, &fixed);
                done.set(true);
            }
        });
        if done.get() {
            cx.state::<HashSet<(usize, usize)>>().insert(range);
        }
    }
}
