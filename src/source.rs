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
    /// Source with CRLF normalised to LF (what parser and every cop see).
    pub text: &'a str,
    /// The file as read (ProcessedSource#raw_source), CRLF intact. Offsets refer to `text`.
    pub raw_text: &'a str,
    line_starts: Vec<usize>,
    /// Lines without newlines, stopping before a trailing `__END__` section (ProcessedSource#lines).
    pub lines: Vec<&'a str>,
    pub comments: Vec<Comment>,
    /// Prism grammar version the file was parsed with (see config::target_ruby::prism_version).
    pub version: &'static str,
    /// Target below Ruby 3.3: RuboCop parses with the parser gem, which accepts a few things Prism
    /// rejects (`runner::tolerated`).
    pub parser_gem: bool,
    /// AllCops/TargetRubyVersion (set by the runner).
    pub target_ruby: f64,
    /// Magic comment says binary/ASCII-8BIT: Ruby counts columns in bytes (see `binary_encoding`).
    pub binary: bool,
    tokens: OnceCell<Vec<Token>>,
    lex_tokens: OnceCell<Vec<Token>>,
    /// variable_force's tree of the program, built once for the cops that walk it.
    pub(crate) variable_tree: OnceCell<crate::helpers::variable_force::Tree>,
    /// parser_tokens::translated, which a dozen cops ask for.
    pub(crate) translated_tokens: OnceCell<Vec<Token>>,
    /// parse_warnings::warnings (a second parse), shared by the cops that need it.
    pub(crate) parse_warnings: OnceCell<Vec<crate::helpers::parse_warnings::Warning>>,
    /// Where lexing stops: the end, or an indented `__END__` the parser gem ends the input at
    /// (runner::parse_for_target; Prism would lex past it).
    lex_end: usize,
    /// The bytes `runner::parse_for_target`'s copy changed (offset, new byte): lexed with them, so
    /// the tokens agree with the parse.
    pub lex_edits: Vec<(usize, u8)>,
    /// Below 2.7, `syntax::legacy_numbered_sends`.
    pub numbered_sends: Vec<usize>,
    /// Below 3.3, in a file with a `\` line continuation: the offsets of the parser gem's tNL
    /// tokens (`legacy_syntax::newline_offsets`), whose lines count as code for CommentConfig.
    pub gem_newlines: Vec<usize>,
    /// The program the cops run on (set by runner::analyze): node_equality finds nodes by range in it.
    pub root: Option<ruby_prism::Node<'a>>,
    /// `root`'s nodes by range (node_equality::node_at).
    nodes_by_range: OnceCell<std::collections::HashMap<(usize, usize), ruby_prism::Node<'static>>>,
}

impl<'a> Source<'a> {
    /// The target when the parser gem parses it (below 3.3), for checks of the target's grammar.
    pub fn legacy(&self) -> Option<f64> {
        self.parser_gem.then_some(self.target_ruby)
    }

    /// The processed source's file name: TargetFinder's expanded absolute path (`.`/`..` folded,
    /// symlinks kept), which RuboCop uses for `__FILE__` and in cop error/warning locations.
    pub fn absolute_path(&self) -> String {
        crate::config::expand_path(std::path::Path::new(self.path), &crate::config::cwd())
            .to_string_lossy()
            .into_owned()
    }

