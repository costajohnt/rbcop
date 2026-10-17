//! RuboCop's AnnotationComment (`TODO: note` style comments).

use regex::Regex;

/// `AnnotationComment.new(comment, keywords)`: the regex captures of a comment's text.
pub struct Annotation<'a> {
    pub margin: &'a str,
    pub keyword: Option<&'a str>,
    pub colon: Option<&'a str>,
    pub space: Option<&'a str>,
    pub note: Option<&'a str>,
}

/// The annotation regex for a keyword list. Each thread compiles a keyword list once (compiling
/// cost more than every file's matching), and clones share the compiled program.
pub fn regex(keywords: &[String]) -> Regex {
    // ponytail: a linear list; a run has one keyword list per config, so it stays tiny.
    thread_local! {
        static COMPILED: std::cell::RefCell<Vec<(Vec<String>, Regex)>> = const { std::cell::RefCell::new(Vec::new()) };
    }
    COMPILED.with(|c| {
        if let Some((_, re)) = c.borrow().iter().find(|(k, _)| k == keywords) {
            return re.clone();
        }
        let re = compile(keywords);
        c.borrow_mut().push((keywords.to_vec(), re.clone()));
        re
    })
}

fn compile(keywords: &[String]) -> Regex {
    let mut kws: Vec<&String> = keywords.iter().collect();
    kws.sort_by_key(|k| std::cmp::Reverse(k.chars().count()));
    let union = kws.iter().map(|k| regex::escape(k)).collect::<Vec<_>>().join("|");
    // /^(# ?)(\b#{keywords}\b)(\s*:)?(\s+)?(\S+)?/i with Ruby's ASCII \s and \b.
    let s = r"[ \t\r\n\f\v]";
    Regex::new(&format!(r"(?im)^(# ?)((?-u:\b)(?:{union})(?-u:\b))({s}*:)?({s}+)?([^ \t\r\n\f\v]+)?")).unwrap()
}

impl<'a> Annotation<'a> {
    pub fn new(text: &'a str, re: &Regex) -> Self {
        match re.captures(text) {
            Some(c) => Annotation {
                margin: c.get(1).map_or("", |m| m.as_str()),
                keyword: c.get(2).map(|m| m.as_str()),
                colon: c.get(3).map(|m| m.as_str()),
                space: c.get(4).map(|m| m.as_str()),
                note: c.get(5).map(|m| m.as_str()),
            },
            None => Annotation { margin: "", keyword: None, colon: None, space: None, note: None },
        }
    }

    /// `annotation?`: keyword followed by a colon or space, and not just a capitalised word
    /// starting a sentence.
    pub fn annotation(&self) -> bool {
        let Some(kw) = self.keyword else { return false };
        if self.colon.is_none() && self.space.is_none() {
            return false;
        }
        !(kw == capitalize(kw) && self.colon.is_none() && self.space.is_some() && self.note.is_some())
    }

    /// `correct?(colon:)`
    pub fn correct(&self, colon: bool) -> bool {
        let Some(kw) = self.keyword else { return false };
        if self.space.is_none() || self.note.is_none() || kw != kw.to_uppercase() {
            return false;
        }
        self.colon.is_none() == !colon
    }

    /// `bounds` relative to the comment start: (start, end) byte offsets.
    pub fn bounds(&self) -> (usize, usize) {
        let start = self.margin.len();
        let len = [self.keyword, self.colon, self.space].iter().map(|p| p.map_or(0, str::len)).sum::<usize>();
        (start, start + len)
    }
}

/// Ruby's `String#capitalize`.
fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(f) => f.to_uppercase().chain(chars.flat_map(char::to_lowercase)).collect(),
        None => String::new(),
    }
}
