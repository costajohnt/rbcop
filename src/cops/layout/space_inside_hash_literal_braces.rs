use crate::helpers::parser_tokens::{space_after, translated_sorted, within};
use crate::prelude::*;

pub const NAME: &str = "Layout/SpaceInsideHashLiteralBraces";
pub static COP: SpaceInsideHashLiteralBraces = SpaceInsideHashLiteralBraces;
pub struct SpaceInsideHashLiteralBraces;

#[derive(Default)]
struct State {
    tokens: Option<Vec<Token>>,
}

/// `range_of_space_to_the_right/left` step over `/[ \t]/`, not Ruby's `\s`.
fn blank(c: Option<&u8>) -> bool {
    matches!(c, Some(b' ' | b'\t'))
}

impl Cop for SpaceInsideHashLiteralBraces {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::HASH_NODE, kind::HASH_PATTERN_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let (open, close) = match node.as_hash_node() {
            Some(h) => (Some(h.opening_loc()), Some(h.closing_loc())),
            None => {
                let h = node.as_hash_pattern_node().unwrap();
                (h.opening_loc(), h.closing_loc())
            }
        };
        let loc = node.location();
        let src = cx.src;
        let tokens = {
            let st = cx.state::<State>();
            let all = st.tokens.get_or_insert_with(|| translated_sorted(src));
            within(all, loc.start_offset(), loc.end_offset()).to_vec()
        };
        // tokens.first.left_brace? (the hash's own `{`) && tokens.last.right_curly_brace?
        let hash_open = open.as_ref().filter(|o| o.as_slice() == b"{").map(|o| o.start_offset());
        let (Some(first), Some(last)) = (tokens.first(), tokens.last()) else { return };
        if hash_open != Some(first.start) || last.kind != sys::PM_TOKEN_BRACE_RIGHT {
            return;
        }
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("space").to_string();
        let no_space_empty = cx.cfg.str("EnforcedStyleForEmptyBraces") == Some("no_space");
        let left_brace = |t: &Token| t.start == first.start;
        let n = tokens.len();
        let mut pairs = vec![(tokens[0], tokens.get(1).copied())];
        if n > 2 {
            pairs.push((tokens[n - 2], Some(tokens[n - 1])));
        }
        for (t1, t2) in pairs {
            let Some(t2) = t2 else { continue };
            if src.line_of(t1.start) < src.line_of(t2.start) || t2.kind == sys::PM_TOKEN_COMMENT {
                continue;
            }
            let right_curly = t2.kind == sys::PM_TOKEN_BRACE_RIGHT;
            let is_empty = left_brace(&t1) && right_curly;
            // ponytail: token types compared by Prism kind; a block's `{` vs a hash's `{` (tLCURLY vs
            // tLBRACE) never meet here.
            let same = t1.kind == t2.kind;
            let expect_space = if same && style == "compact" {
                false
            } else if is_empty {
                !no_space_empty
            } else {
                style != "no_space"
            };
            if space_after(src, &t1) == expect_space {
                continue;
            }
            let brace = if left_brace(&t1) { t1 } else { t2 };
            let is_left = src.text.as_bytes()[brace.start] == b'{';
            let b = src.text.as_bytes();
            let (s, e) = if expect_space {
                (brace.start, brace.end)
            } else if is_left {
                let mut e = brace.end;
                while blank(b.get(e)) {
                    e += 1;
                }
                (brace.start + 1, e)
            } else {
                let mut s = brace.start;
                while s > 0 && blank(b.get(s - 1)) {
                    s -= 1;
                }
                (s, brace.end - 1)
            };
            let inside = if is_empty { "empty hash literal braces" } else { src.slice(brace.start, brace.end) };
            let problem = if expect_space { "missing" } else { "detected" };
            let range_src = src.slice(s, e);
            cx.add_fix(s, e, format!("Space inside {inside} {problem}."), |c| {
                if range_src.bytes().any(helpers::parser_tokens::is_ruby_space) {
                    c.remove(s, e);
                } else if range_src == "{" {
                    c.insert_after(s, e, " ");
                } else {
                    c.insert_before(s, e, " ");
                }
            });
        }
        if no_space_empty {
            let (s, e) = match (open, close) {
                (Some(o), Some(c)) => (o.end_offset(), c.start_offset()),
                _ => (loc.start_offset(), loc.end_offset()),
            };
            let inner = src.slice(s, e);
            if !inner.is_empty() && inner.bytes().all(helpers::parser_tokens::is_ruby_space) {
                cx.add_fix(s, e, "Space inside empty hash literal braces detected.", |c| c.remove(s, e));
            }
        }
    }
}
