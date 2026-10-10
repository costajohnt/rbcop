//! A scanner for regexp literal source that mirrors the parts of the regexp_parser gem (2.12)
//! RuboCop's `RegexpNode#parsed_tree` cops look at: escape expressions, character sets with
//! their members, free-spacing comments (`/x`, `(?x)`, `(?x:...)`), `(?#...)` comments, and the
//! main errors that make `parsed_tree` nil (unbalanced groups or sets, trailing backslash, bad
//! `\x`, unknown POSIX classes, references to missing groups).
//!
//! ponytail: unknown `\p{...}` property names are not rejected (regexp_parser rejects some names
//! Onigmo accepts, e.g. `\p{InGreek}`); add its property table if a corpus needs it.
//!
//! Offsets are byte offsets into the pattern.

/// What a set member is, as far as the cops care.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MemberKind {
    /// literal character
    Literal,
    /// `escape` expression (`\.`, `\n`, `\x41`, `\u{..}`, octal, control, meta)
    Escape,
    /// character type (`\d`, `\h`, ...)
    Type,
    /// `\p{..}` / `\P{..}`
    Property,
    /// nested set, range or intersection (type `:set`)
    Set,
    /// `[:alpha:]`
    PosixClass,
    /// `[:^alpha:]`
    NonPosixClass,
}

#[derive(Clone, Copy, Debug)]
pub struct Member {
    pub kind: MemberKind,
    pub start: usize,
    pub end: usize,
    /// `\u{61 62}`: two or more codepoints.
    pub multiple_codepoints: bool,
}

#[derive(Debug)]
pub struct Set {
    pub start: usize,
    /// Just past the closing `]` (quantifier excluded).
    pub end: usize,
    pub negative: bool,
    pub members: Vec<Member>,
}

#[derive(Clone, Copy, Debug)]
pub struct Escape {
    pub start: usize,
    pub end: usize,
    /// Inside a character set (any depth).
    pub in_set: bool,
}

#[derive(Debug, Default)]
pub struct Parsed {
    /// Expressions of type `:escape`, in source order.
    pub escapes: Vec<Escape>,
    /// Every character set (nested ones included), in `each_expression` order (outer first).
    pub sets: Vec<Set>,
}

struct Scanner<'a> {
    s: &'a [u8],
    i: usize,
    out: Parsed,
    captures: usize,
    names: Vec<&'a [u8]>,
    num_refs: Vec<usize>,
    name_refs: Vec<&'a [u8]>,
}

const POSIX_CLASSES: &[&[u8]] = &[
    b"alnum", b"alpha", b"ascii", b"blank", b"cntrl", b"digit", b"graph", b"lower", b"print", b"punct", b"space",
    b"upper", b"word", b"xdigit",
];

/// Parse a pattern (interpolations already blanked) with the `x` option as given.
pub fn parse(pattern: &str, extended: bool) -> Option<Parsed> {
    let mut sc = Scanner {
        s: pattern.as_bytes(),
        i: 0,
        out: Parsed::default(),
        captures: 0,
        names: Vec::new(),
        num_refs: Vec::new(),
        name_refs: Vec::new(),
    };
    sc.sequence(extended, 0)?;
    if sc.i < sc.s.len() {
        return None; // unmatched `)`
    }
    if sc.num_refs.iter().any(|&n| n == 0 || n > sc.captures) || sc.name_refs.iter().any(|n| !sc.names.contains(n)) {
        return None;
    }
    Some(sc.out)
}

fn char_len(b: u8) -> usize {
    match b {
        0xf0..=0xff => 4,
        0xe0..=0xef => 3,
        0xc0..=0xdf => 2,
        _ => 1,
    }
}

impl<'a> Scanner<'a> {
    fn peek(&self, k: usize) -> Option<u8> {
        self.s.get(self.i + k).copied()
    }

    /// Advance over one (UTF-8) character.
    fn bump(&mut self) {
        let n = char_len(self.s[self.i]);
        self.i = (self.i + n).min(self.s.len());
    }

