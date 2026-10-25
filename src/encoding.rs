//! Ruby's transcoding of a source to UTF-8 (`String#encode`, as Parser::Source::Buffer
//! .reencode_string calls it for a magic comment naming another encoding), from the tables
//! scripts/encodings.rb records from RuboCop's Ruby: each encoding's characters, its undefined
//! conversions, and so the bytes and wording of the first error Ruby raises.

use std::sync::LazyLock;

pub struct Encoding {
    /// Encoding#name (`Shift_JIS`).
    pub name: &'static str,
    /// 0 no table, 1/2 a table (the wording of an undefined conversion), 3 no converter.
    kind: u8,
    /// Sorted 6-byte entries: the bytes (u24) and the code point (u24, `UNDEFINED` for none).
    entries: &'static [u8],
}

const UNDEFINED: u32 = 0xFF_FFFF;

struct Tables {
    aliases: Vec<(&'static str, usize)>,
    encodings: Vec<Encoding>,
}

static TABLES: LazyLock<Tables> = LazyLock::new(|| {
    let data: &'static [u8] = include_bytes!("encodings.bin");
    let mut at = 0;
    let mut take = |n: usize| {
        let s = &data[at..at + n];
        at += n;
        s
    };
    let str_of = |s: &'static [u8]| std::str::from_utf8(s).unwrap();
    let count = u16::from_be_bytes(take(2).try_into().unwrap()) as usize;
    let mut aliases = Vec::with_capacity(count);
    for _ in 0..count {
        let len = take(1)[0] as usize;
        let name = str_of(take(len));
        aliases.push((name, take(1)[0] as usize));
    }
    let count = take(1)[0] as usize;
    let mut encodings = Vec::with_capacity(count);
    for _ in 0..count {
        let len = take(1)[0] as usize;
        let name = str_of(take(len));
        let kind = take(1)[0];
        let entries = if matches!(kind, 1 | 2) {
            let n = u32::from_be_bytes(take(4).try_into().unwrap()) as usize;
            take(n * 6)
        } else {
            &[]
        };
        encodings.push(Encoding { name, kind, entries });
    }
    Tables { aliases, encodings }
});

/// Encoding.find: the encoding a name (any case) or alias names.
pub fn find(name: &str) -> Option<&'static Encoding> {
    let t = &*TABLES;
    t.aliases.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|&(_, i)| &t.encodings[i])
}

impl Encoding {
    pub fn is_utf8(&self) -> bool {
        self.name == "UTF-8"
    }

    fn key(&self, i: usize) -> u32 {
        let r = &self.entries[i * 6..];
        u32::from_be_bytes([0, r[0], r[1], r[2]])
    }

    fn lookup(&self, key: u32) -> Option<u32> {
        let n = self.entries.len() / 6;
        let i = partition(n, |i| self.key(i) < key);
        (i < n && self.key(i) == key).then(|| {
            let r = &self.entries[i * 6 + 3..];
            u32::from_be_bytes([0, r[0], r[1], r[2]])
        })
    }

    /// Some character's bytes start with `key`'s (`len` bytes).
    fn extends(&self, key: u32, len: usize) -> bool {
        let n = self.entries.len() / 6;
        let has = |lo: u32, hi: u32| {
            let i = partition(n, |i| self.key(i) < lo);
            i < n && self.key(i) < hi
        };
        has(key << 8, (key + 1) << 8) || (len == 1 && has(key << 16, (key + 1) << 16))
    }

    /// `bytes.force_encoding(self).encode('UTF-8')`: the text, or the message of Ruby's error
    /// (None: no table, the source is inspected as read). An ASCII-only string is returned as is.
    pub fn decode(&self, bytes: &[u8]) -> Option<Result<String, String>> {
        if self.kind == 0 {
            return None;
        }
        if bytes.is_ascii() {
            return Some(Ok(String::from_utf8(bytes.to_vec()).unwrap()));
        }
        if self.kind == 3 {
            return Some(Err(format!("code converter not found ({} to UTF-8)", self.name)));
        }
        let mut out = String::with_capacity(bytes.len() * 2);
        let mut i = 0;
        while i < bytes.len() {
            let mut key = 0;
            let mut n = 0;
            loop {
                key = key << 8 | bytes[i + n] as u32;
                n += 1;
                let seq = &bytes[i..i + n];
                if let Some(cp) = self.lookup(key) {
                    if cp == UNDEFINED {
                        return Some(Err(if self.kind == 1 {
                            format!("{} from {} to UTF-8", dump(seq), self.name)
                        } else {
                            format!("{} to UTF-8 in conversion from {} to UTF-8", dump(seq), self.name)
                        }));
                    }
                    out.push(char::from_u32(cp).unwrap());
                    break;
                }
                if n == 1 && bytes[i] < 0x80 {
                    out.push(bytes[i] as char);
                    break;
                }
                if !self.extends(key, n) {
                    return Some(Err(if n == 1 {
                        format!("{} on {}", dump(seq), self.name)
                    } else {
                        format!("{} followed by {} on {}", dump(&seq[..n - 1]), dump(&seq[n - 1..]), self.name)
                    }));
                }
                if i + n == bytes.len() {
                    return Some(Err(format!("incomplete {} on {}", dump(seq), self.name)));
                }
            }
            i += n;
        }
        Some(Ok(out))
    }
}

