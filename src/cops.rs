//! Cop implementations. Each one mirrors the RuboCop 1.88 cop of the same name
//! under default config.

use regex::Regex;
use ruby_prism::{CallNode, InterpolatedStringNode, Node, RegularExpressionNode, StringNode, Visit, XStringNode};
use std::sync::LazyLock;

pub struct Cop {
    pub name: &'static str,
    pub severity: char,
    pub correctable: bool,
}

pub const COPS: &[Cop] = &[
    Cop { name: "Layout/EmptyLines", severity: 'C', correctable: true },
    Cop { name: "Layout/IndentationStyle", severity: 'C', correctable: true },
    Cop { name: "Layout/LineLength", severity: 'C', correctable: false },
    Cop { name: "Layout/TrailingEmptyLines", severity: 'C', correctable: true },
    Cop { name: "Layout/TrailingWhitespace", severity: 'C', correctable: true },
    Cop { name: "Lint/Debugger", severity: 'W', correctable: false },
    Cop { name: "Style/FrozenStringLiteralComment", severity: 'C', correctable: true },
    Cop { name: "Style/NilComparison", severity: 'C', correctable: true },
];

pub struct Offense {
    pub line: usize,
    pub col: usize,
    pub cop: &'static str,
    pub msg: String,
}

const LINE_MAX: usize = 120;
// Layout/IndentationWidth default; LineLength counts each indentation tab as this many columns.
const TAB_WIDTH: usize = 2;

const DEBUGGER_METHODS: &[&str] = &[
    "binding.irb",
    "Kernel.binding.irb",
    "byebug",
    "remote_byebug",
    "Kernel.byebug",
    "Kernel.remote_byebug",
    "page.save_and_open_page",
    "page.save_and_open_screenshot",
    "page.save_page",
    "page.save_screenshot",
    "save_and_open_page",
    "save_and_open_screenshot",
    "save_page",
    "save_screenshot",
    "binding.b",
    "binding.break",
    "Kernel.binding.b",
    "Kernel.binding.break",
    "binding.pry",
    "binding.remote_pry",
    "binding.pry_remote",
    "Kernel.binding.pry",
    "Kernel.binding.remote_pry",
    "Kernel.binding.pry_remote",
    "Pry.rescue",
    "pry",
    "debugger",
    "Kernel.debugger",
    "jard",
    "binding.console",
];
const DEBUGGER_REQUIRES: &[&str] = &["debug/open", "debug/start"];

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).unwrap()
}

static FROZEN_SIMPLE: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)\A\s*#\s*frozen[_-]string[_-]literal:\s*([[:alnum:]\-_]+)\s*\z"));
static EMACS: LazyLock<Regex> = LazyLock::new(|| re(r"-\*-(.+)-\*-"));
static EMACS_FROZEN: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)\Afrozen[_-]string[_-]literal\s*:\s*([[:alnum:]\-_]+)\z"));
// ponytail: approximates URI::RFC2396_PARSER.make_regexp(%w[http https]): a bare `http:` matches,
// and brackets are allowed in the query and fragment but end the path.
static URI: LazyLock<Regex> = LazyLock::new(|| {
    let uric = r"[\-_.!~*'()a-zA-Z\d;/?:@&=+$,\[\]%]";
    re(&format!(r"(?i)https?:[\-_.!~*'()a-zA-Z\d:@&=+$,;/%]*(?:\?{uric}*)?(?:#{uric}*)?"))
});
static QUALIFIED_NAME: LazyLock<Regex> = LazyLock::new(|| re(r"\b(?:[A-Z][A-Za-z0-9_]*::)+[A-Za-z_][A-Za-z0-9_]*\b"));
static BRACED_LINE: LazyLock<Regex> = LazyLock::new(|| re(r"\{[\s\S]*\}$"));
pub static DIRECTIVE: LazyLock<Regex> = LazyLock::new(|| {
    let name = r"(?:[A-Za-z]\w+/)*[A-Za-z]\w+";
    re(&format!(r"#\s*rubocop\s*:\s*(disable|enable|todo|push|pop)\b(?:\s*(all|(?:{name}\s*,\s*)*{name}))?"))
});

