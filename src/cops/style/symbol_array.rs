use crate::helpers::percent_array as pa;
use crate::prelude::*;

pub const NAME: &str = "Style/SymbolArray";
pub static COP: SymbolArray = SymbolArray;
pub struct SymbolArray;

const PERCENT_MSG: &str = "Use `%i` or `%I` for an array of symbols.";
const DELIMITERS: &[&str] = &["[", "]", "(", ")"];
const SPECIAL_GVARS: &[&str] = &[
    "$!", "$\"", "$$", "$&", "$'", "$*", "$+", "$,", "$/", "$;", "$:", "$.", "$<", "$=", "$>", "$?", "$@", "$\\",
    "$_", "$`", "$~", "$0", "$-0", "$-F", "$-I", "$-K", "$-W", "$-a", "$-d", "$-i", "$-l", "$-p", "$-v", "$-w",
];
const REDEFINABLE_OPERATORS: &[&str] = &[
    "|", "^", "&", "<=>", "==", "===", "=~", ">", ">=", "<", "<=", "<<", ">>", "+", "-", "*", "/", "%", "**", "~",
    "+@", "-@", "[]", "[]=", "`", "!", "!=", "!~",
];

static METHOD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A[a-zA-Z_][a-zA-Z0-9_]*[!?]?\z").unwrap());
static IVAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A@@?[a-zA-Z_][a-zA-Z0-9_]*\z").unwrap());
static NTH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A\$[1-9][0-9]*\z").unwrap());
static GVAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A\$[a-zA-Z_][a-zA-Z0-9_]*\z").unwrap());
static PAIRS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\[[^ \t\n\r\x0c\x0b\[\]]*\])|(\([^ \t\n\r\x0c\x0b()]*\))").unwrap());

fn symbol_without_quote(s: &str) -> bool {
    METHOD.is_match(s)
        || IVAR.is_match(s)
        || NTH.is_match(s)
        || GVAR.is_match(s)
        || SPECIAL_GVARS.contains(&s)
        || REDEFINABLE_OPERATORS.contains(&s)
}

/// `content = *sym` joined: a sym's value, or a dsym's parts' sources.
fn content(node: &Node) -> String {
    if let Some(s) = node.as_symbol_node() {
        return String::from_utf8_lossy(s.unescaped()).into_owned();
    }
    if let Some(d) = node.as_interpolated_symbol_node() {
        return d.parts().iter().map(|p| node_text(&p).to_string()).collect();
    }
    String::new()
}

fn complex_content(node: &ruby_prism::ArrayNode) -> bool {
    for sym in node.elements().iter() {
        if DELIMITERS.contains(&node_text(&sym)) {
            return false;
        }
        let content = content(&sym);
        let without_pairs = PAIRS.replace_all(&content, "");
        if content.contains(' ') || DELIMITERS.iter().any(|d| without_pairs.contains(d)) {
            return true;
        }
    }
    false
}

impl Cop for SymbolArray {
    fn correctable(&self) -> bool {
        true
    }

    fn ruby_range(&self) -> (f64, f64) {
        (2.0, f64::MAX)
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::ARRAY_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let array = node.as_array_node().unwrap();
        let open = array.opening_loc().map(|l| text(&l)).unwrap_or("");
        let percent_style = cx.cfg.str("EnforcedStyle") != Some("brackets");
        let (start, end) = (node.location().start_offset(), node.location().end_offset());
        if open == "[" {
            let els: Vec<Node> = array.elements().iter().collect();
            // `sym_type?`: a symbol spanning lines is a parser `dsym`.
            let sym = |e: &Node| e.as_symbol_node().is_some() && !crate::ast::is_parser_dsym(e, cx.config.target_ruby);
            if els.is_empty() || !els.iter().all(sym) || complex_content(&array) {
                return;
            }
            if pa::allowed_bracket_array(cx, node) || !percent_style {
                return;
            }
            let values: Vec<Vec<u8>> =
                els.iter().map(|e| e.as_symbol_node().unwrap().unescaped().to_vec()).collect();
            let replacement = pa::percent_literal_replacement(cx, node, &values, 'i');
            cx.add_fix(start, end, PERCENT_MSG, |c| c.replace(start, end, &replacement));
        } else if open.starts_with("%i") || open.starts_with("%I") {
            let brackets_required = complex_content(&array);
            if percent_style && !brackets_required {
                return;
            }
            let syms: Vec<String> = array
                .elements()
                .iter()
                .map(|c| {
                    if c.as_interpolated_symbol_node().is_some() {
                        let lit = pa::to_string_literal(node_text(&c).as_bytes());
                        format!(":{}", pa::trim_string_interpolation_escape_character(&lit))
                    } else {
                        let v = c.as_symbol_node().map(|s| s.unescaped().to_vec()).unwrap_or_default();
                        let s = String::from_utf8_lossy(&v);
                        if symbol_without_quote(&s) { format!(":{s}") } else { format!(":{}", pa::to_string_literal(&v)) }
                    }
                })
                .collect();
            let code =
                if syms.is_empty() { "[]".to_string() } else { pa::bracketed_with_whitespace(cx.src, &array, &syms) };
            let msg = pa::bracketed_message(&code, "symbols");
            cx.add_fix(start, end, msg, |c| c.replace(start, end, &code));
        }
    }
}
