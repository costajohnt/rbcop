//! libyaml's reader (reader.c) and scanner (scanner.c).
//!
//! Psych hands libyaml the config as a UTF-8 String with the encoding set, so the reader neither
//! detects nor drops a BOM; the scanner skips one at the start of any line (it still counts a
//! column). The reader decodes its input 16384 bytes at a time and rejects a byte sequence that
//! is no UTF-8 character, or a character outside YAML's printable set, as it decodes, so such a
//! sequence fails the whole parse at the first read of its chunk.

use super::{Error, Mark, ScalarStyle};
use std::collections::VecDeque;

#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    StreamStart,
    StreamEnd,
    VersionDirective(u32, u32),
    TagDirective(String, String),
    DocumentStart,
    DocumentEnd,
    BlockSequenceStart,
    BlockMappingStart,
    BlockEnd,
    FlowSequenceStart,
    FlowSequenceEnd,
    FlowMappingStart,
    FlowMappingEnd,
    BlockEntry,
    FlowEntry,
    Key,
    Value,
    Alias(String),
    Anchor(String),
    /// Handle and suffix.
    Tag(String, String),
    Scalar(String, ScalarStyle),
}

#[derive(Clone, Debug)]
pub struct Token {
    pub kind: TokenKind,
    pub start: Mark,
    pub end: Mark,
}

#[derive(Clone, Copy, Default)]
struct SimpleKey {
    possible: bool,
    required: bool,
    token_number: usize,
    mark: Mark,
}

/// libyaml's INPUT_RAW_BUFFER_SIZE: the reader decodes (and checks) this many bytes per read.
const RAW_CHUNK: usize = 16384;

type R<T = ()> = Result<T, Error>;

pub struct Scanner {
    chars: Vec<char>,
    /// The next character to read.
    pos: usize,
    /// Characters the reader has decoded and checked, and their bytes.
    decoded: usize,
    decoded_bytes: usize,
    /// The sequence `chars` stops at, if the input is no UTF-8 to its end.
    bad: Option<BadSequence>,
    /// The reader reached `bad` but fails only at its next read.
    bad_pending: bool,
    eof: bool,
    mark: Mark,
    tokens: VecDeque<Token>,
    tokens_parsed: usize,
    token_available: bool,
    stream_start_produced: bool,
    pub(super) stream_end_produced: bool,
    indent: isize,
    indents: Vec<isize>,
    simple_key_allowed: bool,
    simple_keys: Vec<SimpleKey>,
    flow_level: usize,
}