/// Source text plus line bookkeeping, in RuboCop's terms.
pub struct Src<'a> {
    pub text: &'a str,
    line_starts: Vec<usize>,
    /// Lines without newlines, stopping before `__END__` (ProcessedSource#lines).
    pub lines: Vec<&'a str>,
}

impl<'a> Src<'a> {
    pub fn new(text: &'a str) -> Self {
        let mut line_starts = vec![0];
        line_starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        Src { text, line_starts, lines: text.split('\n').collect() }
    }

    /// 1-based line containing byte offset.
    pub fn line_of(&self, off: usize) -> usize {
        self.line_starts.partition_point(|&s| s <= off)
    }

    /// 1-based (line, character column) of byte offset.
    pub fn pos(&self, off: usize) -> (usize, usize) {
        let line = self.line_of(off);
        (line, self.text[self.line_starts[line - 1]..off].chars().count() + 1)
    }
}

/// A Prism node classified by what the parser gem (which RuboCop cops are written against) would call it.
#[derive(Clone, Copy, PartialEq)]
enum Frame {
    Call,
    Block,
    KwBegin,
    Literal,
    Pair,
    /// Prism-only wrapper with no parser-gem counterpart.
    Transparent,
    Other,
}

fn frame(node: &Node) -> Frame {
    match node {
        Node::CallNode { .. } => Frame::Call,
        Node::BlockNode { .. } | Node::LambdaNode { .. } => Frame::Block,
        Node::BeginNode { .. } if node.as_begin_node().unwrap().begin_keyword_loc().is_some() => Frame::KwBegin,
        Node::ArrayNode { .. } | Node::HashNode { .. } | Node::KeywordHashNode { .. } | Node::RangeNode { .. } => {
            Frame::Literal
        }
        Node::AssocNode { .. } => Frame::Pair,
        Node::ProgramNode { .. } | Node::ElseNode { .. } => Frame::Transparent,
        // A single-statement body is the statement itself in parser; several become a `begin`.
        Node::StatementsNode { .. } if node.as_statements_node().unwrap().body().iter().count() == 1 => {
            Frame::Transparent
        }
        _ => Frame::Other,
    }
}

struct Ast<'s, 'a> {
    src: &'s Src<'a>,
    on: &'s dyn Fn(&str) -> bool,
    stack: Vec<Frame>,
    out: Vec<Offense>,
    /// Heredoc bodies as [first line, terminator line).
    heredocs: Vec<(usize, usize)>,
    /// Lines where the parser gem emits a string-content token (EmptyLines).
    string_lines: Vec<bool>,
    /// Byte ranges of delimited strings and heredoc bodies (IndentationStyle).
    string_ranges: Vec<(usize, usize)>,
}

impl Ast<'_, '_> {
    fn cover(&mut self, start: usize, end: usize) {
        if end > start {
            for l in self.src.line_of(start)..=self.src.line_of(end - 1) {
                self.string_lines[l] = true;
            }
        }
    }

    fn add(&mut self, off: usize, cop: &'static str, msg: String) {
        let (line, col) = self.src.pos(off);
        self.out.push(Offense { line, col, cop, msg });
    }

    fn string_like(&mut self, start: usize, end: usize, opening: Option<&[u8]>, body: Option<(usize, usize)>) {
        let Some(open) = opening else { return };
        if !open.starts_with(b"<<") {
            self.string_ranges.push((start, end));
        } else if let Some((b, t)) = body {
            self.heredocs.push((self.src.line_of(b), self.src.line_of(t)));
            self.string_ranges.push((b, t));
        }
    }

    /// Parser-gem ancestors of the node on top of the stack, nearest first.
    fn ancestors(&self) -> Vec<Frame> {
        (0..self.stack.len().saturating_sub(1))
            .rev()
            .filter(|&i| {
                let f = self.stack[i];
                // In parser `foo { x }` is (block (send foo) x), so the call is not x's ancestor.
                f != Frame::Transparent && !(f == Frame::Call && self.stack[i + 1] == Frame::Block)
            })
            .map(|i| self.stack[i])
            .collect()
    }

