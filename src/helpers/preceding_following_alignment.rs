//! RuboCop's PrecedingFollowingAlignment mixin (ExtraSpacing, SpaceAroundOperators,
//! SpaceBeforeFirstArg). Build a `Data` once per file from the program node, keep it in the cop
//! state, and call the mixin methods through `Alignment { src, d: &data }`.

use crate::helpers::parser_tokens::{T_OP_ASGN, is_ruby_space, translated, within};
use crate::source::{Source, Token};
use ruby_prism::{Node, Visit};
use ruby_prism_sys as sys;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// ASSIGNMENT_OR_COMPARISON_TOKENS: tEQL tEQ tEQQ tNEQ tLEQ tGEQ tOP_ASGN tLSHFT.
pub fn assignment_or_comparison(t: &Token) -> bool {
    matches!(
        t.kind,
        sys::PM_TOKEN_EQUAL
            | sys::PM_TOKEN_EQUAL_EQUAL
            | sys::PM_TOKEN_EQUAL_EQUAL_EQUAL
            | sys::PM_TOKEN_BANG_EQUAL
            | sys::PM_TOKEN_LESS_EQUAL
            | sys::PM_TOKEN_GREATER_EQUAL
            | T_OP_ASGN
            | sys::PM_TOKEN_LESS_LESS
    )
}

/// Token#equal_sign? (tEQL, tOP_ASGN).
pub fn equal_sign(t: &Token) -> bool {
    matches!(t.kind, sys::PM_TOKEN_EQUAL | T_OP_ASGN)
}

/// A source range as the mixin sees it (start/end byte offsets).
#[derive(Clone, Copy, Debug)]
pub struct Range {
    pub start: usize,
    pub end: usize,
}

#[derive(Default)]
struct AstFacts {
    /// `first_line`/`last_line` of every def/defs (definition_boundary_lines).
    def_lines: HashSet<usize>,
    /// Offsets of `=` in optargs and endless `def` (not `defs`), see remove_equals_in_def.
    def_equals: HashSet<usize>,
}

struct Collect<'a, 's> {
    src: &'a Source<'s>,
    facts: AstFacts,
}

impl<'pr> Visit<'pr> for Collect<'_, '_> {
    fn visit_def_node(&mut self, node: &ruby_prism::DefNode<'pr>) {
        let loc = node.location();
        self.facts.def_lines.insert(self.src.line_of(loc.start_offset()));
        self.facts.def_lines.insert(self.src.line_of(loc.end_offset()));
        if node.receiver().is_none()
            && let Some(eq) = node.equal_loc()
        {
            self.facts.def_equals.insert(eq.start_offset());
        }
        ruby_prism::visit_def_node(self, node);
    }

    fn visit_optional_parameter_node(&mut self, node: &ruby_prism::OptionalParameterNode<'pr>) {
        self.facts.def_equals.insert(node.operator_loc().start_offset());
        ruby_prism::visit_optional_parameter_node(self, node);
    }
}

/// File-wide data the mixin memoizes (`@assignment_tokens`, `@aligned_comment_lines`, ...).
#[derive(Default)]
pub struct Data {
    /// processed_source.tokens (translation lexer, source order).
    pub tokens: Vec<Token>,
    pub aligned_comment_lines: HashSet<usize>,
    pub def_lines: HashSet<usize>,
    /// First `=`/`op=` token per line (assignment_tokens).
    pub assignment_tokens: Vec<Token>,
    pub assignment_lines: HashSet<usize>,
    pub interrupting_lines: HashSet<usize>,
    alignment_lines: RefCell<HashMap<usize, Rc<Vec<usize>>>>,
}

