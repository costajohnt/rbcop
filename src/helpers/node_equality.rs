//! rubocop-ast structural node equality (`node1 == node2`, `Set#add?`, `uniq`, `include?`): the
//! parser-gem trees of the two nodes (type and children, so literals compare by value), from the
//! node of `Source::root` at each range.

use crate::helpers::parser_sexp::{hash_sexp, sexp};
use crate::source::Source;
use ruby_prism::Node;
use ruby_prism_sys as sys;
use std::collections::HashMap;

/// Every node of `root` by range: the outermost one, past the Prism-only Program and Arguments
/// wrappers (a parser `begin` of several statements is the StatementsNode).
pub(crate) fn index(root: &Node) -> HashMap<(usize, usize), Node<'static>> {
    use ruby_prism::Visit;
    struct Index<'pr>(HashMap<(usize, usize), Node<'pr>>);
    impl<'pr> Index<'pr> {
        fn add(&mut self, node: Node<'pr>) {
            if !matches!(node, Node::ProgramNode { .. } | Node::ArgumentsNode { .. }) {
                let l = node.location();
                self.0.entry((l.start_offset(), l.end_offset())).or_insert(node);
            }
        }
    }
    impl<'pr> Visit<'pr> for Index<'pr> {
        fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
            self.add(node);
        }
        fn visit_leaf_node_enter(&mut self, node: Node<'pr>) {
            self.add(node);
        }
    }
    let mut ix = Index(HashMap::new());
    ix.visit(root);
    // SAFETY: only the lifetime changes. The map lives in the Source that holds `root`, so it
    // never outlives the parse, and Source::node_at hands its nodes out at the Source's lifetime.
    unsafe { std::mem::transmute::<HashMap<(usize, usize), Node<'_>>, HashMap<(usize, usize), Node<'static>>>(ix.0) }
}

/// The parser-gem tree of the node at `range` (with `hash`: and the one its cached hash
/// describes, which below 3.3 can differ), or None when there is no such node or it does not print.
fn tree(src: &Source, range: (usize, usize), hash: bool) -> Option<Vec<String>> {
    let node = src.node_at(range)?;
    let tree = sexp(node, src)?;
    // `0.0 == -0.0`, though they are not `eql?`.
    let mut key = vec![if hash { tree } else { tree.replace("(float -0.0)", "(float 0.0)") }];
    if hash && src.parser_gem {
        key.push(hash_sexp(node, src)?);
    }
    Some(key)
}

/// A grouping key for `==` (`include?`): equal keys, equal nodes. See `hash_shape` for `uniq` / Set.
pub fn shape(src: &Source, start: usize, end: usize) -> Vec<String> {
    tree(src, (start, end), false).unwrap_or_else(|| tokens(src, start, end))
}

/// A grouping key for `eql?` + `hash` (`uniq`, `Set#add?`, `group_by`, `Array#-`).
pub fn hash_shape(src: &Source, start: usize, end: usize) -> Vec<String> {
    tree(src, (start, end), true).unwrap_or_else(|| tokens(src, start, end))
}

/// Fallback where a range is no node or its tree does not print: the significant token texts,
/// comments and ignorable newlines dropped, statement separators (`;` / newline) unified. Plain
/// `'..'` / `".."` literals without escapes or interpolation compare by value.
/// ponytail: token-based, so `foo()` vs `foo` or escaped vs unescaped spellings of one string count
/// as different although parser builds the same node.
fn tokens(src: &Source, start: usize, end: usize) -> Vec<String> {
    let tokens = src.tokens();
    let first = tokens.partition_point(|t| t.start < start);
    let in_range: Vec<&crate::source::Token> = tokens[first..].iter().take_while(|t| t.end <= end).collect();
    let mut out = significant(src, in_range.iter().copied());
    // A heredoc's body lies after its opening line, outside the node's range, but parser's node
    // includes it: append each body, matched to its opener by order on that line.
    for opener in in_range.iter().filter(|t| t.kind == sys::PM_TOKEN_HEREDOC_START) {
        let line = src.line_of(opener.start);
        let line_from = tokens.partition_point(|t| t.start < src.line_start(line));
        let nth = tokens[line_from..opener_index(tokens, opener)]
            .iter()
            .filter(|t| t.kind == sys::PM_TOKEN_HEREDOC_START && src.line_of(t.start) == line)
            .count();
        if line < src.line_count() {
            let body_from = tokens.partition_point(|t| t.start < src.line_start(line + 1));
            let body = tokens[body_from..].split_inclusive(|t| t.kind == sys::PM_TOKEN_HEREDOC_END).nth(nth);
            out.push("<<".to_string());
            out.extend(significant(src, body.unwrap_or_default().iter()));
        }
    }
    out
}

fn opener_index(tokens: &[crate::source::Token], opener: &crate::source::Token) -> usize {
    tokens.partition_point(|t| (t.start, t.end) < (opener.start, opener.end))
}

fn significant<'a>(src: &Source, tokens: impl Iterator<Item = &'a crate::source::Token>) -> Vec<String> {
    let text = |t: &crate::source::Token| &src.text[t.start..t.end];
    let mut out: Vec<String> = Vec::new();
    // A plain quoted string seen so far: (index in `out` of its opening quote, quote).
    let mut open: Option<(usize, &str)> = None;
    for t in tokens {
        if matches!(
            t.kind,
            sys::PM_TOKEN_COMMENT
                | sys::PM_TOKEN_IGNORED_NEWLINE
                | sys::PM_TOKEN_EMBDOC_BEGIN
                | sys::PM_TOKEN_EMBDOC_LINE
                | sys::PM_TOKEN_EMBDOC_END
                | sys::PM_TOKEN_EOF
        ) {
            continue;
        }
        let s = text(t);
        match (t.kind, open) {
            (sys::PM_TOKEN_STRING_END, Some((i, q))) if s == q && out.len() <= i + 2 => {
                let value = out.get(i + 1).cloned().unwrap_or_default();
                out.truncate(i);
                out.push(format!("\0str:{value}"));
                open = None;
                continue;
            }
            (sys::PM_TOKEN_STRING_CONTENT, Some((i, _))) if out.len() == i + 1 && !s.contains('\\') => {}
            _ => open = None,
        }
        if t.kind == sys::PM_TOKEN_STRING_BEGIN && (s == "'" || s == "\"") {
            open = Some((out.len(), s));
        }
        out.push(match t.kind {
            sys::PM_TOKEN_NEWLINE | sys::PM_TOKEN_SEMICOLON => ";".to_string(),
            _ => s.to_string(),
        });
    }
    out
}

/// `node1 == node2` for two byte ranges.
pub fn same(src: &Source, a: (usize, usize), b: (usize, usize)) -> bool {
    equal(src, a, b, false)
}

/// `node1.eql?(node2) && node1.hash == node2.hash` (Set, Hash keys, `uniq`, `Array#-` / `&`).
pub fn same_hash(src: &Source, a: (usize, usize), b: (usize, usize)) -> bool {
    equal(src, a, b, true)
}

fn equal(src: &Source, a: (usize, usize), b: (usize, usize), hash: bool) -> bool {
    let (ta, tb) = (&src.text[a.0..a.1], &src.text[b.0..b.1]);
    // Equal text proves equality unless a heredoc opener hides different bodies.
    // ponytail: `foo` read as a local and `foo` sent as a method count as equal here.
    if ta == tb && !ta.contains("<<") {
        return true;
    }
    match (tree(src, a, hash), tree(src, b, hash)) {
        (Some(x), Some(y)) => x == y,
        _ => tokens(src, a.0, a.1) == tokens(src, b.0, b.1),
    }
}