    pub fn new(
        path: &'a str,
        text: &'a str,
        comments: Vec<Comment>,
        data_start: Option<usize>,
        version: &'static str,
    ) -> Self {
        let mut line_starts = vec![0];
        line_starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        let mut lines: Vec<&str> = text.split('\n').collect();
        let mut src = Source {
            path,
            text,
            raw_text: text,
            line_starts,
            lines: Vec::new(),
            comments,
            version,
            parser_gem: false,
            target_ruby: 3.4,
            binary: binary_encoding(text),
            tokens: OnceCell::new(),
            lex_tokens: OnceCell::new(),
            variable_tree: OnceCell::new(),
            translated_tokens: OnceCell::new(),
            parse_warnings: OnceCell::new(),
            lex_edits: Vec::new(),
            numbered_sends: Vec::new(),
            gem_newlines: Vec::new(),
            root: None,
            nodes_by_range: OnceCell::new(),
            lex_end: data_start.filter(|&d| !text[d..].starts_with("__END__")).unwrap_or(text.len()),
        };
        // ProcessedSource#lines stops at `__END__` only after the last token; with no tokens at
        // all (nothing but whitespace before it) it keeps every line, and an indented `__END__`
        // (the parser gem's data marker below 3.3) is not the `__END__` line it looks for. Its
        // buffer has no BOM (#521). The line must be `__END__` alone (its buffer turned `\r\n` into
        // `\n`): after `__END__\0` the lines go on, though the parser gem stops there (#586).
        if let Some(off) = data_start
            && matches!(lines.get(src.line_of(off) - 1), Some(&("__END__" | "__END__\r")))
            && !text[..off].trim_start_matches('\u{feff}').bytes().all(|b| b" \t\r\n\x0b\x0c".contains(&b))
        {
            lines.truncate(src.line_of(off) - 1);
        }
        src.lines = lines;
        src
    }