impl Data {
    pub fn new(src: &Source, program: &Node) -> Self {
        let mut c = Collect { src, facts: AstFacts::default() };
        c.visit(program);
        let facts = c.facts;
        let tokens = translated(src);
        let aligned_comment_lines =
            src.comments.iter().filter(|c| src.begins_its_line(c.start)).map(|c| src.line_of(c.start)).collect();
        let mut seen = HashSet::new();
        let assignment_tokens: Vec<Token> = tokens
            .iter()
            .filter(|t| equal_sign(t) && !facts.def_equals.contains(&t.start))
            .filter(|t| seen.insert(src.line_of(t.start)))
            .copied()
            .collect();
        let assignment_lines: HashSet<usize> = assignment_tokens.iter().map(|t| src.line_of(t.start)).collect();
        let interrupting_lines = tokens
            .iter()
            .filter(|t| assignment_or_comparison(t))
            .map(|t| src.line_of(t.start))
            .filter(|l| !assignment_lines.contains(l))
            .collect();
        Data {
            tokens,
            aligned_comment_lines,
            def_lines: facts.def_lines,
            assignment_tokens,
            assignment_lines,
            interrupting_lines,
            alignment_lines: RefCell::default(),
        }
    }
}

/// The mixin's methods over a file's `Data`.
pub struct Alignment<'a, 's> {
    pub src: &'a Source<'s>,
    pub d: &'a Data,
}

