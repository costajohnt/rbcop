use crate::prelude::*;

pub const NAME: &str = "Style/RedundantHeredocDelimiterQuotes";
pub static COP: RedundantHeredocDelimiterQuotes = RedundantHeredocDelimiterQuotes;
pub struct RedundantHeredocDelimiterQuotes;

impl Cop for RedundantHeredocDelimiterQuotes {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        helpers::heredoc::KINDS
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(((start, end), (end_start, end_end), _)) = helpers::heredoc::parts(node) else { return };
        let Some((body_start, body_end)) = helpers::heredoc::body_range(node) else { return };
        let src = cx.src.text;
        let opening = &src[start..end];
        let ty = helpers::heredoc::heredoc_type(opening);
        // node.source.delete(heredoc_type)
        let delimiter: String = opening.chars().filter(|c| !ty.contains(*c)).collect();
        if !delimiter.starts_with(['\'', '"']) {
            return;
        }
        // heredoc_end.source.strip =~ /\W/ (ASCII \w)
        let terminator = src[end_start..end_end].trim_matches(|c: char| c.is_ascii_whitespace() || c == '\0');
        if terminator.bytes().any(|b| !(b.is_ascii_alphanumeric() || b == b'_')) {
            return;
        }
        if delimiter.starts_with('\'') {
            let body = &src.as_bytes()[body_start..body_end];
            let special = body
                .iter()
                .enumerate()
                .any(|(i, &b)| b == b'\\' || (b == b'#' && matches!(body.get(i + 1), Some(b'{' | b'@' | b'$'))));
            if special {
                return;
            }
        }
        let replacement = format!("{ty}{}", helpers::heredoc::delimiter_string(opening));
        let msg = format!("Remove the redundant heredoc delimiter quotes, use `{replacement}` instead.");
        cx.add_fix(start, end, msg, |c| c.replace(start, end, &replacement));
    }
}
