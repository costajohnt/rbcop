//! rubocop-sorbet's SignatureHelp (`sig { ... }` blocks), the parser-sibling and call-search pieces
//! the signature cops share, and the parts of RuboCop::Sorbet::RBSParser they use: the `# @`
//! annotations and `#:` signatures attached to a node, and an RBS method type's return type.

use crate::helpers::comment_association::{self, Associator};
use crate::helpers::parser_parent::{is_top_const, parser_parent_index};
use crate::prelude::*;
use crate::source::Source;
use ruby_prism::{CallNode, Visit};
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Sig {
    /// `sig { }`
    Bare,
    /// `T::Sig.sig { }`
    WithRuntime,
    /// `T::Sig::WithoutRuntime.sig { }`
    WithoutRuntime,
}

/// `(const (const {nil? cbase} :T) :Name)`
pub fn is_t_const(node: &Node, name: &str) -> bool {
    node.as_constant_path_node().is_some_and(|p| {
        p.name().is_some_and(|n| n.as_slice() == name.as_bytes()) && p.parent().is_some_and(|t| is_top_const(&t, "T"))
    })
}

/// SignatureHelp#signature?: `(block (send RECV :sig (sym :final)?) (args) ...)` with RECV nil,
/// `T::Sig` or `T::Sig::WithoutRuntime`. A numbered or `it` block is not a `block`.
pub fn signature(node: &Node) -> Option<Sig> {
    let call = node.as_call_node()?;
    let block = call.block()?.as_block_node()?;
    let empty_args = match block.parameters() {
        None => true,
        Some(p) => p.as_block_parameters_node().is_some_and(|b| b.parameters().is_none() && b.locals().iter().next().is_none()),
    };
    if !empty_args || is_csend(&call) || call.name().as_slice() != b"sig" {
        return None;
    }
    let args = call_args(&call);
    let final_sym = |a: &Node| a.as_symbol_node().is_some_and(|s| s.unescaped() == b"final");
    if !(args.is_empty() || matches!(args.as_slice(), [a] if final_sym(a))) {
        return None;
    }
    let Some(recv) = call.receiver() else { return Some(Sig::Bare) };
    if is_t_const(&recv, "Sig") {
        return Some(Sig::WithRuntime);
    }
    let without = recv.as_constant_path_node().is_some_and(|p| {
        p.name().is_some_and(|n| n.as_slice() == b"WithoutRuntime") && p.parent().is_some_and(|s| is_t_const(&s, "Sig"))
    });
    without.then_some(Sig::WithoutRuntime)
}

/// The parser `body` of a block call: its single statement, else the multi-statement (or
/// rescue/ensure) body, which is never a send.
pub fn block_body<'pr>(call: &CallNode<'pr>) -> Option<Node<'pr>> {
    let body = call.block()?.as_block_node()?.body()?;
    if let Some(s) = body.as_statements_node() {
        let v: Vec<Node> = s.body().iter().collect();
        if v.len() == 1 {
            return v.into_iter().next();
        }
    }
    Some(body)
}

/// `node.send_type?`: a call that is not `&.` and does not own a literal block (parser `block`).
pub fn as_send<'pr>(node: &Node<'pr>) -> Option<CallNode<'pr>> {
    node.as_call_node().filter(|c| !is_csend(c) && c.block().is_none_or(|b| b.as_block_node().is_none()))
}

/// The statements of a parser `begin` (2+ statements), the only parent whose children other
/// statements are: `node.parent.children` siblings elsewhere are the node's own parts.
// ponytail: an if/case branch's siblings (the other branches) are not followed.
pub fn siblings<'pr>(parent: Option<&Node<'pr>>) -> Option<Vec<Node<'pr>>> {
    let v: Vec<Node> = parent?.as_statements_node()?.body().iter().collect();
    (v.len() >= 2).then_some(v)
}

/// Index of `range` among `siblings`.
pub fn index_of(siblings: &[Node], range: (usize, usize)) -> Option<usize> {
    siblings.iter().position(|s| helpers::node_types::range(s) == range)
}

/// Every call in `node` (itself included) in pre-order, as `def_node_search` sees parser sends
/// (a call owning a block is the block's send).
pub fn each_call<'pr>(node: &Node<'pr>, f: impl FnMut(&CallNode<'pr>)) {
    struct V<F>(F);
    impl<'pr, F: FnMut(&CallNode<'pr>)> Visit<'pr> for V<F> {
        fn visit_call_node(&mut self, node: &CallNode<'pr>) {
            (self.0)(node);
            ruby_prism::visit_call_node(self, node);
        }
    }
    V(f).visit(node);
}