    /// The node of `root` at exactly `range` (see node_equality::index).
    pub fn node_at(&self, range: (usize, usize)) -> Option<&ruby_prism::Node<'a>> {
        let root = self.root.as_ref()?;
        self.nodes_by_range.get_or_init(|| crate::helpers::node_equality::index(root)).get(&range)
    }

    /// Whether the `_N` local read at `start` is the parser gem's `(send nil :_N)` (below 2.7).
    pub fn numbered_send(&self, start: usize) -> bool {
        self.numbered_sends.binary_search(&start).is_ok()
    }

    /// Up to 2.5, whether a lonely `else` the parse blanked out (`runner::useless_elses`) lies
    /// between `from` and `last`, outside each of `stmts` (a body's statements, `last` among them):
    /// the parser gem's tree has the statements after it in a `begin` of their own, the body's last
    /// value (#630).
    pub fn after_lonely_else(&self, from: usize, stmts: &[ruby_prism::Node], last: &ruby_prism::Node) -> bool {
        let to = last.location().start_offset();
        self.lex_edits.iter().any(|&(o, b)| {
            (from..to).contains(&o)
                && matches!(b, b' ' | b';')
                && self.text.as_bytes()[o..].starts_with(b"else")
                && !stmts.iter().any(|s| (s.location().start_offset()..s.location().end_offset()).contains(&o))
        })
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
        let segment = &self.text[self.line_start(self.line_of(off))..off];
        if self.binary { segment.len() } else { segment.chars().count() }
    }

    /// Number of physical lines (a trailing newline starts one more, empty line).
    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    pub fn slice(&self, start: usize, end: usize) -> &'a str {
        &self.text[start..end]
    }

    /// Prism lexer tokens in source order (rubocop-ast's `sorted_tokens`), computed on first use
    /// (only cops that need tokens pay for the second parse).
    pub fn tokens(&self) -> &[Token] {
        self.tokens.get_or_init(|| {
            let mut tokens = self.lex_tokens().to_vec();
            tokens.sort_by_key(|t| (t.start, t.end));
            tokens
        })
    }

    /// The same tokens in lexer order (`processed_source.tokens`): a heredoc's body follows its
    /// opening token, before the rest of the opening line.
    pub fn lex_tokens(&self) -> &[Token] {
        self.lex_tokens.get_or_init(|| {
            let mut copy = self.text.as_bytes()[..self.lex_end].to_vec();
            for &(o, b) in &self.lex_edits {
                if let Some(c) = copy.get_mut(o) {
                    *c = b;
                }
            }
            let mut tokens = lex(&copy, self.version, !self.binary);
            // Below 3.3 the parser gem's tIDENTIFIER (`Ж`, as `runner::parse_for_target` parses it).
            if self.parser_gem {
                for t in tokens.iter_mut().filter(|t| t.kind == ruby_prism_sys::PM_TOKEN_CONSTANT) {
                    if crate::legacy_syntax::non_ascii_constant_start(&self.text[t.start..]).is_some() {
                        t.kind = ruby_prism_sys::PM_TOKEN_IDENTIFIER;
                    }
                }
                // `def self&.foo`, parsed as `def self .foo` (#492): the `&.` token.
                for t in tokens.iter_mut().filter(|t| t.kind == ruby_prism_sys::PM_TOKEN_DOT && t.start > 0) {
                    if self.lex_edits.contains(&(t.start - 1, b' ')) && self.text.as_bytes()[t.start - 1] == b'&' {
                        (t.start, t.kind) = (t.start - 1, ruby_prism_sys::PM_TOKEN_AMPERSAND_DOT);
                    }
                }
            }
            tokens
        })
    }

    /// Comment that starts on `line`, if any.
    pub fn comment_on_line(&self, line: usize) -> Option<&Comment> {
        self.comments.iter().find(|c| self.line_of(c.start) == line)
    }

    /// True if the text before `off` on its line is only (Ruby, ASCII) whitespace.
    pub fn begins_its_line(&self, off: usize) -> bool {
        self.text[self.line_start(self.line_of(off))..off].bytes().all(crate::helpers::parser_tokens::is_ruby_space)
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

unsafe extern "C" {
    // Exported by libprism but missing from ruby-prism-sys's generated bindings.
    fn pm_options_version_set(options: *mut ruby_prism_sys::pm_options_t, version: *const u8, length: usize) -> bool;
    fn pm_options_free(options: *mut ruby_prism_sys::pm_options_t);
    fn pm_options_partial_script_set(options: *mut ruby_prism_sys::pm_options_t, partial_script: bool);
}

/// Tokens in source order, as Prism's own `Prism.lex` produces them (via the parser's lex callback).
fn lex(source: &[u8], version: &str, encoding_locked: bool) -> Vec<Token> {
    use ruby_prism_sys::*;
    let mut state = LexState { base: source.as_ptr(), tokens: Vec::new() };
    // SAFETY: mirrors ruby_prism::parse_with_version: the parser is initialised, used and freed
    // here, and the options, callback struct and state outlive the parse.
    unsafe {
        let mut options = std::mem::MaybeUninit::<pm_options_t>::zeroed().assume_init();
        pm_options_version_set(&mut options, version.as_ptr(), version.len());
        options.encoding_locked = encoding_locked;
        pm_options_partial_script_set(&mut options, true);
        let parser = Box::into_raw(Box::new(std::mem::MaybeUninit::<pm_parser_t>::uninit())) as *mut pm_parser_t;
        pm_parser_init(parser, source.as_ptr(), source.len(), &options);
        let mut callback =
            pm_lex_callback_t { data: &mut state as *mut LexState as *mut c_void, callback: Some(on_token) };
        (*parser).lex_callback = &mut callback;
        let node = pm_parse(parser);
        pm_node_destroy(parser, node);
        pm_parser_free(parser);
        pm_options_free(&mut options);
        drop(Box::from_raw(parser as *mut std::mem::MaybeUninit<pm_parser_t>));
    }
    let mut tokens = state.tokens;
    tokens.retain(|t| t.kind != ruby_prism_sys::PM_TOKEN_EOF);
    tokens
}

static ENCODING_RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    // Ruby `\s` is ASCII.
    regex::Regex::new(r"[\t\n\x0B\x0C\r #](?:en)?coding[\t\n\x0B\x0C\r ]*[:=][\t\n\x0B\x0C\r ]*(?:(utf8-mac)|([A-Za-z0-9_-]+?)(?:-unix|-dos|-mac)|([A-Za-z0-9_-]+))")
        .unwrap()
});

/// Parser::Source::Buffer.recognize_encoding: the magic comment on the first line (the second
/// after a shebang) names the source encoding. For binary (ASCII-8BIT) sources Ruby's String#length
/// counts bytes, so every column RuboCop reports is a byte offset.
pub fn binary_encoding(text: &str) -> bool {
    magic_encoding(text)
        .is_some_and(|name| name.eq_ignore_ascii_case("binary") || name.eq_ignore_ascii_case("ascii-8bit"))
}