    /// A group body (or the whole pattern at depth 0); stops at its `)` (not consumed).
    fn sequence(&mut self, mut extended: bool, depth: usize) -> Option<()> {
        while let Some(c) = self.peek(0) {
            match c {
                b'\\' => {
                    let start = self.i;
                    if self.escape(false)? {
                        self.out.escapes.push(Escape { start, end: start + 2, in_set: false });
                    }
                }
                b'[' => {
                    self.set()?;
                }
                b'(' => {
                    if let Some(x) = self.group_open(extended)? {
                        match x {
                            Group::Switch(x) => extended = x,
                            Group::Comment => {}
                            Group::Open(x) => {
                                self.sequence(x, depth + 1)?;
                                if self.peek(0) != Some(b')') {
                                    return None;
                                }
                                self.i += 1;
                            }
                        }
                    }
                }
                b')' => {
                    return if depth > 0 { Some(()) } else { None };
                }
                b'#' if extended => {
                    while let Some(c) = self.peek(0) {
                        self.i += 1;
                        if c == b'\n' {
                            break;
                        }
                    }
                }
                _ => self.bump(),
            }
        }
        if depth > 0 { None } else { Some(()) }
    }

    /// `(` at self.i: consumes the opening (`(`, `(?:`, `(?<n>`, `(?x)`, `(?#...)`, ...).
    fn group_open(&mut self, extended: bool) -> Option<Option<Group>> {
        let s = self.s;
        self.i += 1;
        if self.peek(0) != Some(b'?') {
            self.captures += 1;
            return Some(Some(Group::Open(extended)));
        }
        self.i += 1;
        match self.peek(0) {
            Some(b'#') => {
                while let Some(c) = self.peek(0) {
                    self.i += 1;
                    if c == b')' {
                        return Some(Some(Group::Comment));
                    }
                }
                None
            }
            Some(b':' | b'=' | b'!' | b'>' | b'~') => {
                self.i += 1;
                Some(Some(Group::Open(extended)))
            }
            Some(b'<') if matches!(self.peek(1), Some(b'=' | b'!')) => {
                self.i += 2;
                Some(Some(Group::Open(extended)))
            }
            Some(q @ (b'<' | b'\'')) => {
                let close = if q == b'<' { b'>' } else { b'\'' };
                let start = self.i + 1;
                let end = start + s[start..].iter().position(|&c| c == close)?;
                if end == start {
                    return None;
                }
                self.names.push(&s[start..end]);
                self.captures += 1;
                self.i = end + 1;
                Some(Some(Group::Open(extended)))
            }
            Some(b'(') => {
                // Conditional `(?(cond)yes|no)`.
                let end = self.i + s[self.i..].iter().position(|&c| c == b')')?;
                self.i = end + 1;
                Some(Some(Group::Open(extended)))
            }
            _ => {
                // Options: `(?imx-imx)` or `(?imx-imx:`.
                let mut on = true;
                let mut x = extended;
                loop {
                    match self.peek(0)? {
                        b'-' => on = false,
                        b'x' => x = on,
                        b'i' | b'm' | b'a' | b'd' | b'u' | b'l' => {}
                        b')' => {
                            self.i += 1;
                            return Some(Some(Group::Switch(x)));
                        }
                        b':' => {
                            self.i += 1;
                            return Some(Some(Group::Open(x)));
                        }
                        _ => return None,
                    }
                    self.i += 1;
                }
            }
        }
    }