/// `node = node.parent while node.parent&.send_type?` for the node whose ancestors are
/// `ancestors`: the outermost send's index in `ancestors`, None when the node is not wrapped.
pub fn outermost_send(ancestors: &[Node]) -> Option<usize> {
    let mut i = ancestors.len();
    let mut found = None;
    while let Some(p) = parser_parent_index(ancestors, i) {
        if ancestors[p].as_call_node().is_none_or(|c| is_csend(&c)) {
            break;
        }
        found = Some(p);
        // A call owning a block is the block's send: its parser parent is the block.
        if ancestors.get(p + 1).is_some_and(|n| n.as_block_node().is_some()) {
            break;
        }
        i = p;
    }
    found
}

/// The parser range of the outermost send around a node (`outermost_send`), else `own`.
pub fn outermost_range(ancestors: &[Node], own: (usize, usize)) -> (usize, usize) {
    match outermost_send(ancestors) {
        Some(i) => send_range(&ancestors[i].as_call_node().unwrap()),
        None => own,
    }
}

/// `processed_source.ast_with_comments` by parser node range, built once per file.
#[derive(Default)]
pub struct Associated(Option<HashMap<(usize, usize), Vec<usize>>>);

/// RBSParser.comments_above: the comments parser's associator gives the node spanning `range`
/// that end the contiguous comment block right above its first line.
pub fn comments_above(cx: &mut Cx, range: (usize, usize)) -> Vec<Comment> {
    let src = cx.src;
    let Some(program) = cx.ancestors.first() else { return Vec::new() };
    let map = cx.state::<Associated>().0.get_or_insert_with(|| {
        let mut a = Associator::new(src, comment_association::skipped_directives(src));
        a.visit(program);
        let mut map = HashMap::new();
        for (start, end, comments) in a.nodes {
            map.entry((start, end)).or_insert(comments);
        }
        map
    });
    let line = src.line_of(range.0);
    let mut comments: Vec<Comment> =
        map.get(&range).into_iter().flatten().map(|&i| src.comments[i]).filter(|c| src.line_of(c.start) < line).collect();
    comments.sort_by_key(|c| src.line_of(c.start));
    let mut block = Vec::new();
    let mut expected = line;
    for c in comments.into_iter().rev() {
        if src.line_of(c.start) + 1 != expected {
            break;
        }
        expected = src.line_of(c.start);
        block.insert(0, c);
    }
    block
}

/// RBSParser.rbs_annotations_before: the `# @` comments above the node spanning `range`.
pub fn annotations_before(cx: &mut Cx, range: (usize, usize)) -> Vec<Comment> {
    static ANNOTATION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A#[ \t\r\n\x0b\x0c]*@").unwrap());
    let src = cx.src;
    comments_above(cx, range).into_iter().filter(|c| ANNOTATION.is_match(src.slice(c.start, c.end))).collect()
}

/// RBSParser.rbs_signatures_before: the `#:` signatures (with their `#|` continuation lines) above
/// the node spanning `range`, one group per overload.
pub fn signatures_before(cx: &mut Cx, range: (usize, usize)) -> Vec<Vec<Comment>> {
    let src = cx.src;
    let mut groups: Vec<Vec<Comment>> = Vec::new();
    for c in comments_above(cx, range) {
        let text = src.slice(c.start, c.end);
        if text.starts_with("#:") {
            groups.push(vec![c]);
        } else if text.starts_with("#|")
            && let Some(g) = groups.last_mut()
        {
            g.push(c);
        }
    }
    groups
}

/// An RBS signature's return type: whether it is `void`, the highlight range (its first
/// non-blank run) and the range of the whole type, as byte offsets in the file.
pub struct ReturnType {
    pub void: bool,
    pub highlight: (usize, usize),
    pub replace: (usize, usize),
}