fn partition(n: usize, pred: impl Fn(usize) -> bool) -> usize {
    let (mut lo, mut hi) = (0, n);
    while lo < hi {
        let mid = (lo + hi) / 2;
        if pred(mid) { lo = mid + 1 } else { hi = mid }
    }
    lo
}

/// String#dump of a binary string.
fn dump(bytes: &[u8]) -> String {
    let mut s = String::from("\"");
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'"' => s.push_str("\\\""),
            b'\\' => s.push_str("\\\\"),
            b'\n' => s.push_str("\\n"),
            b'\r' => s.push_str("\\r"),
            b'\t' => s.push_str("\\t"),
            0x0c => s.push_str("\\f"),
            0x0b => s.push_str("\\v"),
            0x08 => s.push_str("\\b"),
            0x07 => s.push_str("\\a"),
            0x1b => s.push_str("\\e"),
            b'#' if matches!(bytes.get(i + 1), Some(b'$' | b'@' | b'{')) => s.push_str("\\#"),
            0x20..=0x7e => s.push(b as char),
            _ => s.push_str(&format!("\\x{b:02X}")),
        }
    }
    s.push('"');
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(name: &str, bytes: &[u8]) -> Result<String, String> {
        find(name).unwrap().decode(bytes).unwrap()
    }

    #[test]
    fn transcodes_like_ruby() {
        assert_eq!(decode("iso-8859-1", b"caf\xe9\n"), Ok("café\n".into()));
        assert_eq!(decode("SHIFT_JIS", b"\x93\xfa\x96{\n"), Ok("日本\n".into()));
        // Ruby's Shift_JIS is not WHATWG's: 0x8160 is WAVE DASH.
        assert_eq!(decode("shift_jis", b"\x81\x60"), Ok("\u{301c}".into()));
        assert_eq!(decode("sjis", b"\x81\x60"), Ok("\u{ff5e}".into()));
        assert_eq!(decode("euc-jp", b"\xc6\xfc\xcb\xdc"), Ok("日本".into()));
        assert_eq!(decode("euc-jp", b"\x8f\xa2\xb7"), Ok("~".into()));
        assert_eq!(decode("us-ascii", b"x"), Ok("x".into()));
    }

    #[test]
    fn errors_word_like_ruby() {
        assert_eq!(decode("euc-jp", b"s = \"\xa4\"\n"), Err(r#""\xA4" followed by "\"" on EUC-JP"#.into()));
        assert_eq!(decode("shift_jis", b"\x80"), Err(r#""\x80" on Shift_JIS"#.into()));
        assert_eq!(decode("shift_jis", b"x\x81"), Err(r#"incomplete "\x81" on Shift_JIS"#.into()));
        assert_eq!(decode("euc-jp", b"\xa9\xa1"), Err(r#""\xA9\xA1" from EUC-JP to UTF-8"#.into()));
        assert_eq!(decode("euc-jp", b"\x8f\xa1#{"), Err(r##""\x8F\xA1" followed by "#" on EUC-JP"##.into()));
        assert_eq!(
            decode("cp1252", b"\x81"),
            Err(r#""\x81" to UTF-8 in conversion from Windows-1252 to UTF-8"#.into())
        );
        assert_eq!(decode("us-ascii", b"\xe3"), Err(r#""\xE3" on US-ASCII"#.into()));
        assert_eq!(decode("emacs-mule", b"\xe9"), Err("code converter not found (Emacs-Mule to UTF-8)".into()));
        assert_eq!(find("euc-tw").unwrap().decode(b"x"), Some(Ok("x".into())));
        assert_eq!(find("gb18030").unwrap().decode(b"\x81\x30\x81\x30"), None);
    }
}