    /// `\` at self.i: consumes the escape sequence; true when it is an `:escape` expression whose
    /// second character matters (anything not alphanumeric is one).
    fn escape(&mut self, in_set: bool) -> Option<bool> {
        let s = self.s;
        let c = *s.get(self.i + 1)?;
        self.i += 1;
        match c {
            b'c' => {
                self.i += 1;
                self.control_target()?;
            }
            b'C' | b'M' if self.peek(1) == Some(b'-') => {
                self.i += 2;
                self.control_target()?;
            }
            b'x' => {
                self.i += 1;
                let n = s[self.i..].iter().take(2).take_while(|c| c.is_ascii_hexdigit()).count();
                if n == 0 {
                    return None;
                }
                self.i += n;
            }
            b'u' => {
                self.i += 1;
                if self.peek(0) == Some(b'{') {
                    let end = self.i + s[self.i..].iter().position(|&c| c == b'}')?;
                    let body = &s[self.i + 1..end];
                    if body.is_empty() || !body.iter().all(|c| c.is_ascii_hexdigit() || *c == b' ') {
                        return None;
                    }
                    self.i = end + 1;
                } else {
                    let n = s[self.i..].iter().take(4).take_while(|c| c.is_ascii_hexdigit()).count();
                    if n < 4 {
                        return None;
                    }
                    self.i += 4;
                }
            }
            b'0' => {
                self.i += 1;
                self.i += s[self.i..].iter().take(2).take_while(|c| (b'0'..=b'7').contains(c)).count();
            }
            b'1'..=b'9' => {
                let start = self.i;
                let n = s[start..].iter().take_while(|c| c.is_ascii_digit()).count();
                if in_set {
                    // octal escape: `\1`, `\12`, `\123`
                    self.i += s[start..].iter().take(3).take_while(|c| (b'0'..=b'7').contains(c)).count().max(1);
                } else {
                    let num: usize = std::str::from_utf8(&s[start..start + n]).ok()?.parse().unwrap_or(usize::MAX);
                    if n > 1 && num > self.captures_total() {
                        // `\10` without ten groups is an octal escape.
                        self.i += s[start..].iter().take(3).take_while(|c| (b'0'..=b'7').contains(c)).count().max(1);
                    } else {
                        self.num_refs.push(num);
                        self.i += n;
                    }
                }
            }
            b'k' | b'g' if !in_set && matches!(self.peek(1), Some(b'<' | b'\'')) => {
                let close = if self.peek(1) == Some(b'<') { b'>' } else { b'\'' };
                let start = self.i + 2;
                let end = start + s[start..].iter().position(|&c| c == close)?;
                let r = &s[start..end];
                let digits = r.strip_prefix(b"-").or(r.strip_prefix(b"+")).unwrap_or(r);
                if !digits.is_empty() && digits.iter().all(|c| c.is_ascii_digit()) {
                    if r[0].is_ascii_digit() {
                        self.num_refs.push(std::str::from_utf8(r).ok()?.parse().ok()?);
                    }
                } else if c == b'k' {
                    // `\k<name+level>` keeps the name part.
                    let name_end = r.iter().position(|&c| c == b'+' || c == b'-').unwrap_or(r.len());
                    self.name_refs.push(&r[..name_end]);
                } else {
                    self.name_refs.push(r);
                }
                self.i = end + 1;
            }
            b'p' | b'P' if self.peek(1) == Some(b'{') => {
                let end = self.i + s[self.i..].iter().position(|&c| c == b'}')?;
                self.i = end + 1;
            }
            c if c.is_ascii_alphanumeric() => self.i += 1,
            _ => {
                self.bump();
                return Some(true);
            }
        }
        Some(false)
    }

