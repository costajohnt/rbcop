use crate::helpers::comment_association;
use crate::helpers::gem_declaration::{is_gem_declaration, str_value};
use crate::prelude::*;
use crate::source::Source;
use ruby_prism::{CallNode, Visit};

pub const NAME: &str = "Bundler/GemComment";
pub static COP: GemComment = GemComment;
pub struct GemComment;

static RESTRICTIVE_VERSION: LazyLock<regex::bytes::Regex> =
    LazyLock::new(|| regex::bytes::Regex::new(r"(?-u)\A\s*(?:<|~>|\d|=)").unwrap());

/// Parser's `Comment::Associator` run over the gem `send` and its descendants: the comments each
/// node gets, as (node first line, comment indices).
struct Associate<'a> {
    src: &'a Source<'a>,
    cursor: usize,
    stack: Vec<Option<usize>>,
    /// (start, end, comments) per parser node.
    nodes: Vec<(usize, usize, Vec<usize>)>,
}

impl Associate<'_> {
    fn comment(&self) -> Option<crate::source::Comment> {
        self.src.comments.get(self.cursor).copied()
    }

    fn leading(&mut self, i: usize) {
        while self.comment().is_some_and(|c| c.end <= self.nodes[i].0) {
            self.nodes[i].2.push(self.cursor);
            self.cursor += 1;
        }
    }

    fn trailing(&mut self, i: usize) {
        while self.comment().is_some_and(|c| c.end <= self.nodes[i].1) {
            self.nodes[i].2.push(self.cursor);
            self.cursor += 1;
        }
        let last_line = self.src.line_of(self.nodes[i].1);
        while self.comment().is_some_and(|c| self.src.line_of(c.start) == last_line) {
            self.nodes[i].2.push(self.cursor);
            self.cursor += 1;
        }
    }

    fn enter(&mut self, node: &Node) {
        // Wrappers parser has no node for; a `begin` (parentheses) gets no leading comments.
        let k = kind::of(node);
        if matches!(k, kind::ARGUMENTS_NODE | kind::STATEMENTS_NODE) {
            self.stack.push(None);
            return;
        }
        let loc = node.location();
        self.nodes.push((loc.start_offset(), loc.end_offset(), Vec::new()));
        let i = self.nodes.len() - 1;
        if k != kind::PARENTHESES_NODE {
            self.leading(i);
        }
        self.stack.push(Some(i));
    }

    fn leave(&mut self) {
        if let Some(Some(i)) = self.stack.pop() {
            self.trailing(i);
        }
    }

    /// `commented?(node)`: the last associated comment up to the node's first line precedes it
    /// by at most one line and is a `#` comment.
    fn commented(&self, i: usize) -> bool {
        let (start, _, ref comments) = self.nodes[i];
        let line = self.src.line_of(start);
        comments.iter().map(|&c| self.src.comments[c]).filter(|c| self.src.line_of(c.start) <= line).last().is_some_and(
            |c| {
                line - self.src.line_of(c.start) <= 1
                    && self.src.slice(c.start, c.end).trim_start_matches([' ', '\t', '\n', '\x0b', '\x0c', '\r']).starts_with('#')
            },
        )
    }
}

impl<'pr> Visit<'pr> for Associate<'_> {
    fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
        self.enter(&node);
    }
    fn visit_branch_node_leave(&mut self) {
        self.leave();
    }
    fn visit_leaf_node_enter(&mut self, node: Node<'pr>) {
        self.enter(&node);
    }
    fn visit_leaf_node_leave(&mut self) {
        self.leave();
    }
}

