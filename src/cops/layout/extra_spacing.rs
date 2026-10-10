use crate::helpers::parser_tokens::T_STRING;
use crate::helpers::preceding_following_alignment::{Alignment, Data, Range, assignment_or_comparison};
use crate::prelude::*;
use ruby_prism::Visit;
use crate::source::Source;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

pub const NAME: &str = "Layout/ExtraSpacing";
pub static COP: ExtraSpacing = ExtraSpacing;
pub struct ExtraSpacing;

const MSG_UNNECESSARY: &str = "Unnecessary spacing detected.";
const MSG_UNALIGNED_ASGN: &str = "`=` is not aligned with the preceding assignment.";

#[derive(Default)]
struct State {
    data: Option<Data>,
    /// Space between key and value of pairs in multiline hashes (left to Layout/HashAlignment).
    ignored: Vec<(usize, usize)>,
}

/// Pairs whose parent hash spans several lines: `key.end...value.begin`.
struct Pairs<'a, 's> {
    src: &'a Source<'s>,
    out: Vec<(usize, usize)>,
}

impl Pairs<'_, '_> {
    fn hash(&mut self, loc: ruby_prism::Location, elements: ruby_prism::NodeList) {
        if self.src.line_of(loc.start_offset()) == self.src.line_of(loc.end_offset()) {
            return;
        }
        for e in elements.iter() {
            if let Some(p) = e.as_assoc_node() {
                self.out.push((p.key().location().end_offset(), p.value().location().start_offset()));
            }
        }
    }
}

impl<'pr> Visit<'pr> for Pairs<'_, '_> {
    fn visit_hash_node(&mut self, node: &ruby_prism::HashNode<'pr>) {
        self.hash(node.location(), node.elements());
        ruby_prism::visit_hash_node(self, node);
    }

    fn visit_keyword_hash_node(&mut self, node: &ruby_prism::KeywordHashNode<'pr>) {
        self.hash(node.location(), node.elements());
        ruby_prism::visit_keyword_hash_node(self, node);
    }

    fn visit_hash_pattern_node(&mut self, node: &ruby_prism::HashPatternNode<'pr>) {
        self.hash(node.location(), node.elements());
        ruby_prism::visit_hash_pattern_node(self, node);
    }
}

type Profile = (HashMap<usize, Vec<(usize, isize)>>, HashMap<(usize, u32), (Vec<usize>, Vec<isize>)>);

struct Run<'a, 's> {
    al: Alignment<'a, 's>,
    by_line: HashMap<usize, Vec<Token>>,
    profiles: RefCell<HashMap<Vec<usize>, Rc<Profile>>>,
    aligned_comments: HashSet<usize>,
}

impl Run<'_, '_> {
    fn col(&self, t: &Token) -> usize {
        self.al.src.column(t.start)
    }

    fn group_profile(&self, line: usize) -> Rc<Profile> {
        let group = self.al.alignment_lines(line);
        let key: Vec<usize> = group.to_vec();
        if let Some(p) = self.profiles.borrow().get(&key) {
            return p.clone();
        }
        let mut by_column: HashMap<usize, Vec<(usize, isize)>> = HashMap::new();
        let mut typed: HashMap<(usize, u32), (Vec<usize>, Vec<isize>)> = HashMap::new();
        for &l in group.iter() {
            let Some(tokens) = self.by_line.get(&l) else { continue };
            for w in tokens.windows(2) {
                let spacing = w[1].start as isize - w[0].end as isize;
                let col = self.col(&w[1]);
                by_column.entry(col).or_default().push((l, spacing));
                let e = typed.entry((col, w[1].kind)).or_default();
                e.0.push(l);
                e.1.push(spacing);
            }
        }
        let p = Rc::new((by_column, typed));
        self.profiles.borrow_mut().insert(key, p.clone());
        p
    }

    fn spacing_varies(&self, prev: &Token, token: &Token) -> bool {
        let line = self.al.line(token.start);
        let profile = self.group_profile(line);
        let (by_column, typed) = &*profile;
        let column = self.col(token);
        let empty = (Vec::new(), Vec::new());
        let (lines, spacings) = typed.get(&(column, token.kind)).unwrap_or(&empty);
        let entries = by_column.get(&column).map_or(&[][..], |v| &v[..]);
        if lines.len() > 1 {
            let distinct: HashSet<_> = spacings.iter().collect();
            distinct.len() > 1 || {
                let line_set: HashSet<usize> = lines.iter().copied().collect();
                by_column.iter().any(|(&c, entries)| {
                    c != column
                        && entries
                            .iter()
                            .filter(|(l, _)| line_set.contains(l))
                            .map(|(_, s)| s)
                            .collect::<HashSet<_>>()
                            .len()
                            > 1
                })
            }
        } else if entries.len() > 1 {
            entries.iter().map(|(_, s)| s).collect::<HashSet<_>>().len() > 1
        } else {
            self.nearest_spacing_varies(prev, token)
        }
    }

    fn nearest_spacing_varies(&self, prev: &Token, token: &Token) -> bool {
        let spacing = token.start as isize - prev.end as isize;
        let line = self.al.line(token.start);
        let n = self.al.src.lines.len();
        let up: Vec<usize> = (1..line).rev().collect();
        let down: Vec<usize> = (line + 1..=n).collect();
        [up, down].iter().any(|nos| self.nearest_spacing_differs(nos, token, spacing))
    }

    fn nearest_spacing_differs(&self, line_numbers: &[usize], token: &Token, spacing: isize) -> bool {
        let column = self.col(token);
        for &line in line_numbers {
            if self.al.d.def_lines.contains(&line) {
                break;
            }
            if self.al.d.aligned_comment_lines.contains(&line) {
                continue;
            }
            if let Some(other) = self.spacing_before_token_at(line, column) {
                return other != spacing;
            }
        }
        false
    }

