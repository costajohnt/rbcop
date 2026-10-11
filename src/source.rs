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
        };
        // ProcessedSource#lines stops at `__END__` only after the last token; with no tokens at
        // all (nothing but whitespace before it) it keeps every line.
        if let Some(off) = data_start
            && !text[..off].bytes().all(|b| b" \t\r\n\x0b\x0c".contains(&b))
        {
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
        self.lex_tokens.get_or_init(|| lex(self.text.as_bytes(), self.version, !self.binary))
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
    regex::Regex::new(r"[\t\n\x0B\x0C\r #](?:en)?coding[\t\n\x0B\x0C\r ]*[:=][\t\n\x0B\x0C\r ]*(utf8-mac|[A-Za-z0-9_-]+?(?:-unix|-dos|-mac)|[A-Za-z0-9_-]+)")
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
    Some(c.get(1)?.as_str().trim_end_matches("-unix").trim_end_matches("-dos").trim_end_matches("-mac"))
}

/// Encoding.name_list of Ruby 4.0.6 (RuboCop's Ruby), lowercase: what `Encoding.find` accepts.
const ENCODING_NAMES: &str = "\
    646 ansi_x3.4-1968 ascii ascii-8bit big5 big5-hkscs big5-hkscs:2008 big5-uao binary cesu-8 \
    cp1250 cp1251 cp1252 cp1253 cp1254 cp1255 cp1256 cp1257 cp1258 cp437 cp50220 cp50221 cp51932 \
    cp65000 cp65001 cp720 cp737 cp775 cp850 cp852 cp855 cp857 cp860 cp861 cp862 cp863 cp864 cp865 \
    cp866 cp869 cp874 cp878 cp932 cp936 cp949 cp950 cp951 cswindows31j ebcdic-cp-us emacs-mule \
    euc-cn euc-jis-2004 euc-jisx0213 euc-jp euc-jp-ms euc-kr euc-tw euccn eucjp eucjp-ms euckr \
    euctw external filesystem gb12345 gb18030 gb1988 gb2312 gbk ibm037 ibm437 ibm720 ibm737 ibm775 \
    ibm850 ibm852 ibm855 ibm857 ibm860 ibm861 ibm862 ibm863 ibm864 ibm865 ibm866 ibm869 internal \
    iso-2022-jp iso-2022-jp-2 iso-2022-jp-kddi iso-8859-1 iso-8859-10 iso-8859-11 iso-8859-13 \
    iso-8859-14 iso-8859-15 iso-8859-16 iso-8859-2 iso-8859-3 iso-8859-4 iso-8859-5 iso-8859-6 \
    iso-8859-7 iso-8859-8 iso-8859-9 iso2022-jp iso2022-jp2 iso8859-1 iso8859-10 iso8859-11 \
    iso8859-13 iso8859-14 iso8859-15 iso8859-16 iso8859-2 iso8859-3 iso8859-4 iso8859-5 iso8859-6 \
    iso8859-7 iso8859-8 iso8859-9 koi8-r koi8-u locale maccenteuro maccroatian maccyrillic macgreek \
    maciceland macjapan macjapanese macroman macromania macthai macturkish macukraine pck shift_jis \
    sjis sjis-docomo sjis-kddi sjis-softbank stateless-iso-2022-jp stateless-iso-2022-jp-kddi \
    tis-620 ucs-2be ucs-4be ucs-4le us-ascii utf-16 utf-16be utf-16le utf-32 utf-32be utf-32le \
    utf-7 utf-8 utf-8-hfs utf-8-mac utf8-docomo utf8-kddi utf8-mac utf8-softbank windows-1250 \
    windows-1251 windows-1252 windows-1253 windows-1254 windows-1255 windows-1256 windows-1257 \
    windows-1258 windows-31j windows-874";

/// Parser::Source::Buffer.recognize_encoding failing on a magic comment naming no encoding
/// (`Unknown encoding name - bogus.`, the message capitalized: the rest downcased), or
/// reencode_string failing on a source whose magic comment says US-ASCII but which has a byte above
/// 0x7f: RuboCop's beautified `"\xe3" on us-ascii.` (other encodings are not checked; see
/// docs/known-gaps.md). Either way RuboCop runs no cop and corrects nothing.
pub fn encoding_error(bytes: &[u8]) -> Option<String> {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(1024)]);
    let name = magic_encoding(&head)?;
    if !ENCODING_NAMES.split_ascii_whitespace().any(|n| n.eq_ignore_ascii_case(name)) {
        return Some(format!("Unknown encoding name - {}.", name.to_ascii_lowercase()));
    }
    let ascii = ["us-ascii", "ascii", "ansi_x3.4-1968", "646"].iter().any(|n| name.eq_ignore_ascii_case(n));
    let b = bytes.iter().find(|&&b| b > 0x7f).filter(|_| ascii)?;
    Some(format!("\"\\x{b:02x}\" on us-ascii."))
}

/// Parser::Source::Buffer#source=: a source without a BOM or a magic comment naming another
/// encoding is UTF-8 (ProcessedSource forces it), and one that is not valid UTF-8 raises
/// EncodingError, which RuboCop reports as Lint/Syntax.
pub fn must_be_utf8(text: &str) -> bool {
    text.starts_with('\u{feff}')
        || magic_encoding(text).is_none_or(|n| n.eq_ignore_ascii_case("utf-8") || n.eq_ignore_ascii_case("utf8"))
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