/// `commented_any_descendant?`
fn commented_any_descendant(src: &Source, call: &CallNode, ancestors: &[Node]) -> bool {
    let (s, e) = send_range(call);
    // Leading comments go to the outermost parser node starting here: a `block` around the send,
    // a modifier `if`, a chained call, ... (a `begin` takes none).
    let outer = call.block().is_some_and(|b| b.as_block_node().is_some())
        || ancestors.iter().any(|a| {
            a.location().start_offset() == s
                && !matches!(kind::of(a), kind::PROGRAM_NODE | kind::STATEMENTS_NODE | kind::PARENTHESES_NODE)
        });
    let mut v = Associate { src, cursor: 0, stack: Vec::new(), nodes: vec![(s, e, Vec::new())] };
    // End of the code before the node: skip back over whitespace and comments.
    let mut prev = s;
    loop {
        let trimmed = src.text[..prev].trim_end().len();
        match src.comments.iter().find(|c| c.start < trimmed && c.end >= trimmed && c.end <= prev) {
            Some(c) => prev = c.start,
            None => {
                prev = trimmed;
                break;
            }
        }
    }
    // A comment on the previous code's last line decorates that code, unless that line is a
    // bare `begin`/`else`/`ensure`/`rescue` keyword (no node ends there).
    // ponytail: other node-less line ends (`then`, `(`) are treated as code.
    let after_line = (prev > 0)
        .then(|| src.line_of(prev))
        .filter(|&l| !matches!(src.text[src.line_start(l)..prev].trim(), "begin" | "else" | "ensure" | "rescue"));
    if !outer {
        for c in comment_association::leading(src, s, prev, after_line) {
            let idx = src.comments.iter().position(|x| x.start == c.start).unwrap();
            v.nodes[0].2.push(idx);
        }
    }
    v.cursor = src.comments.partition_point(|c| c.start < s || Some(src.line_of(c.start)) == after_line);
    v.stack.push(None);
    if let Some(args) = call.arguments() {
        v.visit_arguments_node(&args);
    }
    if let Some(b) = call.block().filter(|b| b.as_block_argument_node().is_some()) {
        v.visit(&b);
    }
    v.stack.pop();
    v.trailing(0);
    (0..v.nodes.len()).any(|i| v.commented(i))
}

impl Cop for GemComment {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if !is_gem_declaration(&call) {
            return;
        }
        let args = send_args(&call);
        let name = str_value(&args[0]).unwrap();
        if cx.cfg.strings("AllowedGems").iter().any(|g| g.as_bytes() == name.as_slice()) {
            return;
        }
        if commented_any_descendant(cx.src, &call, cx.ancestors) {
            return;
        }
        let only_for = cx.cfg.strings("OnlyFor");
        if !only_for.is_empty() && !checked_options_present(&only_for, &args) {
            return;
        }
        let (s, e) = send_range(&call);
        cx.add(s, e, "Missing gem description comment.");
    }
}

fn checked_options_present(only_for: &[String], args: &[Node]) -> bool {
    let has = |o: &str| only_for.iter().any(|x| x == o);
    // `node.arguments[1]&.str_type?`
    let version_specified = args.get(1).is_some_and(|a| str_value(a).is_some());
    if has("version_specifiers") && version_specified {
        return true;
    }
    if has("restrictive_version_specifiers")
        && version_specified
        && args[1..].iter().any(|a| str_value(a).is_some_and(|v| RESTRICTIVE_VERSION.is_match(&v)))
    {
        return true;
    }
    // gem_options: literal sym/str keys of a trailing hash.
    let Some(last) = args.last() else { return false };
    let elements = match (last.as_hash_node(), last.as_keyword_hash_node()) {
        (Some(h), _) => h.elements(),
        (_, Some(h)) => h.elements(),
        _ => return false,
    };
    elements.iter().filter_map(|e| e.as_assoc_node()).any(|p| {
        let key = p.key();
        let value = match (key.as_symbol_node(), str_value(&key)) {
            (Some(s), _) => s.unescaped().to_vec(),
            (_, Some(v)) => v,
            _ => return false,
        };
        only_for.iter().any(|o| o.as_bytes() == value.as_slice())
    })
}