    /// Captures in the whole pattern (for the `\10` octal-or-backref decision).
    fn captures_total(&self) -> usize {
        let mut n = 0;
        let s = self.s;
        let mut i = 0;
        let mut in_set = 0;
        while i < s.len() {
            match s[i] {
                b'\\' => i += 1,
                b'[' => in_set += 1,
                b']' if in_set > 0 => in_set -= 1,
                b'(' if in_set == 0 => {
                    if s.get(i + 1) != Some(&b'?') {
                        n += 1;
                    } else if matches!(s.get(i + 2), Some(b'<' | b'\'')) && !matches!(s.get(i + 3), Some(b'=' | b'!')) {
                        n += 1;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        n
    }

    /// The character after `\c`, `\C-`, `\M-`: one character or a further escape.
    fn control_target(&mut self) -> Option<()> {
        match self.peek(0)? {
            b'\\' => {
                self.escape(false)?;
            }
            _ => self.bump(),
        }
        Some(())
    }

    /// A character set at self.i (`[`); records it and its escapes and nested sets.
    fn set(&mut self) -> Option<()> {
        let s = self.s;
        let start = self.i;
        self.i += 1;
        let negative = self.peek(0) == Some(b'^');
        if negative {
            self.i += 1;
        }
        let idx = self.out.sets.len();
        self.out.sets.push(Set { start, end: 0, negative, members: Vec::new() });
        let mut members: Vec<Member> = Vec::new();
        let mut intersection = false;
        loop {
            let c = self.peek(0)?;
            let m_start = self.i;
            let kind = match c {
                b']' => {
                    self.i += 1;
                    break;
                }
                b'&' if self.peek(1) == Some(b'&') => {
                    self.i += 2;
                    intersection = true;
                    continue;
                }
                b'[' if self.peek(1) == Some(b':') => {
                    let neg = self.peek(2) == Some(b'^');
                    let name_start = self.i + if neg { 3 } else { 2 };
                    let end = name_start + s[name_start..].windows(2).position(|w| w == b":]")?;
                    if !POSIX_CLASSES.contains(&&s[name_start..end]) {
                        return None;
                    }
                    self.i = end + 2;
                    if neg { MemberKind::NonPosixClass } else { MemberKind::PosixClass }
                }
                b'[' => {
                    self.set()?;
                    MemberKind::Set
                }
                b'\\' => {
                    let e_start = self.i;
                    let next = self.peek(1)?;
                    let is_escape = self.escape(true)?;
                    if is_escape {
                        self.out.escapes.push(Escape { start: e_start, end: e_start + 2, in_set: true });
                    }
                    match next {
                        b'd' | b'D' | b'h' | b'H' | b's' | b'S' | b'w' | b'W' => MemberKind::Type,
                        b'p' | b'P' if s.get(e_start + 2) == Some(&b'{') => MemberKind::Property,
                        _ => MemberKind::Escape,
                    }
                }
                _ => {
                    self.bump();
                    MemberKind::Literal
                }
            };
            let mut member = Member { kind, start: m_start, end: self.i, multiple_codepoints: false };
            if kind == MemberKind::Escape && s[m_start..self.i].starts_with(b"\\u{") {
                let body = &s[m_start + 3..self.i - 1];
                member.multiple_codepoints = body.split(|&c| c == b' ').filter(|p| !p.is_empty()).count() >= 2;
            }
            // A range: `a-z` (not before the closing bracket, not after a nested set).
            if kind != MemberKind::Set && self.peek(0) == Some(b'-') && self.peek(1).is_some_and(|c| c != b']') {
                let prev_end = self.i;
                self.i += 1;
                if !self.range_end()? {
                    self.i = prev_end;
                } else {
                    member = Member { kind: MemberKind::Set, start: m_start, end: self.i, multiple_codepoints: false };
                    // `a-b-c`: chained ranges stay one member.
                    while self.peek(0) == Some(b'-') && self.peek(1).is_some_and(|c| c != b']') {
                        let save = self.i;
                        self.i += 1;
                        if !self.range_end()? {
                            self.i = save;
                            break;
                        }
                        member.end = self.i;
                    }
                }
            }
            members.push(member);
        }
        if intersection {
            members = vec![Member { kind: MemberKind::Set, start: start + 1, end: self.i - 1, multiple_codepoints: false }];
        }
        let set = &mut self.out.sets[idx];
        set.end = self.i;
        set.members = members;
        Some(())
    }

    /// The end of a range after `-`: a literal or an escape. False if it is not one.
    fn range_end(&mut self) -> Option<bool> {
        match self.peek(0)? {
            b'[' | b']' => Some(false),
            b'\\' => {
                let e_start = self.i;
                if self.escape(true)? {
                    self.out.escapes.push(Escape { start: e_start, end: e_start + 2, in_set: true });
                }
                Some(true)
            }
            _ => {
                self.bump();
                Some(true)
            }
        }
    }
}

enum Group {
    /// `(?x)`-style switch: the new free-spacing state for the rest of the group.
    Switch(bool),
    Comment,
    /// A group whose body is scanned with this free-spacing state.
    Open(bool),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_and_sets() {
        let p = parse(r"a\.[\-x](?#\.)\c\.", false).unwrap();
        let e: Vec<_> = p.escapes.iter().map(|e| (e.start, e.in_set)).collect();
        assert_eq!(e, vec![(1, false), (4, true)]);
        assert_eq!(p.sets.len(), 1);
        assert_eq!(p.sets[0].members.len(), 2);
        assert!(parse("(a", false).is_none());
        assert!(parse("a)", false).is_none());
        assert!(parse("[a", false).is_none());
        assert!(parse(r"(x)\2", false).is_none());
        let x = parse("a # \\.\n\\.", true).unwrap();
        assert_eq!(x.escapes.len(), 1);
        let r = parse("[a-z][[:alpha:]][a&&b]", false).unwrap();
        assert!(r.sets.iter().all(|s| s.members.len() == 1 && s.members[0].kind != MemberKind::Literal));
    }
}

/// A regexp literal as RuboCop's `RegexpNode` sees it.
pub struct Literal {
    /// `loc.begin` (`/`, `%r{`).
    pub open: (usize, usize),
    /// `loc.end`: the closing delimiter only (Prism's closing_loc also holds the options).
    pub close: (usize, usize),
    /// Between the delimiters.
    pub content: (usize, usize),
    /// `with_interpolations_blanked`: `#{...}` as spaces (one per character); `#@ivar` parts
    /// lose their `#`, as parser's `ivar` child does.
    pub pattern: String,
    pub extended: bool,
}

/// Regexp literal nodes (parser `regexp`, also inside `match_current_line`).
pub fn literal(node: &ruby_prism::Node) -> Option<Literal> {
    use crate::node_kind as kind;
    let range = |l: ruby_prism::Location| (l.start_offset(), l.end_offset());
    let text = |l: ruby_prism::Location| String::from_utf8_lossy(l.as_slice()).into_owned();
    let (open, close, pattern, extended) = match kind::of(node) {
        kind::REGULAR_EXPRESSION_NODE => {
            let r = node.as_regular_expression_node().unwrap();
            (range(r.opening_loc()), range(r.closing_loc()), text(r.content_loc()), r.is_extended())
        }
        kind::MATCH_LAST_LINE_NODE => {
            let r = node.as_match_last_line_node().unwrap();
            (range(r.opening_loc()), range(r.closing_loc()), text(r.content_loc()), r.is_extended())
        }
        kind::INTERPOLATED_REGULAR_EXPRESSION_NODE | kind::INTERPOLATED_MATCH_LAST_LINE_NODE => {
            let (open, close, parts, extended) = match node.as_interpolated_regular_expression_node() {
                Some(r) => (r.opening_loc(), r.closing_loc(), r.parts(), r.is_extended()),
                None => {
                    let r = node.as_interpolated_match_last_line_node().unwrap();
                    (r.opening_loc(), r.closing_loc(), r.parts(), r.is_extended())
                }
            };
            let mut pattern = String::new();
            for p in parts.iter() {
                let t = text(p.location());
                if p.as_embedded_statements_node().is_some() {
                    pattern.extend(std::iter::repeat_n(' ', t.chars().count()));
                } else if let Some(v) = p.as_embedded_variable_node() {
                    pattern.push_str(&text(v.variable().location()));
                } else {
                    pattern.push_str(&t);
                }
            }
            (range(open), range(close), pattern, extended)
        }
        _ => return None,
    };
    Some(Literal { open, close: (close.0, close.0 + 1), content: (open.1, close.0), pattern, extended })
}

impl Literal {
    /// Source byte offset of `origin + ts`: RuboCop adds the pattern's character index to the
    /// end of `loc.begin` as a character offset.
    pub fn source_offset(&self, src: &str, pattern_byte: usize) -> usize {
        let k = self.pattern[..pattern_byte.min(self.pattern.len())].chars().count();
        advance_chars(src, self.open.1, k)
    }

    /// `contents_range(node).source[index]` for a character index (negative counts from the end).
    pub fn content_char(&self, src: &str, index: isize) -> Option<char> {
        let content = &src[self.content.0..self.content.1];
        if index < 0 {
            let n = content.chars().count() as isize;
            if n + index < 0 {
                return None;
            }
            content.chars().nth((n + index) as usize)
        } else {
            content.chars().nth(index as usize)
        }
    }

    /// Character index in the pattern of a byte offset.
    pub fn char_index(&self, pattern_byte: usize) -> usize {
        self.pattern[..pattern_byte.min(self.pattern.len())].chars().count()
    }
}

/// The byte offset `k` characters after `from` (clamped to the end of `src`).
pub fn advance_chars(src: &str, from: usize, k: usize) -> usize {
    src[from..].char_indices().nth(k).map_or(src.len(), |(i, _)| from + i)
}