    fn debugger(&mut self, node: &CallNode, args: &[Node]) {
        let name = node.name().as_slice();
        let hit = if name == b"require" {
            args.len() == 1
                && args[0]
                    .as_string_node()
                    .is_some_and(|s| DEBUGGER_REQUIRES.iter().any(|r| r.as_bytes() == s.unescaped()))
        } else {
            DEBUGGER_METHODS.iter().any(|m| m.rsplit('.').next().unwrap().as_bytes() == name)
                && DEBUGGER_METHODS.contains(&chained_name(node).as_str())
        };
        if !hit {
            return;
        }
        // Lint/Debugger#assumed_usage_context?
        let anc = self.ancestors();
        if args.is_empty() && anc.contains(&Frame::Call) {
            if matches!(anc.first(), Some(Frame::Call | Frame::Literal | Frame::Pair)) {
                return;
            }
            if !anc.iter().any(|f| matches!(f, Frame::Block | Frame::KwBegin)) {
                return;
            }
        }
        let loc = node.location();
        let source = String::from_utf8_lossy(loc.as_slice());
        let msg = format!("Remove debugger entry point `{source}`.");
        self.add(loc.start_offset(), "Lint/Debugger", msg);
    }

    fn nil_comparison(&mut self, node: &CallNode, args: &[Node]) {
        let name = node.name().as_slice();
        if (name == b"==" || name == b"===")
            && node.receiver().is_some()
            && args.len() == 1
            && args[0].as_nil_node().is_some()
        {
            let sel = node.message_loc().unwrap();
            self.add(sel.start_offset(), "Style/NilComparison", "Prefer the use of the `nil?` predicate.".into());
        }
    }
}

/// Lint/Debugger#chained_method_name, e.g. `Kernel.binding.irb`.
fn chained_name(node: &CallNode) -> String {
    let mut name = String::from_utf8_lossy(node.name().as_slice()).into_owned();
    let mut recv = node.receiver();
    while let Some(r) = recv {
        let (part, next) = if let Some(c) = r.as_call_node() {
            (String::from_utf8_lossy(c.name().as_slice()).into_owned(), c.receiver())
        } else if r.as_constant_read_node().is_some() || r.as_constant_path_node().is_some() {
            let src = String::from_utf8_lossy(r.location().as_slice());
            (src.trim_start_matches("::").to_string(), None)
        } else {
            (String::new(), None)
        };
        name = format!("{part}.{name}");
        recv = next;
    }
    name
}

impl<'pr> Visit<'pr> for Ast<'_, '_> {
    fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
        self.stack.push(frame(&node));
    }

    fn visit_branch_node_leave(&mut self) {
        self.stack.pop();
    }

    fn visit_call_node(&mut self, node: &CallNode<'pr>) {
        // Both cops hook on_send only, so `&.` calls (csend) are skipped.
        if !node.call_operator_loc().is_some_and(|l| l.as_slice() == b"&.") {
            let args: Vec<Node> = node.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default();
            if (self.on)("Lint/Debugger") {
                self.debugger(node, &args);
            }
            if (self.on)("Style/NilComparison") {
                self.nil_comparison(node, &args);
            }
        }
        ruby_prism::visit_call_node(self, node);
    }

    fn visit_string_node(&mut self, node: &StringNode<'pr>) {
        let c = node.content_loc();
        self.cover(c.start_offset(), c.end_offset());
        let loc = node.location();
        let body = node.closing_loc().map(|t| (c.start_offset(), t.start_offset()));
        self.string_like(loc.start_offset(), loc.end_offset(), node.opening_loc().map(|o| o.as_slice()), body);
    }

    fn visit_interpolated_string_node(&mut self, node: &InterpolatedStringNode<'pr>) {
        let loc = node.location();
        let body = match (node.parts().first(), node.closing_loc()) {
            (Some(p), Some(t)) => Some((p.location().start_offset(), t.start_offset())),
            _ => None,
        };
        self.string_like(loc.start_offset(), loc.end_offset(), node.opening_loc().map(|o| o.as_slice()), body);
        ruby_prism::visit_interpolated_string_node(self, node);
    }

    fn visit_x_string_node(&mut self, node: &XStringNode<'pr>) {
        let c = node.content_loc();
        self.cover(c.start_offset(), c.end_offset());
        if node.opening_loc().as_slice().starts_with(b"<<") {
            let t = node.closing_loc().start_offset();
            self.heredocs.push((self.src.line_of(c.start_offset()), self.src.line_of(t)));
        }
    }

    fn visit_regular_expression_node(&mut self, node: &RegularExpressionNode<'pr>) {
        let c = node.content_loc();
        self.cover(c.start_offset(), c.end_offset());
    }
}

