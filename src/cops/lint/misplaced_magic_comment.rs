use crate::helpers::magic_comment::frozen_string_literal as frozen_value;
use crate::helpers::magic_comment;
use crate::helpers::range_help::{Side, Surrounding, range_with_surrounding_space};
use crate::prelude::*;
use crate::source::Source;

pub const NAME: &str = "Lint/MisplacedMagicComment";
pub static COP: MisplacedMagicComment = MisplacedMagicComment;
pub struct MisplacedMagicComment;

const MSG_ENCODING: &str =
    "The `encoding` magic comment is ignored unless placed on the first line (or below a shebang on the first line).";
const MSG_ABOVE_SHEBANG: &str = "A magic comment above a shebang renders the shebang ineffective.";

/// `Encoding.name_list` of Ruby 4.0, downcased (`Encoding.find` is case-insensitive).
const ENCODINGS: &[&str] = &[
    "646", "ansi_x3.4-1968", "ascii", "ascii-8bit", "big5", "big5-hkscs", "big5-hkscs:2008", "big5-uao", "binary",
    "cesu-8", "cp1250", "cp1251", "cp1252", "cp1253", "cp1254", "cp1255", "cp1256", "cp1257", "cp1258", "cp437",
    "cp50220", "cp50221", "cp51932", "cp65000", "cp65001", "cp720", "cp737", "cp775", "cp850", "cp852", "cp855",
    "cp857", "cp860", "cp861", "cp862", "cp863", "cp864", "cp865", "cp866", "cp869", "cp874", "cp878", "cp932",
    "cp936", "cp949", "cp950", "cp951", "cswindows31j", "ebcdic-cp-us", "emacs-mule", "euc-cn", "euc-jis-2004",
    "euc-jisx0213", "euc-jp", "euc-jp-ms", "euc-kr", "euc-tw", "euccn", "eucjp", "eucjp-ms", "euckr", "euctw",
    "external", "filesystem", "gb12345", "gb18030", "gb1988", "gb2312", "gbk", "ibm037", "ibm437", "ibm720",
    "ibm737", "ibm775", "ibm850", "ibm852", "ibm855", "ibm857", "ibm860", "ibm861", "ibm862", "ibm863", "ibm864",
    "ibm865", "ibm866", "ibm869", "internal", "iso-2022-jp", "iso-2022-jp-2", "iso-2022-jp-kddi", "iso-8859-1",
    "iso-8859-10", "iso-8859-11", "iso-8859-13", "iso-8859-14", "iso-8859-15", "iso-8859-16", "iso-8859-2",
    "iso-8859-3", "iso-8859-4", "iso-8859-5", "iso-8859-6", "iso-8859-7", "iso-8859-8", "iso-8859-9", "iso2022-jp",
    "iso2022-jp2", "iso8859-1", "iso8859-10", "iso8859-11", "iso8859-13", "iso8859-14", "iso8859-15", "iso8859-16",
    "iso8859-2", "iso8859-3", "iso8859-4", "iso8859-5", "iso8859-6", "iso8859-7", "iso8859-8", "iso8859-9", "koi8-r",
    "koi8-u", "locale", "maccenteuro", "maccroatian", "maccyrillic", "macgreek", "maciceland", "macjapan",
    "macjapanese", "macroman", "macromania", "macthai", "macturkish", "macukraine", "pck", "shift_jis", "sjis",
    "sjis-docomo", "sjis-kddi", "sjis-softbank", "stateless-iso-2022-jp", "stateless-iso-2022-jp-kddi", "tis-620",
    "ucs-2be", "ucs-4be", "ucs-4le", "us-ascii", "utf-16", "utf-16be", "utf-16le", "utf-32", "utf-32be", "utf-32le",
    "utf-7", "utf-8", "utf-8-hfs", "utf-8-mac", "utf8-docomo", "utf8-kddi", "utf8-mac", "utf8-softbank",
    "windows-1250", "windows-1251", "windows-1252", "windows-1253", "windows-1254", "windows-1255", "windows-1256",
    "windows-1257", "windows-1258", "windows-31j", "windows-874",
];