/// The encoding named by the magic comment, if any.
fn magic_encoding(text: &str) -> Option<&str> {
    // recognize_encoding: a BOM means UTF-8 whatever the comment says.
    if text.starts_with('\u{feff}') {
        return None;
    }
    // `\A(.*)\n?(.*\n)?`: the second line counts only when a newline ends it; the comment must
    // start the line.
    let (first, rest) = text.split_once('\n').unwrap_or((text, ""));
    let line = if first.starts_with("#!") { rest.split_once('\n')?.0 } else { first };
    if !line.starts_with('#') {
        return None;
    }
    let c = ENCODING_RE.captures(line)?;
    // `utf8-mac` (lowercase) is special-cased before the emacs suffix is chewed.
    Some(c.get(1).or_else(|| c.get(2)).or_else(|| c.get(3))?.as_str())
}

/// What Parser::Source::Buffer#source= makes of a file's bytes.
pub enum Reencoded {
    /// Inspected as read: no magic comment, UTF-8 or binary, or an encoding without a table
    /// (docs/known-gaps.md).
    AsIs(Vec<u8>),
    /// Transcoded to UTF-8 from the encoding the magic comment names (reencode_string).
    Text(String),
    /// The bytes and Lint/Syntax's beautified message: an unknown encoding name
    /// (`Unknown encoding name - bogus.`) or Ruby's transcoding error (`"\xe3" on us-ascii.`).
    /// RuboCop runs no cop and corrects nothing.
    Error(Vec<u8>, String),
}

/// Parser::Source::Buffer.reencode_string (recognize_encoding's Encoding.find, then String#encode).
pub fn reencode(bytes: Vec<u8>) -> Reencoded {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(1024)]);
    let Some(name) = magic_encoding(&head).map(str::to_string) else { return Reencoded::AsIs(bytes) };
    let Some(enc) = crate::encoding::find(&name) else {
        // Encoding.find("internal") is nil (no default internal encoding): nothing is transcoded.
        if name.eq_ignore_ascii_case("internal") {
            return Reencoded::AsIs(bytes);
        }
        return Reencoded::Error(bytes, format!("Unknown encoding name - {}.", name.to_ascii_lowercase()));
    };
    match enc.decode(&bytes) {
        None => Reencoded::AsIs(bytes),
        Some(Ok(text)) => Reencoded::Text(text),
        Some(Err(message)) => Reencoded::Error(bytes, beautify(&message)),
    }
}

/// Lint/Syntax#beautify_message: String#capitalize and a final period.
pub fn beautify(message: &str) -> String {
    let lower = message.to_lowercase();
    let mut chars = lower.chars();
    let mut out: String = chars.next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
    out.extend(chars);
    if !out.ends_with('.') {
        out.push('.');
    }
    out
}

/// Parser::Source::Buffer#source=: a source without a BOM or a magic comment naming another
/// encoding is UTF-8 (ProcessedSource forces it), and one that is not valid UTF-8 raises
/// EncodingError, which RuboCop reports as Lint/Syntax.
pub fn must_be_utf8(text: &str) -> bool {
    text.starts_with('\u{feff}')
        || magic_encoding(text).is_none_or(|n| crate::encoding::find(n).is_some_and(|e| e.is_utf8()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexer_order_puts_heredoc_bodies_after_their_opener() {
        use ruby_prism_sys::*;
        let src = Source::new("t.rb", "x <<-EOT, 1\n  a\nEOT\n", Vec::new(), None, "3.4");
        let kinds: Vec<u32> = src.lex_tokens().iter().map(|t| t.kind).collect();
        let want = [
            PM_TOKEN_IDENTIFIER,
            PM_TOKEN_HEREDOC_START,
            PM_TOKEN_STRING_CONTENT,
            PM_TOKEN_HEREDOC_END,
            PM_TOKEN_COMMA,
            PM_TOKEN_INTEGER,
            PM_TOKEN_NEWLINE,
        ];
        assert_eq!(kinds, want);
        assert!(src.tokens().windows(2).all(|w| w[0].start <= w[1].start));
    }
}