    fn spacing_before_token_at(&self, line: usize, column: usize) -> Option<isize> {
        let tokens = self.by_line.get(&line)?;
        let index = tokens.iter().position(|t| self.col(t) == column)?;
        (index > 0).then(|| tokens[index].start as isize - tokens[index - 1].end as isize)
    }

    fn aligned_tok(&self, prev: &Token, token: &Token) -> bool {
        if token.kind == sys::PM_TOKEN_COMMENT {
            return self.aligned_comments.contains(&self.al.line(token.start));
        }
        self.al.aligned_with_something(Range { start: token.start, end: token.end })
            && (assignment_or_comparison(token) || self.spacing_varies(prev, token))
    }
}

/// Token#text starts with `#` (comments, `#{`, string content starting with `#`).
fn text_starts_with_hash(src: &Source, t: &Token) -> bool {
    let s = src.slice(t.start, t.end);
    if t.kind == T_STRING && (s.starts_with('\'') || s.starts_with('"')) {
        return s[1..].starts_with('#');
    }
    s.starts_with('#')
}

impl Cop for ExtraSpacing {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::PROGRAM_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // processed_source.blank? (no AST)
        if node.as_program_node().unwrap().statements().body().is_empty() {
            return;
        }
        let src = cx.src;
        let mut pairs = Pairs { src, out: Vec::new() };
        pairs.visit(node);
        let st = cx.state::<State>();
        st.ignored = pairs.out;
        st.data = Some(Data::new(src, node));
    }

    fn on_file(&self, cx: &mut Cx) {
        let st = std::mem::take(cx.state::<State>());
        let Some(data) = st.data else { return };
        let src = cx.src;
        let allow_alignment = cx.cfg.bool("AllowForAlignment").unwrap_or(true);
        let allow_comments = cx.cfg.bool("AllowBeforeTrailingComments").unwrap_or(false);
        let force = cx.cfg.bool("ForceEqualSignAlignment").unwrap_or(false);

        let mut aligned_comments = HashSet::new();
        for w in src.comments.windows(2) {
            if src.column(w[0].start) == src.column(w[1].start) {
                aligned_comments.insert(src.line_of(w[0].start));
                aligned_comments.insert(src.line_of(w[1].start));
            }
        }
        let mut by_line: HashMap<usize, Vec<Token>> = HashMap::new();
        for t in &data.tokens {
            by_line.entry(src.line_of(t.start)).or_default().push(*t);
        }
        let run = Run {
            al: Alignment { src, d: &data },
            by_line,
            profiles: RefCell::default(),
            aligned_comments,
        };
        let assignment_starts: HashSet<usize> = data.assignment_tokens.iter().map(|t| t.start).collect();
        let corrected: RefCell<HashSet<usize>> = RefCell::default();

        // `processed_source.tokens.each_cons(2)`: lexer order, so a heredoc opener and the rest of its
        // line are not neighbours.
        let lex_ordered = helpers::parser_tokens::in_lex_order(src, &data.tokens);
        for w in lex_ordered.windows(2) {
            let (t1, t2) = (&w[0], &w[1]);
            if t2.kind == sys::PM_TOKEN_NEWLINE {
                continue;
            }
            if force && assignment_starts.contains(&t2.start) {
                let r = Range { start: t2.start, end: t2.end };
                if run.al.aligned_with_equals_sign(r, true) != Some(false) {
                    continue;
                }
                let line = run.al.line(t2.start);
                let last = src.lines.len();
                let mut lines = run.al.relevant_assignment_lines(&(1..=line).rev().collect::<Vec<_>>());
                lines.extend(run.al.relevant_assignment_lines(&(line..=last).collect::<Vec<_>>()));
                let lines: HashSet<usize> = lines.into_iter().collect();
                let tokens: Vec<Token> =
                    data.assignment_tokens.iter().filter(|t| lines.contains(&src.line_of(t.start))).copied().collect();
                // align_column: the end column the `=` would have without the spaces before it.
                let align_column = |t: &Token| {
                    let col = src.column(t.start);
                    let leading: Vec<char> = src.lines[src.line_of(t.start) - 1].chars().take(col).collect();
                    let spaces = leading.iter().rev().take_while(|&&c| c == ' ').count();
                    src.column(t.end) - spaces + 1
                };
                let Some(align_to) = tokens.iter().map(align_column).max() else { continue };
                let corrected = &corrected;
                cx.add_fix(t2.start, t2.end, MSG_UNALIGNED_ASGN, |c| {
                    for t in &tokens {
                        if !corrected.borrow_mut().insert(t.start) {
                            continue;
                        }
                        let diff = align_to as isize - src.column(t.end) as isize;
                        if diff > 0 {
                            c.insert_before(t.start, t.end, &" ".repeat(diff as usize));
                        } else if diff < 0 {
                            c.remove_preceding(t.start, t.end, (-diff) as usize);
                        }
                    }
                });
                continue;
            }
            if allow_comments && text_starts_with_hash(src, t2) {
                continue;
            }
            if src.line_of(t1.start) != src.line_of(t2.start) {
                continue;
            }
            let (start, end) = (t1.end, t2.start.wrapping_sub(1));
            if t2.start == 0 || end <= start {
                continue;
            }
            if allow_alignment && run.aligned_tok(t1, t2) {
                continue;
            }
            if st.ignored.iter().any(|&(s, e)| s <= start && start < e) {
                continue;
            }
            cx.add_fix(start, end, MSG_UNNECESSARY, |c| c.remove(start, end));
        }
    }
}
