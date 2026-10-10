use crate::prelude::*;

pub const NAME: &str = "Style/DocumentDynamicEvalDefinition";
pub static COP: DocumentDynamicEvalDefinition = DocumentDynamicEvalDefinition;
pub struct DocumentDynamicEvalDefinition;

const WS: &[u8] = b" \t\n\x0b\x0c\r";
const RE_WS: &str = r"[ \t\n\x0B\x0C\r]";

/// `str.match?(/\s*#(?!{).*/)`: some `#` not followed by `{`.
fn has_comment(s: &str) -> bool {
    let b = s.as_bytes();
    b.iter().enumerate().any(|(i, &c)| c == b'#' && b.get(i + 1) != Some(&b'{'))
}

/// `str.gsub(/\s*#(?!{).*/, '')`.
fn strip_comments(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    let mut copied = 0;
    while i < b.len() {
        let mut j = i;
        while j < b.len() && WS.contains(&b[j]) {
            j += 1;
        }
        if j < b.len() && b[j] == b'#' && b.get(j + 1) != Some(&b'{') {
            let end = b[j..].iter().position(|&c| c == b'\n').map_or(b.len(), |p| j + p);
            out.push_str(&s[copied..i]);
            copied = end;
            i = end.max(i + 1);
            continue;
        }
        i += 1;
    }
    out.push_str(&s[copied..]);
    out
}

/// `line.gsub!(/^\s*#(?!{)/, '')` on a single line: None when nothing matched.
fn strip_block_comment(line: &str) -> Option<&str> {
    let b = line.as_bytes();
    let j = b.iter().position(|c| !WS.contains(c))?;
    (b[j] == b'#' && b.get(j + 1) != Some(&b'{')).then(|| &line[j + 1..])
}

/// `merge_adjacent_comments` into an ordered (line, text) list.
fn merge(blocks: &mut Vec<(usize, String)>, line: &str, index: usize) {
    let Some(text) = strip_block_comment(line) else { return };
    match blocks.last_mut() {
        Some((l, t)) if *l + 1 == index => {
            t.push('\n');
            t.push_str(text);
            *l = index;
        }
        _ => blocks.push((index, text.to_string())),
    }
}

fn blank(s: &str) -> bool {
    s.chars().all(|c| c.is_whitespace() || c == '\0')
}

/// `source_to_regexp`, as a Rust pattern ("" for nil).
fn source_to_regexp(source: &str) -> String {
    if blank(source) {
        return format!("(?:{RE_WS}+)");
    }
    let source = strip_comments(source);
    if blank(&source) {
        return String::new();
    }
    let segments: Vec<String> = helpers::ruby_string::strip(&source).split("\\#").map(regex::escape).collect();
    format!(r"(?:{RE_WS}*{})", segments.join(r"\\?\#"))
}

impl Cop for DocumentDynamicEvalDefinition {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call)
            || !matches!(method_name(&call), "eval" | "class_eval" | "module_eval" | "instance_eval")
        {
            return;
        }
        let Some(arg) = call.arguments().and_then(|a| a.arguments().iter().next()) else { return };
        let Some(dstr) = arg.as_interpolated_string_node() else { return };
        let embedded: Vec<(usize, usize, bool)> = dstr
            .parts()
            .iter()
            .filter_map(|p| {
                let l = p.location();
                if p.as_embedded_statements_node().is_some() {
                    Some((l.start_offset(), l.end_offset(), true))
                } else {
                    p.as_embedded_variable_node().map(|_| (l.start_offset(), l.end_offset(), false))
                }
            })
            .collect();
        if !embedded.iter().any(|e| e.2) {
            return;
        }
        let src = cx.src;
        let inline_docs = embedded
            .iter()
            .filter(|e| e.2)
            .all(|e| has_comment(src.lines[src.line_of(e.0) - 1]));
        if inline_docs || (is_heredoc(&arg) && comment_block_docs(src, &call, &dstr, &embedded)) {
            return;
        }
        let Some(sel) = call.message_loc() else { return };
        cx.add(sel.start_offset(), sel.end_offset(), "Add a comment block showing its appearance if interpolated.");
    }
}

fn comment_block_docs(
    src: &crate::source::Source,
    call: &ruby_prism::CallNode,
    dstr: &ruby_prism::InterpolatedStringNode,
    embedded: &[(usize, usize, bool)],
) -> bool {
    let Some(closing) = dstr.closing_loc() else { return false };
    let Some(first) = dstr.parts().iter().next() else { return false };
    let body_first = src.line_of(first.location().start_offset());
    let terminator = src.line_of(closing.start_offset());
    let mut heredoc_blocks = Vec::new();
    for line in body_first..=terminator {
        merge(&mut heredoc_blocks, src.lines[line - 1], line);
    }
    let (ss, se) = send_range(call);
    let (l1, l2) = (src.line_of(ss), src.line_of(se));
    let mut preceding = Vec::new();
    for c in &src.comments {
        let line = src.line_of(c.start);
        if (l1..=l2).contains(&line) {
            merge(&mut preceding, src.slice(c.start, c.end), line);
        }
    }
    let comments: Vec<String> = heredoc_blocks.into_iter().chain(preceding).map(|(_, t)| t).collect();
    if comments.is_empty() {
        return false;
    }
    // comment_regexp: the heredoc's parser children, interpolations as `.+`.
    let body_start = src.line_start(body_first);
    let body_end = src.line_start(terminator);
    let mut pattern = String::new();
    let mut pos = body_start;
    let text_parts = |from: usize, to: usize, pattern: &mut String| {
        let mut s = from;
        while s < to {
            let e = src.text[s..to].find('\n').map_or(to, |p| s + p + 1);
            pattern.push_str(&source_to_regexp(&src.text[s..e]));
            s = e;
        }
    };
    for &(s, e, stmts) in embedded {
        text_parts(pos, s, &mut pattern);
        if stmts {
            pattern.push_str("(?:.+)");
        } else {
            pattern.push_str(&source_to_regexp(&src.text[s + 1..e]));
        }
        pos = e;
    }
    text_parts(pos, body_end, &mut pattern);
    let Ok(re) = regex::RegexBuilder::new(&pattern).size_limit(1 << 26).build() else { return false };
    comments.iter().any(|c| re.is_match(c)) || re.is_match(&comments.concat())
}