/// RBSParser::Signature#return_type_range and #void?: the group's comment bodies joined by
/// newlines (RBS::Buffer#sub_buffer) parsed as a method type; None when it does not parse.
// ponytail: RBS locations are characters and the gem adds byte sizes to them; offsets here are
// bytes throughout, which agree for ASCII signatures.
pub fn return_type(src: &Source, group: &[Comment]) -> Option<ReturnType> {
    static MARKER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A#[:|][ \t\r\n\x0b\x0c]*").unwrap());
    let mut text = String::new();
    // (offset in `text`, offset in the file, length) per comment body.
    let mut segs: Vec<(usize, usize, usize)> = Vec::new();
    for c in group {
        let body = src.slice(c.start, c.end);
        let skip = MARKER.find(body).map_or(0, |m| m.end());
        if !segs.is_empty() {
            text.push('\n');
        }
        segs.push((text.len(), c.start + skip, body.len() - skip));
        text.push_str(&body[skip..]);
    }
    let (start, end, void) = rbs::method_return(text.as_bytes())?;
    let map = |p: usize| {
        let &(t, f, _) = segs.iter().rev().find(|s| s.0 <= p).unwrap();
        f + (p - t)
    };
    let source = &text[start..end];
    let first = source.find(|c: char| !helpers::ruby_string::is_space(c))?;
    let len = source[first..].find(helpers::ruby_string::is_space).unwrap_or(source.len() - first);
    let h = map(start) + first;
    Some(ReturnType { void, highlight: (h, h + len), replace: (map(start), map(end)) })
}

/// A recursive-descent reader of the RBS method type grammar (rbs 4.x `parse_method_type` with
/// `require_eof: true`), only as far as finding the return type's location.
mod rbs {
    /// (start, end, is `void`) of a parsed type.
    type Ty = (usize, usize, bool);