fn is_break_char(c: char) -> bool {
    matches!(c, '\r' | '\n' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

fn is_alpha_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

/// The reader's check: tab, LF, CR, printable ASCII, NEL, and the rest of Unicode but surrogates,
/// U+FFFE and U+FFFF.
fn is_printable_input(c: char) -> bool {
    matches!(c as u32, 0x09 | 0x0A | 0x0D | 0x20..=0x7E | 0x85 | 0xA0..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF)
}

/// A byte sequence yaml_parser_update_buffer's UTF-8 decoder rejects.
struct BadSequence {
    problem: &'static str,
    /// The bytes the decoder needs in its raw buffer to see the problem.
    width: usize,
    /// Running out of input: an error only when the reader is asked for more.
    incomplete: bool,
}

/// The characters libyaml's UTF-8 decoder takes from `bytes`, up to the first sequence it rejects.
fn decode_utf8(bytes: &[u8]) -> (Vec<char>, Option<BadSequence>) {
    let mut chars = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&octet) = bytes.get(i) {
        let width = match octet {
            0x00..=0x7F => 1,
            _ if octet & 0xE0 == 0xC0 => 2,
            _ if octet & 0xF0 == 0xE0 => 3,
            _ if octet & 0xF8 == 0xF0 => 4,
            _ => 0,
        };
        let bad = |problem, width| Some(BadSequence { problem, width, incomplete: false });
        if width == 0 {
            return (chars, bad("invalid leading UTF-8 octet", 1));
        }
        if i + width > bytes.len() {
            let problem = "incomplete UTF-8 octet sequence";
            return (chars, Some(BadSequence { problem, width, incomplete: true }));
        }
        let mut value = u32::from(octet) & [0x7F, 0x1F, 0x0F, 0x07][width - 1];
        for &trailing in &bytes[i + 1..i + width] {
            if trailing & 0xC0 != 0x80 {
                return (chars, bad("invalid trailing UTF-8 octet", width));
            }
            value = (value << 6) + (u32::from(trailing) & 0x3F);
        }
        if value < [0, 0x80, 0x800, 0x10000][width - 1] {
            return (chars, bad("invalid length of a UTF-8 sequence", width));
        }
        let Some(c) = char::from_u32(value) else { return (chars, bad("invalid Unicode character", width)) };
        chars.push(c);
        i += width;
    }
    (chars, None)
}

fn hex(c: char) -> u32 {
    c.to_digit(16).unwrap_or(0)
}

impl Scanner {
    pub fn new(text: &[u8]) -> Scanner {
        let (chars, bad) = decode_utf8(text);
        Scanner {
            chars,
            pos: 0,
            decoded: 0,
            decoded_bytes: 0,
            bad,
            bad_pending: false,
            eof: false,
            mark: Mark::default(),
            tokens: VecDeque::new(),
            tokens_parsed: 0,
            token_available: false,
            stream_start_produced: false,
            stream_end_produced: false,
            indent: 0,
            indents: Vec::new(),
            simple_key_allowed: false,
            simple_keys: Vec::new(),
            flow_level: 0,
        }
    }

    // ---- reader ----

    /// CACHE: make sure `n` characters are decoded (the end of input reads as NUL).
    fn cache(&mut self, n: usize) -> R {
        let reader_error = |problem| Error { problem, context: None, context_mark: Mark::default() };
        while self.pos + n > self.decoded && !self.eof {
            if let Some(bad) = self.bad.as_ref().filter(|_| self.bad_pending) {
                return Err(reader_error(bad.problem));
            }
            let limit = self.decoded_bytes + RAW_CHUNK;
            while let Some(&c) = self.chars.get(self.decoded) {
                if self.decoded_bytes + c.len_utf8() > limit {
                    break;
                }
                if !is_printable_input(c) {
                    return Err(reader_error("control characters are not allowed"));
                }
                self.decoded_bytes += c.len_utf8();
                self.decoded += 1;
            }
            if self.decoded < self.chars.len() {
                continue;
            }
            match &self.bad {
                None => self.eof = true,
                // A sequence that does not fit this read is decoded with the next one.
                Some(bad) if bad.incomplete || self.decoded_bytes + bad.width > limit => self.bad_pending = true,
                Some(bad) => return Err(reader_error(bad.problem)),
            }
        }
        Ok(())
    }

    fn at(&self, k: usize) -> char {
        self.chars.get(self.pos + k).copied().unwrap_or('\0')
    }

    fn check(&self, c: char) -> bool {
        self.at(0) == c
    }

    fn is_alpha(&self) -> bool {
        is_alpha_char(self.at(0))
    }

    fn is_blank_at(&self, k: usize) -> bool {
        matches!(self.at(k), ' ' | '\t')
    }

    fn is_break_at(&self, k: usize) -> bool {
        is_break_char(self.at(k))
    }

    fn is_breakz_at(&self, k: usize) -> bool {
        self.is_break_at(k) || self.at(k) == '\0'
    }

    fn is_blankz_at(&self, k: usize) -> bool {
        self.is_blank_at(k) || self.is_breakz_at(k)
    }

    fn is_crlf(&self) -> bool {
        self.at(0) == '\r' && self.at(1) == '\n'
    }

    fn skip(&mut self) {
        self.mark.index += 1;
        self.mark.column += 1;
        self.pos += 1;
    }

    fn skip_line(&mut self) {
        if self.is_crlf() {
            self.mark.index += 2;
            self.mark.column = 0;
            self.mark.line += 1;
            self.pos += 2;
        } else if self.is_break_at(0) {
            self.mark.index += 1;
            self.mark.column = 0;
            self.mark.line += 1;
            self.pos += 1;
        }
    }

    fn read(&mut self, s: &mut String) {
        s.push(self.at(0));
        self.skip();
    }

    /// READ_LINE: CR LF, CR, LF and NEL become LF; LS and PS are kept.
    fn read_line(&mut self, s: &mut String) {
        let c = self.at(0);
        if self.is_crlf() {
            s.push('\n');
            self.pos += 2;
            self.mark.index += 2;
        } else if matches!(c, '\r' | '\n' | '\u{85}') {
            s.push('\n');
            self.pos += 1;
            self.mark.index += 1;
        } else if matches!(c, '\u{2028}' | '\u{2029}') {
            s.push(c);
            self.pos += 1;
            self.mark.index += 1;
        } else {
            return;
        }
        self.mark.column = 0;
        self.mark.line += 1;
    }

    fn err(&self, context: Option<&'static str>, context_mark: Mark, problem: &'static str) -> Error {
        Error { problem, context, context_mark }
    }

    // ---- the parser's interface (PEEK_TOKEN, SKIP_TOKEN) ----

    pub fn peek(&mut self) -> R<&Token> {
        if !self.token_available {
            self.fetch_more_tokens()?;
        }
        Ok(self.tokens.front().expect("a token"))
    }

    pub fn skip_token(&mut self) -> Token {
        self.token_available = false;
        self.tokens_parsed += 1;
        let t = self.tokens.pop_front().expect("a token");
        self.stream_end_produced = t.kind == TokenKind::StreamEnd;
        t
    }

    fn fetch_more_tokens(&mut self) -> R {
        loop {
            let need_more = if self.tokens.is_empty() {
                true
            } else {
                self.stale_simple_keys()?;
                self.simple_keys.iter().any(|k| k.possible && k.token_number == self.tokens_parsed)
            };
            if !need_more {
                break;
            }
            self.fetch_next_token()?;
        }
        self.token_available = true;
        Ok(())
    }

    fn fetch_next_token(&mut self) -> R {
        self.cache(1)?;
        if !self.stream_start_produced {
            self.fetch_stream_start();
            return Ok(());
        }
        self.scan_to_next_token()?;
        self.stale_simple_keys()?;
        self.unroll_indent(self.mark.column as isize);
        self.cache(4)?;
        let c = self.at(0);
        if c == '\0' {
            return self.fetch_stream_end();
        }
        if self.mark.column == 0 && c == '%' {
            return self.fetch_directive();
        }
        if self.mark.column == 0
            && ((c == '-' && self.at(1) == '-' && self.at(2) == '-')
                || (c == '.' && self.at(1) == '.' && self.at(2) == '.'))
            && self.is_blankz_at(3)
        {
            let kind = if c == '-' { TokenKind::DocumentStart } else { TokenKind::DocumentEnd };
            return self.fetch_document_indicator(kind);
        }
        let flow = self.flow_level > 0;
        match c {
            '[' => return self.fetch_flow_collection_start(TokenKind::FlowSequenceStart),
            '{' => return self.fetch_flow_collection_start(TokenKind::FlowMappingStart),
            ']' => return self.fetch_flow_collection_end(TokenKind::FlowSequenceEnd),
            '}' => return self.fetch_flow_collection_end(TokenKind::FlowMappingEnd),
            ',' => return self.fetch_flow_entry(),
            '-' if self.is_blankz_at(1) => return self.fetch_block_entry(),
            '?' if flow || self.is_blankz_at(1) => return self.fetch_key(),
            ':' if flow || self.is_blankz_at(1) => return self.fetch_value(),
            '*' => return self.fetch_anchor(true),
            '&' => return self.fetch_anchor(false),
            '!' => return self.fetch_tag(),
            '|' if !flow => return self.fetch_block_scalar(true),
            '>' if !flow => return self.fetch_block_scalar(false),
            '\'' => return self.fetch_flow_scalar(true),
            '"' => return self.fetch_flow_scalar(false),
            _ => {}
        }
        let indicator = matches!(
            c,
            '-' | '?'
                | ':'
                | ','
                | '['
                | ']'
                | '{'
                | '}'
                | '#'
                | '&'
                | '*'
                | '!'
                | '|'
                | '>'
                | '\''
                | '"'
                | '%'
                | '@'
                | '`'
        );
        if !(self.is_blankz_at(0) || indicator)
            || (c == '-' && !self.is_blank_at(1))
            || (!flow && matches!(c, '?' | ':') && !self.is_blankz_at(1))
        {
            return self.fetch_plain_scalar();
        }
        Err(self.err(
            Some("while scanning for the next token"),
            self.mark,
            "found character that cannot start any token",
        ))
    }

    fn stale_simple_keys(&mut self) -> R {
        let mark = self.mark;
        for k in &mut self.simple_keys {
            if k.possible && (k.mark.line < mark.line || k.mark.index + 1024 < mark.index) {
                if k.required {
                    return Err(Error {
                        problem: "could not find expected ':'",
                        context: Some("while scanning a simple key"),
                        context_mark: k.mark,
                    });
                }
                k.possible = false;
            }
        }
        Ok(())
    }

    fn save_simple_key(&mut self) -> R {
        let required = self.flow_level == 0 && self.indent == self.mark.column as isize;
        if self.simple_key_allowed {
            let key = SimpleKey {
                possible: true,
                required,
                token_number: self.tokens_parsed + self.tokens.len(),
                mark: self.mark,
            };
            self.remove_simple_key()?;
            *self.simple_keys.last_mut().expect("a simple key level") = key;
        }
        Ok(())
    }

    fn remove_simple_key(&mut self) -> R {
        let k = self.simple_keys.last_mut().expect("a simple key level");
        if k.possible && k.required {
            return Err(Error {
                problem: "could not find expected ':'",
                context: Some("while scanning a simple key"),
                context_mark: k.mark,
            });
        }
        k.possible = false;
        Ok(())
    }

    fn increase_flow_level(&mut self) {
        self.simple_keys.push(SimpleKey::default());
        self.flow_level += 1;
    }

    fn decrease_flow_level(&mut self) {
        if self.flow_level > 0 {
            self.flow_level -= 1;
            self.simple_keys.pop();
        }
    }

    /// `number`: the token number to insert at, or None to append.
    fn roll_indent(&mut self, column: isize, number: Option<usize>, kind: TokenKind, mark: Mark) {
        if self.flow_level > 0 {
            return;
        }
        if self.indent < column {
            self.indents.push(self.indent);
            self.indent = column;
            let token = Token { kind, start: mark, end: mark };
            match number {
                None => self.tokens.push_back(token),
                Some(n) => self.tokens.insert(n - self.tokens_parsed, token),
            }
        }
    }

    fn unroll_indent(&mut self, column: isize) {
        if self.flow_level > 0 {
            return;
        }
        while self.indent > column {
            self.tokens.push_back(Token { kind: TokenKind::BlockEnd, start: self.mark, end: self.mark });
            self.indent = self.indents.pop().unwrap_or(-1);
        }
    }

    fn push(&mut self, kind: TokenKind, start: Mark) {
        self.tokens.push_back(Token { kind, start, end: self.mark });
    }

    fn fetch_stream_start(&mut self) {
        self.indent = -1;
        self.simple_keys.push(SimpleKey::default());
        self.simple_key_allowed = true;
        self.stream_start_produced = true;
        self.push(TokenKind::StreamStart, self.mark);
    }

    fn fetch_stream_end(&mut self) -> R {
        if self.mark.column != 0 {
            self.mark.column = 0;
            self.mark.line += 1;
        }
        self.unroll_indent(-1);
        self.remove_simple_key()?;
        self.simple_key_allowed = false;
        self.push(TokenKind::StreamEnd, self.mark);
        Ok(())
    }

    fn fetch_directive(&mut self) -> R {
        self.unroll_indent(-1);
        self.remove_simple_key()?;
        self.simple_key_allowed = false;
        let token = self.scan_directive()?;
        self.tokens.push_back(token);
        Ok(())
    }

    fn fetch_document_indicator(&mut self, kind: TokenKind) -> R {
        self.unroll_indent(-1);
        self.remove_simple_key()?;
        self.simple_key_allowed = false;
        let start = self.mark;
        self.skip();
        self.skip();
        self.skip();
        self.push(kind, start);
        Ok(())
    }

    fn fetch_flow_collection_start(&mut self, kind: TokenKind) -> R {
        self.save_simple_key()?;
        self.increase_flow_level();
        self.simple_key_allowed = true;
        let start = self.mark;
        self.skip();
        self.push(kind, start);
        Ok(())
    }

    fn fetch_flow_collection_end(&mut self, kind: TokenKind) -> R {
        self.remove_simple_key()?;
        self.decrease_flow_level();
        self.simple_key_allowed = false;
        let start = self.mark;
        self.skip();
        self.push(kind, start);
        Ok(())
    }

    fn fetch_flow_entry(&mut self) -> R {
        self.remove_simple_key()?;
        self.simple_key_allowed = true;
        let start = self.mark;
        self.skip();
        self.push(TokenKind::FlowEntry, start);
        Ok(())
    }

    fn fetch_block_entry(&mut self) -> R {
        if self.flow_level == 0 {
            if !self.simple_key_allowed {
                return Err(self.err(None, self.mark, "block sequence entries are not allowed in this context"));
            }
            self.roll_indent(self.mark.column as isize, None, TokenKind::BlockSequenceStart, self.mark);
        }
        self.remove_simple_key()?;
        self.simple_key_allowed = true;
        let start = self.mark;
        self.skip();
        self.push(TokenKind::BlockEntry, start);
        Ok(())
    }

    fn fetch_key(&mut self) -> R {
        if self.flow_level == 0 {
            if !self.simple_key_allowed {
                return Err(self.err(None, self.mark, "mapping keys are not allowed in this context"));
            }
            self.roll_indent(self.mark.column as isize, None, TokenKind::BlockMappingStart, self.mark);
        }
        self.remove_simple_key()?;
        self.simple_key_allowed = self.flow_level == 0;
        let start = self.mark;
        self.skip();
        self.push(TokenKind::Key, start);
        Ok(())
    }

    fn fetch_value(&mut self) -> R {
        let key = *self.simple_keys.last().expect("a simple key level");
        if key.possible {
            let token = Token { kind: TokenKind::Key, start: key.mark, end: key.mark };
            self.tokens.insert(key.token_number - self.tokens_parsed, token);
            self.roll_indent(key.mark.column as isize, Some(key.token_number), TokenKind::BlockMappingStart, key.mark);
            self.simple_keys.last_mut().expect("a simple key level").possible = false;
            self.simple_key_allowed = false;
        } else {
            if self.flow_level == 0 {
                if !self.simple_key_allowed {
                    return Err(self.err(None, self.mark, "mapping values are not allowed in this context"));
                }
                self.roll_indent(self.mark.column as isize, None, TokenKind::BlockMappingStart, self.mark);
            }
            self.simple_key_allowed = self.flow_level == 0;
        }
        let start = self.mark;
        self.skip();
        self.push(TokenKind::Value, start);
        Ok(())
    }

    fn fetch_anchor(&mut self, alias: bool) -> R {
        self.save_simple_key()?;
        self.simple_key_allowed = false;
        let token = self.scan_anchor(alias)?;
        self.tokens.push_back(token);
        Ok(())
    }

    fn fetch_tag(&mut self) -> R {
        self.save_simple_key()?;
        self.simple_key_allowed = false;
        let token = self.scan_tag()?;
        self.tokens.push_back(token);
        Ok(())
    }

    fn fetch_block_scalar(&mut self, literal: bool) -> R {
        self.remove_simple_key()?;
        self.simple_key_allowed = true;
        let token = self.scan_block_scalar(literal)?;
        self.tokens.push_back(token);
        Ok(())
    }

    fn fetch_flow_scalar(&mut self, single: bool) -> R {
        self.save_simple_key()?;
        self.simple_key_allowed = false;
        let token = self.scan_flow_scalar(single)?;
        self.tokens.push_back(token);
        Ok(())
    }

    fn fetch_plain_scalar(&mut self) -> R {
        self.save_simple_key()?;
        self.simple_key_allowed = false;
        let token = self.scan_plain_scalar()?;
        self.tokens.push_back(token);
        Ok(())
    }

    fn scan_to_next_token(&mut self) -> R {
        loop {
            self.cache(1)?;
            if self.mark.column == 0 && self.check('\u{feff}') {
                self.skip();
            }
            self.cache(1)?;
            while self.check(' ') || ((self.flow_level > 0 || !self.simple_key_allowed) && self.check('\t')) {
                self.skip();
                self.cache(1)?;
            }
            if self.check('#') {
                while !self.is_breakz_at(0) {
                    self.skip();
                    self.cache(1)?;
                }
            }
            if self.is_break_at(0) {
                self.cache(2)?;
                self.skip_line();
                if self.flow_level == 0 {
                    self.simple_key_allowed = true;
                }
            } else {
                break;
            }
        }
        Ok(())
    }

    fn scan_directive(&mut self) -> R<Token> {
        let start = self.mark;
        self.skip();
        let name = self.scan_directive_name(start)?;
        let kind = match name.as_str() {
            "YAML" => {
                let (major, minor) = self.scan_version_directive_value(start)?;
                TokenKind::VersionDirective(major, minor)
            }
            "TAG" => {
                let (handle, prefix) = self.scan_tag_directive_value(start)?;
                TokenKind::TagDirective(handle, prefix)
            }
            _ => return Err(self.err(Some("while scanning a directive"), start, "found unknown directive name")),
        };
        let end = self.mark;
        self.cache(1)?;
        while self.is_blank_at(0) {
            self.skip();
            self.cache(1)?;
        }
        if self.check('#') {
            while !self.is_breakz_at(0) {
                self.skip();
                self.cache(1)?;
            }
        }
        if !self.is_breakz_at(0) {
            return Err(self.err(
                Some("while scanning a directive"),
                start,
                "did not find expected comment or line break",
            ));
        }
        if self.is_break_at(0) {
            self.cache(2)?;
            self.skip_line();
        }
        Ok(Token { kind, start, end })
    }

    fn scan_directive_name(&mut self, start: Mark) -> R<String> {
        let mut s = String::new();
        self.cache(1)?;
        while self.is_alpha() {
            self.read(&mut s);
            self.cache(1)?;
        }
        if s.is_empty() {
            return Err(self.err(Some("while scanning a directive"), start, "could not find expected directive name"));
        }
        if !self.is_blankz_at(0) {
            return Err(self.err(
                Some("while scanning a directive"),
                start,
                "found unexpected non-alphabetical character",
            ));
        }
        Ok(s)
    }

    fn scan_version_directive_value(&mut self, start: Mark) -> R<(u32, u32)> {
        self.cache(1)?;
        while self.is_blank_at(0) {
            self.skip();
            self.cache(1)?;
        }
        let major = self.scan_version_directive_number(start)?;
        if !self.check('.') {
            return Err(self.err(
                Some("while scanning a %YAML directive"),
                start,
                "did not find expected digit or '.' character",
            ));
        }
        self.skip();
        let minor = self.scan_version_directive_number(start)?;
        Ok((major, minor))
    }

    fn scan_version_directive_number(&mut self, start: Mark) -> R<u32> {
        let mut value = 0u32;
        let mut length = 0;
        self.cache(1)?;
        while let Some(d) = self.at(0).to_digit(10) {
            length += 1;
            if length > 9 {
                return Err(self.err(
                    Some("while scanning a %YAML directive"),
                    start,
                    "found extremely long version number",
                ));
            }
            value = value * 10 + d;
            self.skip();
            self.cache(1)?;
        }
        if length == 0 {
            return Err(self.err(
                Some("while scanning a %YAML directive"),
                start,
                "did not find expected version number",
            ));
        }
        Ok(value)
    }

    fn scan_tag_directive_value(&mut self, start: Mark) -> R<(String, String)> {
        self.cache(1)?;
        while self.is_blank_at(0) {
            self.skip();
            self.cache(1)?;
        }
        let handle = self.scan_tag_handle(true, start)?;
        self.cache(1)?;
        if !self.is_blank_at(0) {
            return Err(self.err(Some("while scanning a %TAG directive"), start, "did not find expected whitespace"));
        }
        while self.is_blank_at(0) {
            self.skip();
            self.cache(1)?;
        }
        let prefix = self.scan_tag_uri(true, true, None, start)?;
        self.cache(1)?;
        if !self.is_blankz_at(0) {
            return Err(self.err(
                Some("while scanning a %TAG directive"),
                start,
                "did not find expected whitespace or line break",
            ));
        }
        Ok((handle, prefix))
    }

    fn scan_anchor(&mut self, alias: bool) -> R<Token> {
        let start = self.mark;
        let mut s = String::new();
        self.skip();
        self.cache(1)?;
        while self.is_alpha() {
            self.read(&mut s);
            self.cache(1)?;
        }
        let end = self.mark;
        if s.is_empty()
            || !(self.is_blankz_at(0) || matches!(self.at(0), '?' | ':' | ',' | ']' | '}' | '%' | '@' | '`'))
        {
            let context = if alias { "while scanning an alias" } else { "while scanning an anchor" };
            return Err(self.err(Some(context), start, "did not find expected alphabetic or numeric character"));
        }
        let kind = if alias { TokenKind::Alias(s) } else { TokenKind::Anchor(s) };
        Ok(Token { kind, start, end })
    }

    fn scan_tag(&mut self) -> R<Token> {
        let start = self.mark;
        self.cache(2)?;
        let (handle, suffix);
        if self.at(1) == '<' {
            self.skip();
            self.skip();
            let s = self.scan_tag_uri(true, false, None, start)?;
            if !self.check('>') {
                return Err(self.err(Some("while scanning a tag"), start, "did not find the expected '>'"));
            }
            self.skip();
            (handle, suffix) = (String::new(), s);
        } else {
            let h = self.scan_tag_handle(false, start)?;
            if h.len() > 1 && h.starts_with('!') && h.ends_with('!') {
                let s = self.scan_tag_uri(false, false, None, start)?;
                (handle, suffix) = (h, s);
            } else {
                let s = self.scan_tag_uri(false, false, Some(&h), start)?;
                // The `!` tag alone: handle "", suffix "!".
                (handle, suffix) = if s.is_empty() { (String::new(), "!".to_string()) } else { ("!".to_string(), s) };
            }
        }
        self.cache(1)?;
        if !self.is_blankz_at(0) && (self.flow_level == 0 || !self.check(',')) {
            return Err(self.err(
                Some("while scanning a tag"),
                start,
                "did not find expected whitespace or line break",
            ));
        }
        Ok(Token { kind: TokenKind::Tag(handle, suffix), start, end: self.mark })
    }

    fn scan_tag_handle(&mut self, directive: bool, start: Mark) -> R<String> {
        let mut s = String::new();
        self.cache(1)?;
        if !self.check('!') {
            let context = if directive { "while scanning a tag directive" } else { "while scanning a tag" };
            return Err(self.err(Some(context), start, "did not find expected '!'"));
        }
        self.read(&mut s);
        self.cache(1)?;
        while self.is_alpha() {
            self.read(&mut s);
            self.cache(1)?;
        }
        if self.check('!') {
            self.read(&mut s);
        } else if directive && s != "!" {
            return Err(self.err(Some("while parsing a tag directive"), start, "did not find expected '!'"));
        }
        Ok(s)
    }

    fn scan_tag_uri(&mut self, uri_char: bool, directive: bool, head: Option<&str>, start: Mark) -> R<String> {
        let mut length = head.map_or(0, str::len);
        let mut bytes: Vec<u8> = Vec::new();
        if let Some(h) = head.filter(|h| h.len() > 1) {
            bytes.extend_from_slice(&h.as_bytes()[1..]);
        }
        self.cache(1)?;
        loop {
            let c = self.at(0);
            let ok = is_alpha_char(c)
                || matches!(
                    c,
                    ';' | '/'
                        | '?'
                        | ':'
                        | '@'
                        | '&'
                        | '='
                        | '+'
                        | '$'
                        | '.'
                        | '%'
                        | '!'
                        | '~'
                        | '*'
                        | '\''
                        | '('
                        | ')'
                )
                || (uri_char && matches!(c, ',' | '[' | ']'));
            if !ok {
                break;
            }
            if c == '%' {
                self.scan_uri_escapes(directive, start, &mut bytes)?;
            } else {
                let mut buf = [0; 4];
                bytes.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                self.skip();
            }
            length += 1;
            self.cache(1)?;
        }
        if length == 0 {
            let context = if directive { "while parsing a %TAG directive" } else { "while parsing a tag" };
            return Err(self.err(Some(context), start, "did not find expected tag URI"));
        }
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    fn scan_uri_escapes(&mut self, directive: bool, start: Mark, bytes: &mut Vec<u8>) -> R {
        let context = if directive { "while parsing a %TAG directive" } else { "while parsing a tag" };
        let mut width = 0;
        loop {
            self.cache(3)?;
            if !(self.check('%') && self.at(1).is_ascii_hexdigit() && self.at(2).is_ascii_hexdigit()) {
                return Err(self.err(Some(context), start, "did not find URI escaped octet"));
            }
            let octet = ((hex(self.at(1)) << 4) + hex(self.at(2))) as u8;
            if width == 0 {
                width = match octet {
                    o if o & 0x80 == 0 => 1,
                    o if o & 0xE0 == 0xC0 => 2,
                    o if o & 0xF0 == 0xE0 => 3,
                    o if o & 0xF8 == 0xF0 => 4,
                    _ => return Err(self.err(Some(context), start, "found an incorrect leading UTF-8 octet")),
                };
            } else if octet & 0xC0 != 0x80 {
                return Err(self.err(Some(context), start, "found an incorrect trailing UTF-8 octet"));
            }
            bytes.push(octet);
            self.skip();
            self.skip();
            self.skip();
            width -= 1;
            if width == 0 {
                return Ok(());
            }
        }
    }

    fn scan_block_scalar(&mut self, literal: bool) -> R<Token> {
        let mut string = String::new();
        let mut leading_break = String::new();
        let mut trailing_breaks = String::new();
        let mut chomping = 0;
        let mut increment = 0;
        let mut indent: isize = 0;
        let mut leading_blank = false;
        let start = self.mark;
        self.skip();
        self.cache(1)?;
        let zero =
            |s: &Self| s.err(Some("while scanning a block scalar"), start, "found an indentation indicator equal to 0");
        if self.check('+') || self.check('-') {
            chomping = if self.check('+') { 1 } else { -1 };
            self.skip();
            self.cache(1)?;
            if let Some(d) = self.at(0).to_digit(10) {
                if d == 0 {
                    return Err(zero(self));
                }
                increment = d as isize;
                self.skip();
            }
        } else if let Some(d) = self.at(0).to_digit(10) {
            if d == 0 {
                return Err(zero(self));
            }
            increment = d as isize;
            self.skip();
            self.cache(1)?;
            if self.check('+') || self.check('-') {
                chomping = if self.check('+') { 1 } else { -1 };
                self.skip();
            }
        }
        self.cache(1)?;
        while self.is_blank_at(0) {
            self.skip();
            self.cache(1)?;
        }
        if self.check('#') {
            while !self.is_breakz_at(0) {
                self.skip();
                self.cache(1)?;
            }
        }
        if !self.is_breakz_at(0) {
            return Err(self.err(
                Some("while scanning a block scalar"),
                start,
                "did not find expected comment or line break",
            ));
        }
        if self.is_break_at(0) {
            self.cache(2)?;
            self.skip_line();
        }
        let mut end = self.mark;
        if increment > 0 {
            indent = if self.indent >= 0 { self.indent + increment } else { increment };
        }
        self.scan_block_scalar_breaks(&mut indent, &mut trailing_breaks, start, &mut end)?;
        self.cache(1)?;
        while self.mark.column as isize == indent && self.at(0) != '\0' {
            let trailing_blank = self.is_blank_at(0);
            if !literal && leading_break.starts_with('\n') && !leading_blank && !trailing_blank {
                if trailing_breaks.is_empty() {
                    string.push(' ');
                }
                leading_break.clear();
            } else {
                string.push_str(&leading_break);
                leading_break.clear();
            }
            string.push_str(&trailing_breaks);
            trailing_breaks.clear();
            leading_blank = self.is_blank_at(0);
            while !self.is_breakz_at(0) {
                self.read(&mut string);
                self.cache(1)?;
            }
            self.cache(2)?;
            self.read_line(&mut leading_break);
            self.scan_block_scalar_breaks(&mut indent, &mut trailing_breaks, start, &mut end)?;
        }
        if chomping != -1 {
            string.push_str(&leading_break);
        }
        if chomping == 1 {
            string.push_str(&trailing_breaks);
        }
        let style = if literal { ScalarStyle::Literal } else { ScalarStyle::Folded };
        Ok(Token { kind: TokenKind::Scalar(string, style), start, end })
    }

    fn scan_block_scalar_breaks(&mut self, indent: &mut isize, breaks: &mut String, start: Mark, end: &mut Mark) -> R {
        let mut max_indent = 0;
        *end = self.mark;
        loop {
            self.cache(1)?;
            while (*indent == 0 || (self.mark.column as isize) < *indent) && self.check(' ') {
                self.skip();
                self.cache(1)?;
            }
            max_indent = max_indent.max(self.mark.column as isize);
            if (*indent == 0 || (self.mark.column as isize) < *indent) && self.check('\t') {
                return Err(self.err(
                    Some("while scanning a block scalar"),
                    start,
                    "found a tab character where an indentation space is expected",
                ));
            }
            if !self.is_break_at(0) {
                break;
            }
            self.cache(2)?;
            self.read_line(breaks);
            *end = self.mark;
        }
        if *indent == 0 {
            *indent = max_indent.max(self.indent + 1).max(1);
        }
        Ok(())
    }

    fn scan_flow_scalar(&mut self, single: bool) -> R<Token> {
        let mut string = String::new();
        let mut leading_break = String::new();
        let mut trailing_breaks = String::new();
        let mut whitespaces = String::new();
        let start = self.mark;
        let quote = if single { '\'' } else { '"' };
        self.skip();
        loop {
            self.cache(4)?;
            if self.mark.column == 0
                && ((self.at(0) == '-' && self.at(1) == '-' && self.at(2) == '-')
                    || (self.at(0) == '.' && self.at(1) == '.' && self.at(2) == '.'))
                && self.is_blankz_at(3)
            {
                return Err(self.err(
                    Some("while scanning a quoted scalar"),
                    start,
                    "found unexpected document indicator",
                ));
            }
            if self.at(0) == '\0' {
                return Err(self.err(Some("while scanning a quoted scalar"), start, "found unexpected end of stream"));
            }
            self.cache(2)?;
            let mut leading_blanks = false;
            while !self.is_blankz_at(0) {
                if single && self.at(0) == '\'' && self.at(1) == '\'' {
                    string.push('\'');
                    self.skip();
                    self.skip();
                } else if self.check(quote) {
                    break;
                } else if !single && self.check('\\') && self.is_break_at(1) {
                    self.cache(3)?;
                    self.skip();
                    self.skip_line();
                    leading_blanks = true;
                    break;
                } else if !single && self.check('\\') {
                    let mut code_length = 0;
                    match self.at(1) {
                        '0' => string.push('\0'),
                        'a' => string.push('\x07'),
                        'b' => string.push('\x08'),
                        't' | '\t' => string.push('\t'),
                        'n' => string.push('\n'),
                        'v' => string.push('\x0B'),
                        'f' => string.push('\x0C'),
                        'r' => string.push('\r'),
                        'e' => string.push('\x1B'),
                        ' ' => string.push(' '),
                        '"' => string.push('"'),
                        '/' => string.push('/'),
                        '\\' => string.push('\\'),
                        'N' => string.push('\u{85}'),
                        '_' => string.push('\u{A0}'),
                        'L' => string.push('\u{2028}'),
                        'P' => string.push('\u{2029}'),
                        'x' => code_length = 2,
                        'u' => code_length = 4,
                        'U' => code_length = 8,
                        _ => {
                            return Err(self.err(
                                Some("while parsing a quoted scalar"),
                                start,
                                "found unknown escape character",
                            ));
                        }
                    }
                    self.skip();
                    self.skip();
                    if code_length > 0 {
                        self.cache(code_length)?;
                        let mut value = 0u32;
                        for k in 0..code_length {
                            if !self.at(k).is_ascii_hexdigit() {
                                return Err(self.err(
                                    Some("while parsing a quoted scalar"),
                                    start,
                                    "did not find expected hexdecimal number",
                                ));
                            }
                            value = (value << 4) + hex(self.at(k));
                        }
                        match char::from_u32(value) {
                            Some(c) => string.push(c),
                            None => {
                                return Err(self.err(
                                    Some("while parsing a quoted scalar"),
                                    start,
                                    "found invalid Unicode character escape code",
                                ));
                            }
                        }
                        for _ in 0..code_length {
                            self.skip();
                        }
                    }
                } else {
                    self.read(&mut string);
                }
                self.cache(2)?;
            }
            self.cache(1)?;
            if self.check(quote) {
                break;
            }
            self.cache(1)?;
            while self.is_blank_at(0) || self.is_break_at(0) {
                if self.is_blank_at(0) {
                    if !leading_blanks {
                        self.read(&mut whitespaces);
                    } else {
                        self.skip();
                    }
                } else {
                    self.cache(2)?;
                    if !leading_blanks {
                        whitespaces.clear();
                        self.read_line(&mut leading_break);
                        leading_blanks = true;
                    } else {
                        self.read_line(&mut trailing_breaks);
                    }
                }
                self.cache(1)?;
            }
            if leading_blanks {
                fold_breaks(&mut string, &mut leading_break, &mut trailing_breaks);
            } else {
                string.push_str(&whitespaces);
                whitespaces.clear();
            }
        }
        self.skip();
        let style = if single { ScalarStyle::SingleQuoted } else { ScalarStyle::DoubleQuoted };
        Ok(Token { kind: TokenKind::Scalar(string, style), start, end: self.mark })
    }

    fn scan_plain_scalar(&mut self) -> R<Token> {
        let mut string = String::new();
        let mut leading_break = String::new();
        let mut trailing_breaks = String::new();
        let mut whitespaces = String::new();
        let mut leading_blanks = false;
        let indent = self.indent + 1;
        let start = self.mark;
        let mut end = self.mark;
        loop {
            self.cache(4)?;
            if self.mark.column == 0
                && ((self.at(0) == '-' && self.at(1) == '-' && self.at(2) == '-')
                    || (self.at(0) == '.' && self.at(1) == '.' && self.at(2) == '.'))
                && self.is_blankz_at(3)
            {
                break;
            }
            if self.check('#') {
                break;
            }
            while !self.is_blankz_at(0) {
                if self.flow_level > 0 && self.check(':') && matches!(self.at(1), ',' | '?' | '[' | ']' | '{' | '}') {
                    return Err(self.err(Some("while scanning a plain scalar"), start, "found unexpected ':'"));
                }
                if (self.check(':') && self.is_blankz_at(1))
                    || (self.flow_level > 0 && matches!(self.at(0), ',' | '[' | ']' | '{' | '}'))
                {
                    break;
                }
                if leading_blanks || !whitespaces.is_empty() {
                    if leading_blanks {
                        fold_breaks(&mut string, &mut leading_break, &mut trailing_breaks);
                        leading_blanks = false;
                    } else {
                        string.push_str(&whitespaces);
                        whitespaces.clear();
                    }
                }
                self.read(&mut string);
                end = self.mark;
                self.cache(2)?;
            }
            if !(self.is_blank_at(0) || self.is_break_at(0)) {
                break;
            }
            self.cache(1)?;
            while self.is_blank_at(0) || self.is_break_at(0) {
                if self.is_blank_at(0) {
                    if leading_blanks && (self.mark.column as isize) < indent && self.check('\t') {
                        return Err(self.err(
                            Some("while scanning a plain scalar"),
                            start,
                            "found a tab character that violates indentation",
                        ));
                    }
                    if !leading_blanks {
                        self.read(&mut whitespaces);
                    } else {
                        self.skip();
                    }
                } else {
                    self.cache(2)?;
                    if !leading_blanks {
                        whitespaces.clear();
                        self.read_line(&mut leading_break);
                        leading_blanks = true;
                    } else {
                        self.read_line(&mut trailing_breaks);
                    }
                }
                self.cache(1)?;
            }
            if self.flow_level == 0 && (self.mark.column as isize) < indent {
                break;
            }
        }
        if leading_blanks {
            self.simple_key_allowed = true;
        }
        Ok(Token { kind: TokenKind::Scalar(string, ScalarStyle::Plain), start, end })
    }
}

/// The line folding of quoted and plain scalars: a single LF becomes a space, more are kept
/// without the first; an LS/PS break is kept as is.
fn fold_breaks(string: &mut String, leading_break: &mut String, trailing_breaks: &mut String) {
    if leading_break.starts_with('\n') {
        if trailing_breaks.is_empty() {
            string.push(' ');
        } else {
            string.push_str(trailing_breaks);
            trailing_breaks.clear();
        }
        leading_break.clear();
    } else {
        string.push_str(leading_break);
        string.push_str(trailing_breaks);
        leading_break.clear();
        trailing_breaks.clear();
    }
}