/// Run every enabled cop on one file. `None` means the file has syntax errors.
pub fn check(text: &str, on: &dyn Fn(&str) -> bool) -> Option<Vec<Offense>> {
    let result = ruby_prism::parse(text.as_bytes());
    // Prism also reports semantic errors the parser gem accepts, e.g. top-level `yield` in .builder templates.
    // ponytail: only verified ones are tolerated; add others as real code turns them up.
    if result.errors().any(|e| e.message() != "Invalid yield") {
        return None;
    }
    let mut src = Src::new(text);
    if let Some(data) = result.data_loc() {
        src.lines.truncate(src.line_of(data.start_offset()) - 1);
    }
    let comments: Vec<(usize, usize)> =
        result.comments().map(|c| (c.location().start_offset(), c.location().end_offset())).collect();
    let program = result.node();
    let first_code =
        program.as_program_node().unwrap().statements().body().iter().next().map(|n| n.location().start_offset());
    let no_tokens = first_code.is_none() && comments.is_empty();

    let mut ast = Ast {
        src: &src,
        on,
        stack: Vec::new(),
        out: Vec::new(),
        heredocs: Vec::new(),
        string_lines: vec![false; src.line_starts.len() + 2],
        string_ranges: Vec::new(),
    };
    ast.visit(&program);
    let Ast { mut out, heredocs, string_lines, string_ranges, .. } = ast;
    let mut add = |line, col, cop, msg: &str| out.push(Offense { line, col, cop, msg: msg.to_string() });

    if on("Style/FrozenStringLiteralComment") && !no_tokens {
        let leading = match first_code {
            Some(off) => &src.lines[..(src.line_of(off) - 1).min(src.lines.len())],
            None => &src.lines[..],
        };
        if !leading.iter().any(|l| frozen_value(l).is_some_and(|v| v == "true" || v == "false")) {
            add(1, 1, "Style/FrozenStringLiteralComment", "Missing frozen string literal comment.");
        }
    }

    if on("Layout/TrailingWhitespace") {
        for (i, line) in src.lines.iter().enumerate() {
            let trimmed = line.trim_end_matches(is_blank);
            if trimmed.len() != line.len() {
                add(i + 1, trimmed.chars().count() + 1, "Layout/TrailingWhitespace", "Trailing whitespace detected.");
            }
        }
    }

    if on("Layout/IndentationStyle") {
        for (i, line) in src.lines.iter().enumerate() {
            // /\A\s*\t+/
            let ws = line.len() - line.trim_start_matches(|c: char| c.is_ascii_whitespace()).len();
            let Some(end) = line[..ws].rfind('\t') else {
                continue;
            };
            let (a, b) = (src.line_starts[i], src.line_starts[i] + end + 1);
            if !string_ranges.iter().any(|&(s, e)| s <= a && b <= e) {
                add(i + 1, 1, "Layout/IndentationStyle", "Tab detected in indentation.");
            }
        }
    }

    if on("Layout/EmptyLines") && !no_tokens && text.contains("\n\n\n") {
        // A blank line is extra when the line before it is blank too and the parser
        // gem emits no token on it (string content gets one token per line).
        let last_code = src.lines.iter().rposition(|l| !l.trim().is_empty()).map_or(0, |i| i + 1);
        let last_comment = comments.last().map_or(0, |&(_, e)| src.line_of(e - 1));
        let last_token_line = last_code.max(last_comment);
        for (i, pair) in src.lines.windows(2).enumerate().take(last_token_line.saturating_sub(2)) {
            let line = i + 2;
            if pair[0].is_empty() && pair[1].is_empty() && !string_lines[line] {
                add(line, 1, "Layout/EmptyLines", "Extra blank line detected.");
            }
        }
    }

    if on("Layout/LineLength") {
        let in_heredoc = |line: usize| heredocs.iter().any(|&(a, b)| a <= line && line < b);
        let has_directive =
            |line: usize| comments.iter().any(|&(s, e)| src.line_of(s) == line && directive(&text[s..e]).is_some());
        for (i, line) in src.lines.iter().enumerate() {
            let len = line_length(line);
            if len <= LINE_MAX || (i == 0 && line.starts_with("#!")) || in_heredoc(i + 1) {
                continue;
            }
            if has_directive(i + 1) {
                // AllowCopDirectives: measure the line without the directive.
                let before = DIRECTIVE.split(line).next().unwrap_or("").trim_end().chars().count();
                if before > LINE_MAX {
                    add(i + 1, LINE_MAX + 1, "Layout/LineLength", &format!("Line is too long. [{before}/{LINE_MAX}]"));
                }
                continue;
            }
            let uri = excessive_range(line, &URI);
            let qn = excessive_range(line, &QUALIFIED_NAME);
            let allowed = |r: (usize, usize)| r.0 < LINE_MAX && r.1 == len;
            let ok = match (uri, qn) {
                (Some(u), Some(q)) => allowed(u) && allowed(q),
                (Some(r), None) | (None, Some(r)) => allowed(r),
                (None, None) => false,
            };
            if ok {
                continue;
            }
            let start = match uri.or(qn) {
                Some((b, e)) if b < LINE_MAX => e,
                _ => LINE_MAX.saturating_sub(indentation_difference(line)),
            };
            add(i + 1, start + 1, "Layout/LineLength", &format!("Line is too long. [{len}/{LINE_MAX}]"));
        }
    }

    // TrailingEmptyLines skips any file matching /\s*__END__/ or ending in "%\n\n".
    if on("Layout/TrailingEmptyLines") && !text.is_empty() && !text.contains("__END__") && !text.ends_with("%\n\n") {
        let ws_len = text.len() - text.trim_end_matches(|c: char| c.is_ascii_whitespace()).len();
        let ws = &text[text.len() - ws_len..];
        let blank = ws.matches('\n').count() as i64 - 1;
        if blank != 0 {
            let (line, col) = src.pos(text.len() - ws_len + usize::from(!ws.is_empty()));
            let msg = match blank {
                -1 => "Final newline missing.".to_string(),
                n => format!("{n} trailing blank lines detected."),
            };
            add(line, col, "Layout/TrailingEmptyLines", &msg);
        }
    }

    if text.contains("rubocop") {
        let off = disabled_lines(&src, &comments);
        out.retain(|o| !off.iter().any(|&(cop, a, b)| cop == o.cop && a <= o.line && o.line <= b));
    }
    Some(out)
}

