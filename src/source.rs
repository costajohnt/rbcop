//! A file's text in RuboCop's terms: lines, offsets, comments and lexer tokens.

use std::cell::OnceCell;
use std::ffi::c_void;

/// One lexer token. `kind` is a Prism `PM_TOKEN_*` value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: u32,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct Comment {
    pub start: usize,
    pub end: usize,
    /// `=begin`/`=end` block rather than `#`.
    pub embdoc: bool,
}

pub struct Source<'a> {
    pub path: &'a str,
    pub text: &'a str,
    line_starts: Vec<usize>,
    /// Lines without newlines, stopping before a trailing `__END__` section (ProcessedSource#lines).
    pub lines: Vec<&'a str>,
    pub comments: Vec<Comment>,
    tokens: OnceCell<Vec<Token>>,
}

impl<'a> Source<'a> {
    pub fn new(path: &'a str, text: &'a str, comments: Vec<Comment>, data_start: Option<usize>) -> Self {
        let mut line_starts = vec![0];
        line_starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        let mut lines: Vec<&str> = text.split('\n').collect();
        let mut src = Source { path, text, line_starts, lines: Vec::new(), comments, tokens: OnceCell::new() };
        if let Some(off) = data_start {
            lines.truncate(src.line_of(off) - 1);
        }
        src.lines = lines;
        src
    }

    /// 1-based line containing byte offset.
    pub fn line_of(&self, off: usize) -> usize {
        self.line_starts.partition_point(|&s| s <= off)
    }

    /// Byte offset where 1-based `line` starts.
    pub fn line_start(&self, line: usize) -> usize {
        self.line_starts[line - 1]
    }

    /// 0-based character column of byte offset (RuboCop's `column`).
    pub fn column(&self, off: usize) -> usize {
        self.text[self.line_start(self.line_of(off))..off].chars().count()
    }

    /// Number of physical lines (a trailing newline starts one more, empty line).
    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    pub fn slice(&self, start: usize, end: usize) -> &'a str {
        &self.text[start..end]
    }

    /// Prism lexer tokens, computed on first use (only cops that need tokens pay for the second parse).
    pub fn tokens(&self) -> &[Token] {
        self.tokens.get_or_init(|| lex(self.text.as_bytes()))
    }

    /// Comment that starts on `line`, if any.
    pub fn comment_on_line(&self, line: usize) -> Option<&Comment> {
        self.comments.iter().find(|c| self.line_of(c.start) == line)
    }

    /// True if the text before `off` on its line is only whitespace.
    pub fn begins_its_line(&self, off: usize) -> bool {
        self.text[self.line_start(self.line_of(off))..off].chars().all(char::is_whitespace)
    }
}

struct LexState {
    base: *const u8,
    tokens: Vec<Token>,
}

unsafe extern "C" fn on_token(
    data: *mut c_void,
    _parser: *mut ruby_prism_sys::pm_parser_t,
    token: *mut ruby_prism_sys::pm_token_t,
) {
    // SAFETY: `data` is the LexState passed to pm_parser_init below, alive for the whole parse;
    // token pointers point into the source buffer that starts at `base`.
    unsafe {
        let state = &mut *(data as *mut LexState);
        let t = &*token;
        state.tokens.push(Token {
            kind: t.type_,
            start: t.start.offset_from(state.base) as usize,
            end: t.end.offset_from(state.base) as usize,
        });
    }
}

/// Tokens in source order, as Prism's own `Prism.lex` produces them (via the parser's lex callback).
fn lex(source: &[u8]) -> Vec<Token> {
    use ruby_prism_sys::*;
    let mut state = LexState { base: source.as_ptr(), tokens: Vec::new() };
    // SAFETY: mirrors ruby_prism::parse: the parser is initialised, used and freed here, and the
    // callback struct and state outlive the parse.
    unsafe {
        let parser = Box::into_raw(Box::new(std::mem::MaybeUninit::<pm_parser_t>::uninit())) as *mut pm_parser_t;
        pm_parser_init(parser, source.as_ptr(), source.len(), std::ptr::null());
        let mut callback =
            pm_lex_callback_t { data: &mut state as *mut LexState as *mut c_void, callback: Some(on_token) };
        (*parser).lex_callback = &mut callback;
        let node = pm_parse(parser);
        pm_node_destroy(parser, node);
        pm_parser_free(parser);
        drop(Box::from_raw(parser as *mut std::mem::MaybeUninit<pm_parser_t>));
    }
    let mut tokens = state.tokens;
    // Heredoc bodies are lexed after the rest of their opening line; RuboCop's tokens are position-sorted.
    tokens.sort_by_key(|t| (t.start, t.end));
    tokens.retain(|t| t.kind != ruby_prism_sys::PM_TOKEN_EOF);
    tokens
}