impl<'s> Alignment<'_, 's> {
    pub fn line(&self, off: usize) -> usize {
        self.src.line_of(off)
    }

    fn lines(&self) -> &[&'s str] {
        &self.src.lines
    }

    /// processed_source.line_indentation(line): leading whitespace length.
    pub fn line_indentation(&self, line: usize) -> usize {
        self.lines().get(line.wrapping_sub(1)).map_or(0, |l| l.chars().take_while(|&c| is_space(c)).count())
    }

    /// String#blank? on a line.
    pub fn blank(&self, line: usize) -> bool {
        self.lines().get(line.wrapping_sub(1)).is_none_or(|l| l.chars().all(|c| is_space(c) || c == '\0'))
    }

    fn source(&self, r: Range) -> &'s str {
        self.src.slice(r.start, r.end)
    }

    /// aligned_with_something?(range)
    pub fn aligned_with_something(&self, r: Range) -> bool {
        self.aligned_with_adjacent_line(r, &|r, line, lineno| {
            self.aligned_words(r, line) || self.aligned_equals_operator(r, lineno)
        })
    }

    /// aligned_with_operator?(range)
    pub fn aligned_with_operator(&self, r: Range) -> bool {
        self.aligned_with_adjacent_line(r, &|r, line, lineno| {
            let token = self.source(r);
            token == char_slice(line, self.src.column(r.start), token.chars().count())
                || self.aligned_equals_operator(r, lineno)
        })
    }

    fn aligned_with_adjacent_line(&self, r: Range, pred: &dyn Fn(Range, &str, usize) -> bool) -> bool {
        let line = self.line(r.start);
        let n = self.lines().len();
        // pre = (line - 2).downto(0), post = line.upto(lines.size - 1): 0-based line indexes.
        let pre: Vec<usize> = (0..line.saturating_sub(1)).rev().collect();
        let post: Vec<usize> = (line..n).collect();
        let ranges = [pre, post];
        if ranges.iter().any(|nos| self.aligned_with_line(nos, r, None, pred)) {
            return true;
        }
        let base = self.lines().get(line - 1).and_then(|l| first_non_space(l));
        ranges.iter().any(|nos| self.aligned_with_line(nos, r, base, pred))
    }

    fn aligned_with_line(
        &self,
        line_nos: &[usize],
        r: Range,
        indent: Option<usize>,
        pred: &dyn Fn(Range, &str, usize) -> bool,
    ) -> bool {
        for &lineno in line_nos {
            if self.d.aligned_comment_lines.contains(&(lineno + 1)) {
                continue;
            }
            let line = self.lines()[lineno];
            let Some(index) = first_non_space(line) else { continue };
            if let Some(indent) = indent {
                if index < indent {
                    break;
                }
                if index > indent {
                    continue;
                }
            }
            return pred(r, line, lineno + 1);
        }
        false
    }

    fn aligned_words(&self, r: Range, line: &str) -> bool {
        let left_edge = self.src.column(r.start);
        if left_edge > 0 {
            let two: Vec<char> = line.chars().skip(left_edge - 1).take(2).collect();
            if two.len() == 2 && is_space(two[0]) && !is_space(two[1]) {
                return true;
            }
        }
        let token = self.source(r);
        token == char_slice(line, left_edge, token.chars().count())
    }

    /// aligned_equals_operator?(range, lineno)
    pub fn aligned_equals_operator(&self, r: Range, lineno: usize) -> bool {
        if lineno == 0 || lineno > self.src.line_count() {
            return false;
        }
        let start = self.src.line_start(lineno);
        let end = self.src.text[start..].find('\n').map_or(self.src.text.len(), |i| start + i);
        let Some(op) = within(&self.d.tokens, start, end).iter().find(|t| assignment_or_comparison(t)).copied() else {
            return false;
        };
        let source = self.source(r);
        let last_col = self.src.column(r.end);
        let op_last_col = self.src.column(op.end);
        if source.ends_with('=') && last_col == op_last_col {
            return true;
        }
        ((source == "<<" && equal_sign(&op)) || (source.ends_with('=') && op.kind == sys::PM_TOKEN_LESS_LESS))
            && last_col == op_last_col
    }

    /// aligned_with_preceding_equals_operator(token) (`preceding`) or
    /// aligned_with_subsequent_equals_operator(token): `Some(true)` = :yes, `Some(false)` = :no,
    /// `None` = :none.
    pub fn aligned_with_equals_sign(&self, r: Range, preceding: bool) -> Option<bool> {
        let line = self.line(r.start);
        let token_indent = self.line_indentation(line);
        let range: Vec<usize> =
            if preceding { (1..=line).rev().collect() } else { (line..=self.lines().len()).collect() };
        let relevant = *self.relevant_assignment_lines(&range).get(1)?;
        if self.line_indentation(relevant) < token_indent || relevant > self.lines().len() {
            return None;
        }
        Some(self.aligned_equals_operator(r, relevant))
    }

    /// relevant_lines(line_range, boundary_lines) { |line| ... }
    pub fn relevant_lines(&self, range: &[usize], boundary: &HashSet<usize>, pred: impl Fn(usize) -> bool) -> Vec<usize> {
        let mut result = Vec::new();
        let Some(&original) = range.first() else { return result };
        let original_indent = self.line_indentation(original);
        let mut at_level = true;
        for &line in range {
            let indent = self.line_indentation(line);
            let blank = self.blank(line);
            if (line != original && boundary.contains(&line))
                || (indent < original_indent && !blank)
                || (at_level && blank)
            {
                break;
            }
            if pred(line) && indent == original_indent {
                result.push(line);
            }
            if !blank {
                at_level = indent == original_indent;
            }
        }
        result
    }

    /// relevant_assignment_lines(line_range)
    pub fn relevant_assignment_lines(&self, range: &[usize]) -> Vec<usize> {
        self.relevant_lines(range, &self.d.interrupting_lines, |l| self.d.assignment_lines.contains(&l))
    }

    /// alignment_lines(line_number), memoized per group.
    pub fn alignment_lines(&self, line: usize) -> Rc<Vec<usize>> {
        if let Some(v) = self.d.alignment_lines.borrow().get(&line) {
            return v.clone();
        }
        let n = self.lines().len();
        let not_comment = |l: usize| !self.d.aligned_comment_lines.contains(&l);
        let down: Vec<usize> = (1..=line).rev().collect();
        let up: Vec<usize> = (line..=n).collect();
        let mut lines = self.relevant_lines(&down, &self.d.def_lines, not_comment);
        lines.extend(self.relevant_lines(&up, &self.d.def_lines, not_comment));
        lines.sort_unstable();
        lines.dedup();
        let rc = Rc::new(lines);
        let mut memo = self.d.alignment_lines.borrow_mut();
        for &l in rc.iter() {
            memo.insert(l, rc.clone());
        }
        rc
    }
}

fn is_space(c: char) -> bool {
    c.is_ascii() && is_ruby_space(c as u8)
}

/// `line =~ /\S/` as a character index.
fn first_non_space(line: &str) -> Option<usize> {
    line.chars().position(|c| !is_space(c))
}

/// `line[col, len]` on characters.
pub fn char_slice(line: &str, col: usize, len: usize) -> &str {
    let Some(s) = line.char_indices().map(|(i, _)| i).chain(std::iter::once(line.len())).nth(col) else {
        return "";
    };
    let e = line[s..].char_indices().nth(len).map_or(line.len(), |(i, _)| s + i);
    &line[s..e]
}