/// CommentConfig: (cop, first line, last line) spans switched off by directive comments.
// ponytail: push/pop directives are ignored; add them if a real codebase needs them.
fn disabled_lines(src: &Src, comments: &[(usize, usize)]) -> Vec<(&'static str, usize, usize)> {
    let mut spans = Vec::new();
    let mut open: Vec<(&'static str, usize)> = Vec::new();
    for &(s, e) in comments {
        let comment = &src.text[s..e];
        let Some((mode, Some(names))) = directive(comment) else {
            continue;
        };
        if mode == "push" || mode == "pop" {
            continue;
        }
        let line = src.line_of(s);
        let disable = mode == "disable" || mode == "todo";
        let comment_only = src.text[src.line_starts[line - 1]..s].chars().all(char::is_whitespace);
        let single_line = DIRECTIVE.find(comment).is_some_and(|m| m.start() != 0);
        for cop in expand(&names) {
            if !comment_only || single_line {
                if disable {
                    spans.push((cop, line, line));
                }
                continue;
            }
            if let Some(i) = open.iter().position(|&(c, _)| c == cop) {
                let (_, start) = open.remove(i);
                spans.push((cop, start, line));
            }
            if disable {
                open.push((cop, line));
            }
        }
    }
    spans.extend(open.into_iter().map(|(cop, start)| (cop, start, usize::MAX)));
    spans
}

/// `all`, departments and unqualified names to the cops we implement.
fn expand(names: &str) -> Vec<&'static str> {
    if names == "all" {
        return COPS.iter().map(|c| c.name).collect();
    }
    names
        .split(',')
        .map(str::trim)
        .flat_map(|n| {
            COPS.iter().map(|c| c.name).filter(move |c| {
                *c == n || c.strip_prefix(n).is_some_and(|r| r.starts_with('/')) || c.split('/').nth(1) == Some(n)
            })
        })
        .collect()
}

