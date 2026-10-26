use crate::prelude::*;
use helpers::comment_association::leading;
use helpers::visibility_help::node_visibility;

pub const NAME: &str = "Style/ClassMethodsDefinitions";
pub static COP: ClassMethodsDefinitions = ClassMethodsDefinitions;
pub struct ClassMethodsDefinitions;

impl Cop for ClassMethodsDefinitions {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::SINGLETON_CLASS_NODE, kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let def_self = cx.cfg.str("EnforcedStyle") != Some("self_class");
        if let Some(def) = node.as_def_node() {
            if def_self || def.receiver().is_none_or(|r| r.as_self_node().is_none()) {
                return;
            }
            let loc = node.location();
            cx.add_with(loc.start_offset(), loc.end_offset(), "Use `class << self` to define a class method.", false);
            return;
        }
        if !def_self {
            return;
        }
        let sclass = node.as_singleton_class_node().unwrap();
        if sclass.expression().as_self_node().is_none() {
            return;
        }
        // def_nodes: the body itself, or the `def` children of a multi-statement body.
        let stmts: Vec<Node> = match sclass.body().and_then(|b| b.as_statements_node()) {
            Some(s) => s.body().iter().collect(),
            None => return,
        };
        let multi = stmts.len() > 1;
        let defs: Vec<usize> =
            (0..stmts.len()).filter(|&i| stmts[i].as_def_node().is_some_and(|d| d.receiver().is_none())).collect();
        if defs.is_empty() {
            return;
        }
        let sibs: &[Node] = if multi { &stmts } else { &[] };
        let visibility = |i: usize| node_visibility(&stmts[i], Some(node), sibs, if multi { i } else { 0 });
        if !defs.iter().all(|&i| visibility(i) == "public") {
            return;
        }
        let src = cx.src;
        let text = src.text;
        // no_code_after?: only a comment (after any `;`) may follow each def on its last line.
        let code_after = |i: usize| {
            let end = stmts[i].location().end_offset();
            let line_end = text[end..].find('\n').map_or(text.len(), |p| end + p);
            let trailing = text[end..line_end].trim_matches(|c: char| c.is_ascii_whitespace() || c == '\0');
            let trailing = trailing.trim_start_matches(';').trim_start_matches(|c: char| c.is_ascii_whitespace());
            !trailing.is_empty() && !trailing.starts_with('#')
        };
        if defs.iter().any(|&i| code_after(i)) {
            return;
        }
        let sloc = node.location();
        let (s_start, s_end) = (sloc.start_offset(), sloc.end_offset());
        // inside_corrected_sclass?: an enclosing `class << self` was corrected (ignore_node).
        let outer: Vec<(usize, usize)> = cx
            .ancestors
            .iter()
            .filter(|a| a.as_singleton_class_node().is_some())
            .map(|a| (a.location().start_offset(), a.location().end_offset()))
            .collect();
        let corrected = cx.state::<Vec<(usize, usize)>>();
        let inside_corrected = outer.iter().any(|r| corrected.contains(r));
        if inside_corrected {
            cx.add_fix(s_start, s_end, "Do not define public methods within class << self.", |_| {});
            return;
        }
        if cx.block_runs(s_start, s_end) {
            cx.state::<Vec<(usize, usize)>>().push((s_start, s_end));
        }
        // autocorrect_sclass
        let s_col = src.column(s_start);
        let mut removals = Vec::new();
        let mut rewritten: Vec<String> = Vec::new();
        for &i in &defs {
            let d = stmts[i].as_def_node().unwrap();
            let dloc = stmts[i].location();
            let (d_start, d_end) = (dloc.start_offset(), dloc.end_offset());
            // source_range_with_comment
            let (after, after_line) = if i == 0 {
                (0, Some(src.line_of(s_start)))
            } else {
                let prev = stmts[i - 1].location().end_offset();
                (prev, Some(src.line_of(prev)))
            };
            let comments = leading(src, d_start, after, after_line);
            let first_line = comments
                .first()
                .map(|c| src.line_of(c.start))
                .filter(|&l| l < src.line_of(d_start))
                .unwrap_or(src.line_of(d_start));
            let Some(begin) = src.line_start(first_line).checked_sub(1) else { return };
            let end_line = src.line_of(d_end);
            let end = text[src.line_start(end_line)..].find('\n').map_or(text.len(), |p| src.line_start(end_line) + p);
            // prefix_def_with_self
            let name = std::str::from_utf8(d.name().as_slice()).unwrap_or("");
            let mut source = String::new();
            source.push_str(&text[begin..d.def_keyword_loc().start_offset()]);
            source.push_str(&format!("def self.{name}"));
            source.push_str(&text[d.name_loc().end_offset()..end]);
            // gsub(/^ {indentation_diff}/, '')
            let diff = src.column(d_start) as i64 - s_col as i64;
            let source = if diff > 0 { strip_indent(&source, diff as usize) } else { source };
            removals.push((begin, end));
            rewritten.push(source.strip_suffix('\n').unwrap_or(&source).to_string());
        }
        let only_methods = stmts.iter().all(|s| s.as_def_node().is_some_and(|d| d.receiver().is_none()));
        if only_methods && let Some(first) = rewritten.first_mut() {
            *first = helpers::ruby_string::strip(first).to_string();
        }
        let joined = rewritten.join("\n");
        cx.add_fix(s_start, s_end, "Do not define public methods within class << self.", |c| {
            for &(b, e) in &removals {
                c.remove(b, e);
            }
            if only_methods {
                c.remove(s_start, s_end);
            } else {
                c.insert_after(s_start, s_end, "\n");
            }
            c.insert_after(s_start, s_end, &joined);
        });
    }
}

/// `gsub(/^ {n}/, '')`: drop exactly `n` leading spaces from each line that has them.
fn strip_indent(s: &str, n: usize) -> String {
    let pad = " ".repeat(n);
    s.split_inclusive('\n').map(|l| l.strip_prefix(pad.as_str()).unwrap_or(l)).collect()
}