    struct P<'a> {
        s: &'a [u8],
        i: usize,
    }

    fn ident_start(c: u8) -> bool {
        c.is_ascii_alphabetic() || c == b'_' || c >= 0x80
    }

    fn ident_char(c: u8) -> bool {
        c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80
    }

    const BASES: &[&str] = &["bool", "bot", "class", "instance", "nil", "self", "top", "untyped", "void", "true", "false"];

    impl P<'_> {
        /// Skip blanks, newlines and `#` line comments.
        fn ws(&mut self) {
            loop {
                while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
                    self.i += 1;
                }
                if self.s.get(self.i) == Some(&b'#') {
                    while self.i < self.s.len() && self.s[self.i] != b'\n' {
                        self.i += 1;
                    }
                } else {
                    return;
                }
            }
        }

        fn peek(&mut self) -> Option<u8> {
            self.ws();
            self.s.get(self.i).copied()
        }

        fn at(&mut self, lit: &str) -> bool {
            self.ws();
            self.s[self.i..].starts_with(lit.as_bytes())
        }

        fn eat(&mut self, lit: &str) -> bool {
            let ok = self.at(lit);
            if ok {
                self.i += lit.len();
            }
            ok
        }

        fn expect(&mut self, lit: &str) -> Option<()> {
            self.eat(lit).then_some(())
        }

        /// An identifier at the cursor (no blanks skipped), with an optional `?`/`!` suffix.
        fn word(&mut self) -> Option<(usize, usize)> {
            let start = self.i;
            if !self.s.get(self.i).is_some_and(|&c| ident_start(c)) {
                return None;
            }
            while self.s.get(self.i).is_some_and(|&c| ident_char(c)) {
                self.i += 1;
            }
            Some((start, self.i))
        }

        fn keyword(&mut self, w: &str) -> bool {
            self.ws();
            let save = self.i;
            match self.word() {
                Some((s, e)) if &self.s[s..e] == w.as_bytes() => true,
                _ => {
                    self.i = save;
                    false
                }
            }
        }

        /// A keyword label `name:` (not `name::`), consumed when present.
        fn label(&mut self) -> bool {
            self.ws();
            let save = self.i;
            if self.s.get(self.i) == Some(&b'`') {
                self.backquoted();
            } else if self.word().is_some() && matches!(self.s.get(self.i), Some(b'?' | b'!')) {
                self.i += 1;
            }
            if self.i > save && self.s.get(self.i) == Some(&b':') && self.s.get(self.i + 1) != Some(&b':') {
                self.i += 1;
                return true;
            }
            self.i = save;
            false
        }

        fn backquoted(&mut self) -> bool {
            let save = self.i;
            self.i += 1;
            while self.i < self.s.len() && !matches!(self.s[self.i], b'`' | b'\n') {
                self.i += 1;
            }
            if self.s.get(self.i) == Some(&b'`') {
                self.i += 1;
                return true;
            }
            self.i = save;
            false
        }

        /// A parameter's optional name.
        fn param_name(&mut self) {
            self.ws();
            match self.s.get(self.i) {
                Some(b'`') => {
                    self.backquoted();
                }
                Some(&c) if c.is_ascii_lowercase() || c == b'_' => {
                    self.word();
                }
                _ => {}
            }
        }

        fn method_type(&mut self) -> Option<Ty> {
            if self.peek() == Some(b'[') {
                self.type_params()?;
            }
            if self.peek() == Some(b'(') {
                self.params()?;
            }
            if self.at("?") || self.at("{") {
                self.block()?;
            }
            self.expect("->")?;
            let ret = self.optional()?;
            self.ws();
            (self.i == self.s.len()).then_some(ret)
        }

        fn type_params(&mut self) -> Option<()> {
            self.expect("[")?;
            loop {
                self.keyword("unchecked");
                let _ = self.keyword("in") || self.keyword("out");
                self.ws();
                if !self.s.get(self.i).is_some_and(|c| c.is_ascii_uppercase()) {
                    return None;
                }
                self.word()?;
                if self.eat("<") {
                    self.ty()?;
                }
                if self.eat("=") {
                    self.ty()?;
                }
                if !self.eat(",") {
                    break;
                }
            }
            self.expect("]")
        }

        fn params(&mut self) -> Option<()> {
            self.expect("(")?;
            if self.eat(")") {
                return Some(());
            }
            let save = self.i;
            if self.eat("?") && self.eat(")") {
                return Some(());
            }
            self.i = save;
            loop {
                self.param()?;
                if !self.eat(",") || self.at(")") {
                    break;
                }
            }
            self.expect(")")
        }

        fn param(&mut self) -> Option<()> {
            if self.eat("**") || self.eat("*") {
                self.ty()?;
            } else {
                self.eat("?");
                self.label();
                self.ty()?;
            }
            self.param_name();
            Some(())
        }

        fn self_binding(&mut self) -> Option<()> {
            self.expect("[")?;
            if !self.keyword("self") {
                return None;
            }
            self.expect(":")?;
            self.ty()?;
            self.expect("]")
        }

        fn block(&mut self) -> Option<()> {
            self.eat("?");
            self.expect("{")?;
            if self.peek() == Some(b'(') {
                self.params()?;
            }
            if self.peek() == Some(b'[') {
                self.self_binding()?;
            }
            self.expect("->")?;
            self.optional()?;
            self.expect("}")
        }

        /// A union of intersections.
        fn ty(&mut self) -> Option<Ty> {
            let first = self.intersection()?;
            let mut end = first.1;
            let mut more = false;
            while self.eat("|") {
                end = self.intersection()?.1;
                more = true;
            }
            Some(if more { (first.0, end, false) } else { first })
        }

        fn intersection(&mut self) -> Option<Ty> {
            let first = self.optional()?;
            let mut end = first.1;
            let mut more = false;
            while self.eat("&") {
                end = self.optional()?.1;
                more = true;
            }
            Some(if more { (first.0, end, false) } else { first })
        }

        fn optional(&mut self) -> Option<Ty> {
            let t = self.simple()?;
            let save = self.i;
            // `?` before `{` opens an optional block, not an optional type.
            if self.eat("?") {
                if !self.at("{") {
                    return Some((t.0, self.i, false));
                }
                self.i = save;
            }
            Some(t)
        }

        fn type_args(&mut self) -> Option<()> {
            if self.s.get(self.i) == Some(&b'[') {
                self.i += 1;
                if !self.eat("]") {
                    loop {
                        self.ty()?;
                        if !self.eat(",") || self.at("]") {
                            break;
                        }
                    }
                    self.expect("]")?;
                }
            }
            Some(())
        }

        fn simple(&mut self) -> Option<Ty> {
            self.ws();
            let start = self.i;
            let c = *self.s.get(self.i)?;
            match c {
                b'(' => {
                    self.i += 1;
                    let t = self.ty()?;
                    self.expect(")")?;
                    Some(t)
                }
                b'^' => {
                    self.i += 1;
                    if self.peek() == Some(b'(') {
                        self.params()?;
                    }
                    if self.peek() == Some(b'[') {
                        self.self_binding()?;
                    }
                    if self.at("?") || self.at("{") {
                        self.block()?;
                    }
                    self.expect("->")?;
                    let r = self.optional()?;
                    Some((start, r.1, false))
                }
                b'[' => {
                    self.i += 1;
                    if !self.eat("]") {
                        loop {
                            self.ty()?;
                            if !self.eat(",") || self.at("]") {
                                break;
                            }
                        }
                        self.expect("]")?;
                    }
                    Some((start, self.i, false))
                }
                b'{' => {
                    self.i += 1;
                    if !self.eat("}") {
                        loop {
                            self.eat("?");
                            if !self.label() {
                                self.simple()?;
                                self.expect("=>")?;
                            }
                            self.ty()?;
                            if !self.eat(",") || self.at("}") {
                                break;
                            }
                        }
                        self.expect("}")?;
                    }
                    Some((start, self.i, false))
                }
                b'"' | b'\'' => {
                    self.i += 1;
                    while self.i < self.s.len() && self.s[self.i] != c {
                        self.i += if self.s[self.i] == b'\\' { 2 } else { 1 };
                    }
                    (self.i < self.s.len()).then(|| {
                        self.i += 1;
                        (start, self.i, false)
                    })
                }
                b':' if self.s.get(self.i + 1) != Some(&b':') => {
                    self.i += 1;
                    match self.s.get(self.i) {
                        Some(b'"' | b'\'') => {
                            self.simple()?;
                        }
                        Some(&d) if ident_start(d) => {
                            self.word();
                            if matches!(self.s.get(self.i), Some(b'?' | b'!' | b'=')) {
                                self.i += 1;
                            }
                        }
                        _ => {
                            let n = self.i;
                            while self.s.get(self.i).is_some_and(|c| b"+-*/%<>=!~^&|[]@`".contains(c)) {
                                self.i += 1;
                            }
                            if self.i == n {
                                return None;
                            }
                        }
                    }
                    Some((start, self.i, false))
                }
                b'-' | b'0'..=b'9' => {
                    self.i += 1;
                    let n = self.i;
                    while self.s.get(self.i).is_some_and(|c| c.is_ascii_digit() || *c == b'_') {
                        self.i += 1;
                    }
                    (c != b'-' || self.i > n).then_some((start, self.i, false))
                }
                _ => {
                    if self.s[self.i..].starts_with(b"::") {
                        self.i += 2;
                    }
                    let (ws, we) = self.word()?;
                    let mut segments = 1;
                    while self.s[self.i..].starts_with(b"::") {
                        self.i += 2;
                        self.word()?;
                        segments += 1;
                    }
                    let name = &self.s[ws..we];
                    if start == ws && segments == 1 {
                        if name == b"singleton" {
                            self.expect("(")?;
                            self.ws();
                            if self.s[self.i..].starts_with(b"::") {
                                self.i += 2;
                            }
                            self.word()?;
                            while self.s[self.i..].starts_with(b"::") {
                                self.i += 2;
                                self.word()?;
                            }
                            self.expect(")")?;
                            return Some((start, self.i, false));
                        }
                        if let Some(b) = BASES.iter().find(|b| b.as_bytes() == name) {
                            return Some((start, self.i, *b == "void"));
                        }
                    }
                    self.type_args()?;
                    Some((start, self.i, false))
                }
            }
        }
    }

    /// The return type of an RBS method type, if `s` parses as one.
    pub fn method_return(s: &[u8]) -> Option<Ty> {
        P { s, i: 0 }.method_type()
    }

    #[cfg(test)]
    mod tests {
        use super::method_return;

        fn ret(s: &str) -> Option<(String, bool)> {
            method_return(s.as_bytes()).map(|(a, b, v)| (s[a..b].to_string(), v))
        }

        #[test]
        fn return_types() {
            assert_eq!(ret("(String) -> String"), Some(("String".into(), false)));
            assert_eq!(ret("(String) -> void"), Some(("void".into(), true)));
            assert_eq!(ret("(String) -> Integer | String"), None);
            assert_eq!(ret("(String) ->\n(Integer | String)"), Some(("Integer | String".into(), false)));
            assert_eq!(ret("(String) -> ^(Integer) -> void"), Some(("^(Integer) -> void".into(), false)));
            assert_eq!(ret("() { (?) -> untyped } -> String"), Some(("String".into(), false)));
            assert_eq!(ret("(\nString name\n) -> String"), Some(("String".into(), false)));
            assert_eq!(ret("[T] (T x, ?Integer, *untyped, k: Hash[Symbol, T], **untyped) ?{ -> void } -> T?"), Some(("T?".into(), false)));
            assert_eq!(ret("(String)"), None);
            assert_eq!(ret("-> (void)"), Some(("void".into(), true)));
        }
    }
}