/// `/\A#(?![^#]*#)/`: a `#` comment with no other `#` in it.
fn magic_comment_shaped(text: &str) -> bool {
    text.starts_with('#') && !text[1..].contains('#')
}

/// `preceded_only_by_magic_comments?`: every line above is a magic comment (or a shebang on line 1).
fn preceded_only_by_magic_comments(src: &Source, line: usize) -> bool {
    (1..line).all(|n| {
        let text = src.lines.get(n - 1).copied().unwrap_or("");
        (n == 1 && text.starts_with("#!")) || magic_comment::valid(text)
    })
}

fn comment_starts_line(src: &Source, line: usize) -> bool {
    src.lines.get(line - 1).is_some_and(|l| l.trim_start_matches([' ', '\t', '\n', '\x0b', '\x0c', '\r', '\0']).starts_with('#'))
}

/// `buffer.line_range(line)`: the line without its newline.
fn line_range(src: &Source, line: usize) -> (usize, usize) {
    let b = src.line_start(line);
    (b, b + src.text[b..].find('\n').unwrap_or(src.text.len() - b))
}

impl Cop for MisplacedMagicComment {
    fn correctable(&self) -> bool {
        true
    }

    fn on_file(&self, cx: &mut Cx) {
        let src = cx.src;
        if src.text.is_empty() {
            return;
        }
        let comment_text = |c: &Comment| &src.text[c.start..c.end];
        // check_magic_comment_above_shebang
        let shebang = src.comments.iter().find(|c| {
            src.line_of(c.start) > 1 && comment_text(c).starts_with("#!") && src.column(c.start) == 0
        });
        if let Some(c) = shebang.filter(|c| preceded_only_by_magic_comments(src, src.line_of(c.start))) {
            cx.add_with(c.start, c.end, MSG_ABOVE_SHEBANG, false);
        }
        let effective_line = if src.lines.first().is_some_and(|l| l.starts_with("#!")) { 2 } else { 1 };
        let first_code_token = src
            .tokens()
            .iter()
            .find(|t| {
                !matches!(
                    t.kind,
                    sys::PM_TOKEN_COMMENT
                        | sys::PM_TOKEN_EMBDOC_BEGIN
                        | sys::PM_TOKEN_EMBDOC_LINE
                        | sys::PM_TOKEN_EMBDOC_END
                        | sys::PM_TOKEN_NEWLINE
                        | sys::PM_TOKEN_IGNORED_NEWLINE
                        | sys::PM_TOKEN___END__
                        | sys::PM_TOKEN_EOF
                )
            })
            .map(|t| t.start);
        for c in src.comments.iter().filter(|c| !c.embdoc) {
            let text = comment_text(c);
            if !magic_comment_shaped(text) || !magic_comment::valid(text) {
                continue;
            }
            let line = src.line_of(c.start);
            let msg = if magic_comment::encoding_specified(text) {
                let known = magic_comment::encoding(text).is_some_and(|e| ENCODINGS.contains(&e.to_lowercase().as_str()));
                if !known
                    || (line == effective_line && comment_starts_line(src, line))
                    || preceded_only_by_magic_comments(src, line)
                {
                    continue;
                }
                MSG_ENCODING.to_string()
            } else if frozen_value(text).is_some() {
                if first_code_token.is_none_or(|f| c.start <= f) {
                    continue;
                }
                "The `frozen_string_literal` magic comment is ignored after any code.".to_string()
            } else {
                continue;
            };
            // move_comment
            let removal = if comment_starts_line(src, line) {
                helpers::whole_lines::range_by_whole_lines(src.text, c.start, c.end, true)
            } else {
                let opts = Surrounding { side: Side::Left, ..Default::default() };
                range_with_surrounding_space(src.text, c.start, c.end, opts)
            };
            let target = line_range(src, effective_line);
            let text = text.to_string();
            cx.add_fix(c.start, c.end, msg, |k| {
                k.remove(removal.0, removal.1);
                if line < effective_line {
                    k.insert_after(target.0, target.1, &format!("\n{text}"));
                } else {
                    k.insert_before(target.0, target.1, &format!("{text}\n"));
                }
            });
        }
    }
}
