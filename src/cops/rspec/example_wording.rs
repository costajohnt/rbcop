use crate::prelude::*;

pub const NAME: &str = "RSpec/ExampleWording";
pub static COP: ExampleWording = ExampleWording;
pub struct ExampleWording;

static SHOULD_PREFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\Ashould(?:n't|n’t)?\b").unwrap());
static WILL_PREFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\A(?:will|won't|won’t)\b").unwrap());
static IT_PREFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\Ait ").unwrap());
static SHOULDNT_BE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\Ashould(?:n't|n’t| not)\b be\b").unwrap());
static SHOULDNT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\Ashould(?:n't|n’t| not)\b").unwrap());
static WILL_NOT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\Awill not\b").unwrap());
static WONT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\Awo(?:n't|n’t)\b").unwrap());
static ES_SUFFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)(?:o|s|x|ch|sh|z)\z").unwrap());
static IES_SUFFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)[^aeou]y\z").unwrap());

fn uppercase(word: &str) -> bool {
    word.to_uppercase() == word
}

/// RuboCop::RSpec::Wording#rewrite
fn rewrite(text: &str, ignores: &[String], replacements: &Map) -> String {
    for (re, replacement) in [(&*SHOULDNT_BE, "is not"), (&*SHOULDNT, "does not"), (&*WILL_NOT, "does not"), (&*WONT, "does not")] {
        if let Some(m) = re.find(text) {
            let r = if uppercase(m.as_str()) { replacement.to_uppercase() } else { replacement.to_string() };
            return format!("{r}{}", &text[m.end()..]);
        }
    }
    // remove_should_and_pluralize
    let mut words: Vec<String> = text.split([' ', '\t', '\n', '\x0b', '\x0c', '\r']).filter(|w| !w.is_empty()).map(String::from).skip(1).collect();
    for word in words.iter_mut() {
        if ignores.iter().any(|i| i.eq_ignore_ascii_case(word)) {
            continue;
        }
        *word = substitute(word, replacements);
        break;
    }
    words.join(" ")
}

fn substitute(word: &str, replacements: &Map) -> String {
    if let Some(v) = replacements.get(word) {
        return v.as_string().unwrap_or_default();
    }
    let append = |w: &str, suffix: &str| {
        let suffix = if uppercase(w) { suffix.to_uppercase() } else { suffix.to_string() };
        format!("{w}{suffix}")
    };
    if ES_SUFFIX.is_match(word) {
        append(word, "es")
    } else if IES_SUFFIX.is_match(word) {
        let mut chars: Vec<char> = word.chars().collect();
        chars.pop();
        append(&chars.into_iter().collect::<String>(), "ies")
    } else {
        append(word, "s")
    }
}

/// `preprocess(message)`: strip, squeeze spaces, downcase.
fn preprocess(s: &str) -> String {
    let t = helpers::ruby_string::strip(s);
    let mut out = String::new();
    let mut prev_space = false;
    for c in t.chars() {
        if c == ' ' && prev_space {
            continue;
        }
        prev_space = c == ' ';
        out.push(c);
    }
    out.to_lowercase()
}

impl Cop for ExampleWording {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (block (send _ :it ${(str $_) (dstr (str $_) ...)} ...) ...)
        let Some(call) = helpers::rspec::plain_block_call(node) else { return };
        if is_csend(&call) || method_name(&call) != "it" {
            return;
        }
        let Some(desc) = send_args(&call).into_iter().next() else { return };
        if !helpers::rspec::is_str(&desc) && !helpers::rspec::is_dstr(&desc) {
            return;
        }
        let Some(message) = helpers::rspec::str_or_dstr_head(&desc) else { return };
        let (ns, ne) = helpers::rspec::node_range(&desc);
        let (ds, de) = (ns + 1, ne - 1);
        let msg = if SHOULD_PREFIX.is_match(&message) {
            "Do not use should when describing your tests."
        } else if WILL_PREFIX.is_match(&message) {
            "Do not use the future tense when describing your tests."
        } else if IT_PREFIX.is_match(&message) {
            "Do not repeat 'it' when describing your tests."
        } else {
            let text = helpers::rspec::dstr_value(&desc);
            let disallowed = match cx.cfg.get("DisallowedExamples") {
                Some(Value::Seq(items)) => items.iter().filter_map(|v| v.as_string()).any(|e| preprocess(&e) == preprocess(&text)),
                _ => false,
            };
            if disallowed {
                cx.add_with(ds, de, "Your example description is insufficient.", false);
            }
            return;
        };
        if is_heredoc(&desc) {
            cx.add_fix(ds, de, msg, |_| {});
            return;
        }
        let text = helpers::rspec::dstr_value(&desc);
        let replacement = if SHOULD_PREFIX.is_match(&text) || WILL_PREFIX.is_match(&text) {
            let ignores = cx.cfg.strings("IgnoredWords");
            let custom = cx.cfg.get("CustomTransform").and_then(Value::as_map).cloned().unwrap_or_default();
            rewrite(&text, &ignores, &custom)
        } else {
            IT_PREFIX.replace(&text, "").into_owned()
        };
        if !helpers::rspec::docstring_inert(&desc) {
            cx.add_withheld_fix(ds, de, msg);
            return;
        }
        cx.add_fix(ds, de, msg, |c| c.replace(ds, de, &replacement));
    }
}
