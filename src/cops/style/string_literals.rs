use crate::helpers::percent_array::double_quotes_required;
use crate::helpers::string_help;
use crate::prelude::*;

pub const NAME: &str = "Style/StringLiterals";
pub static COP: StringLiterals = StringLiterals;
pub struct StringLiterals;

const MSG_INCONSISTENT: &str = "Inconsistent quote style.";

fn message(single: bool) -> &'static str {
    if single {
        "Prefer single-quoted strings when you don't need string interpolation or special symbols."
    } else {
        "Prefer double-quoted strings unless you need single quotes to avoid extra backslashes for escaping."
    }
}

/// A parser `dstr` child: its `loc.begin` source, its source, and whether it is a `dstr`.
struct Child<'a> {
    begin: Option<&'a str>,
    src: &'a str,
    dstr: bool,
}

/// on_dstr's node (not a heredoc) with only str/dstr children, as parser children.
fn dstr_children<'pr>(node: &Node<'pr>) -> Option<(Vec<Child<'pr>>, Option<&'pr str>)> {
    if is_heredoc(node) {
        return None;
    }
    if let Some(s) = node.as_string_node() {
        if !is_parser_dstr(node) {
            return None;
        }
        // A multi-line literal: one `str` per line, without delimiters.
        let content = text(&s.content_loc());
        let children = content.split_inclusive('\n').map(|l| Child { begin: None, src: l, dstr: false }).collect();
        return Some((children, s.opening_loc().map(|o| text(&o))));
    }
    let d = node.as_interpolated_string_node()?;
    let mut children = Vec::new();
    for p in d.parts().iter() {
        let begin = match &p {
            Node::StringNode { .. } => p.as_string_node().unwrap().opening_loc(),
            Node::InterpolatedStringNode { .. } => p.as_interpolated_string_node().unwrap().opening_loc(),
            _ => return None,
        };
        let dstr = p.as_interpolated_string_node().is_some() || is_parser_dstr(&p);
        children.push(Child { begin: begin.map(|b| text(&b)), src: node_text(&p), dstr });
    }
    Some((children, d.opening_loc().map(|o| text(&o))))
}

#[derive(Default)]
struct Ignored(Vec<(usize, usize)>);

impl Cop for StringLiterals {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::STRING_NODE, kind::INTERPOLATED_STRING_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let single = cx.cfg.str("EnforcedStyle") != Some("double_quotes");
        let binary = cx.src.binary;
        let loc = node.location();
        let (start, end) = (loc.start_offset(), loc.end_offset());
        if cx.cfg.bool("ConsistentQuotesInMultiline") == Some(true)
            && let Some((children, own_begin)) = dstr_children(node)
        {
            self.on_dstr(cx, (start, end), &children, own_begin, single);
            return;
        }
        let Some(s) = string_help::delimited_str(node) else { return };
        if !string_help::legacy_gvar_interpolations(cx, node).is_empty() {
            return; // a parser `dstr`
        }
        if string_help::in_regexp(cx.ancestors) || cx.state::<Ignored>().0.iter().any(|&(a, b)| a <= start && end <= b) {
            return;
        }
        if string_help::wrong_quotes(node_text(node), single) && !string_help::inside_interpolation(cx.ancestors) {
            cx.add_fix(start, end, message(single), |c| string_help::correct(c, &s, single, binary));
        }
    }
}

impl StringLiterals {
    fn on_dstr(&self, cx: &mut Cx, range: (usize, usize), children: &[Child], own_begin: Option<&str>, single: bool) {
        let mut styles: Vec<&str> = if children.iter().all(|c| c.begin.is_none()) {
            own_begin.into_iter().collect()
        } else if children.iter().any(|c| c.begin.is_none()) {
            return; // `nil.source` raises in RuboCop: no offense, node not ignored
        } else {
            children.iter().filter_map(|c| c.begin).collect()
        };
        let mut seen = Vec::new();
        styles.retain(|s| if seen.contains(s) { false } else { seen.push(*s); true });
        let msg = if styles.len() > 1 {
            Some(MSG_INCONSISTENT)
        } else {
            let quote = styles.first().copied();
            if quote == Some("'") && !single {
                children.iter().all(|c| string_help::wrong_quotes(c.src, single)).then(|| message(single))
            } else if quote == Some("\"") && single {
                (!children.iter().any(|c| c.dstr || double_quotes_required(c.src))).then(|| message(single))
            } else {
                None
            }
        };
        if let Some(msg) = msg {
            // StringLiteralCorrector skips dstr nodes: never correctable.
            cx.add_with(range.0, range.1, msg, false);
        }
        cx.state::<Ignored>().0.push(range);
    }
}
