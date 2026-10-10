use crate::prelude::*;

pub const NAME: &str = "Style/SendWithLiteralMethodName";
pub static COP: SendWithLiteralMethodName = SendWithLiteralMethodName;
pub struct SendWithLiteralMethodName;

static METHOD_NAME_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A[a-zA-Z_][a-zA-Z0-9_]*[!?]?\z").unwrap());

const RESERVED_WORDS: &[&str] = &[
    "BEGIN", "END", "alias", "and", "begin", "break", "case", "class", "def", "defined?", "do", "else", "elsif", "end",
    "ensure", "false", "for", "if", "in", "module", "next", "nil", "not", "or", "redo", "rescue", "retry", "return",
    "self", "super", "then", "true", "undef", "unless", "until", "when", "while", "yield",
];

impl Cop for SendWithLiteralMethodName {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let method = method_name(&call);
        if !matches!(method, "public_send" | "send" | "__send__") {
            return;
        }
        if cx.cfg.bool("AllowSend").unwrap_or(false) && method != "public_send" {
            return;
        }
        let args = send_args(&call);
        let Some(first) = args.first() else { return };
        // STATIC_METHOD_NAME_NODE_TYPES: parser `sym` and `str` (a multi-line literal is a `dstr`).
        // A `str` value is a String, never in the Symbol list RESERVED_WORDS.
        let (value, reserved_check) = if let Some(s) = first.as_symbol_node() {
            (s.unescaped().to_vec(), true)
        } else if let Some(s) = first.as_string_node().filter(|_| !is_parser_dstr(first)) {
            (s.unescaped().to_vec(), false)
        } else {
            return;
        };
        let Ok(name) = std::str::from_utf8(&value) else { return };
        if !METHOD_NAME_PATTERN.is_match(name) || (reserved_check && RESERVED_WORDS.contains(&name)) {
            return;
        }
        let Some(sel) = call.message_loc() else { return };
        let (start, end) = (sel.start_offset(), send_range(&call).1);
        let (first_start, sel_end) = (first.location().start_offset(), sel.end_offset());
        let second_start = args.get(1).map(|a| a.location().start_offset());
        let name = name.to_string();
        cx.add_fix(start, end, format!("Use `{name}` method call directly instead."), |c| match second_start {
            None => c.replace(start, end, &name),
            Some(second) => {
                c.replace(start, sel_end, &name);
                c.remove(first_start, second);
            }
        });
    }
}