/// [[:blank:]] in Ruby: tab plus Unicode space separators.
fn is_blank(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{202f}' | '\u{205f}' | '\u{3000}')
}

/// Value of a `frozen_string_literal` magic comment, lowercased.
fn frozen_value(line: &str) -> Option<String> {
    match EMACS.captures(line) {
        Some(e) => e[1].split(';').find_map(|t| EMACS_FROZEN.captures(t.trim()).map(|c| c[1].to_lowercase())),
        None => FROZEN_SIMPLE.captures(line).map(|c| c[1].to_lowercase()),
    }
}

/// Mode and cop list of a `# rubocop:disable ...` comment, unless it is commented out (`## rubocop:`).
pub fn directive(comment: &str) -> Option<(String, Option<String>)> {
    let m = DIRECTIVE.captures(comment)?;
    let pre = &comment[..m.get(0).unwrap().start()];
    if pre.starts_with('#') && pre[1..].trim().is_empty() {
        return None;
    }
    Some((m[1].to_string(), m.get(2).map(|c| c.as_str().to_string())))
}

fn indentation_difference(line: &str) -> usize {
    line.bytes().take_while(|&b| b == b'\t').count() * (TAB_WIDTH - 1)
}

fn line_length(line: &str) -> usize {
    line.chars().count() + indentation_difference(line)
}

/// Would `URI.parse` (RFC 3986) accept this? Brackets are only legal around an IPv6 host,
/// `#` may appear once and `%` must start an escape.
// ponytail: covers the failures seen in real code, not the whole RFC 3986 grammar.
fn valid_uri(s: &str) -> bool {
    let b = s.as_bytes();
    !s.contains(['[', ']'])
        && s.matches('#').count() <= 1
        && b.iter()
            .enumerate()
            .all(|(i, &c)| c != b'%' || b.get(i + 1..i + 3).is_some_and(|h| h.iter().all(u8::is_ascii_hexdigit)))
}

/// LineLengthHelp#find_excessive_range, in character columns.
fn excessive_range(line: &str, pattern: &Regex) -> Option<(usize, usize)> {
    let is_uri = std::ptr::eq(pattern, &*URI);
    let m = pattern.find_iter(line).filter(|m| !is_uri || valid_uri(m.as_str())).last()?;
    let mut end = m.end();
    if BRACED_LINE.is_match(line)
        && let Some(close) = line[end..].rfind('}')
    {
        end += close + 1;
    }
    end += line[end..].find(char::is_whitespace).unwrap_or(line.len() - end);
    let diff = indentation_difference(line);
    let col = |b: usize| line[..b].chars().count() + diff;
    let (b, e) = (col(m.start()), col(end));
    (b >= LINE_MAX || e >= LINE_MAX).then_some((b, e))
}
