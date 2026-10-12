use crate::prelude::*;

pub const NAME: &str = "Performance/BigDecimalWithNumericArgument";
pub static COP: BigDecimalWithNumericArgument = BigDecimalWithNumericArgument;
pub struct BigDecimalWithNumericArgument;

const FLOAT_MSG: &str = "Convert float literal to string and pass it to `BigDecimal`.";
const INTEGER_MSG: &str = "Convert string literal to integer and pass it to `BigDecimal`.";

/// `${float_type? str_type?}`: Some(true) for a float, Some(false) for a plain string.
fn numeric_or_str(node: &Node) -> Option<bool> {
    if node.as_float_node().is_some() {
        return Some(true);
    }
    (matches!(kind::of(node), kind::STRING_NODE | kind::SOURCE_FILE_NODE) && !is_parser_dstr(node)).then_some(false)
}

/// The value of a `str` node matches /\A\d+\z/.
fn digits(node: &Node) -> Option<String> {
    let s = node.as_string_node()?;
    let v = s.unescaped();
    (!v.is_empty() && v.iter().all(u8::is_ascii_digit)).then(|| String::from_utf8_lossy(v).into_owned())
}

impl Cop for BigDecimalWithNumericArgument {
    fn correctable(&self) -> bool {
        true
    }

    fn ruby_range(&self) -> (f64, f64) {
        (3.1, f64::MAX)
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) {
            return;
        }
        let args = send_args(&call);
        match (method_name(&call), call.receiver()) {
            // (send nil? :BigDecimal ${float_type? str_type?} ...)
            ("BigDecimal", None) => {
                let Some(first) = args.first() else { return };
                let l = first.location();
                let (start, end) = (l.start_offset(), l.end_offset());
                match numeric_or_str(first) {
                    Some(true) => cx.add_fix(start, end, FLOAT_MSG, |c| c.wrap(start, end, "'", "'")),
                    Some(false) => {
                        if let Some(v) = digits(first) {
                            cx.add_fix(start, end, INTEGER_MSG, |c| c.replace(start, end, &v));
                        }
                    }
                    None => {}
                }
            }
            // (send [!nil? ${float_type? str_type?}] :to_d ...)
            ("to_d", Some(recv)) => {
                let l = recv.location();
                let (start, end) = (l.start_offset(), l.end_offset());
                let (s, e) = send_range(&call);
                match numeric_or_str(&recv) {
                    Some(true) => {
                        let mut parts = vec![format!("'{}'", node_text(&recv))];
                        parts.extend(args.iter().map(|a| node_text(a).to_string()));
                        let replacement = format!("BigDecimal({})", parts.join(", "));
                        cx.add_fix(start, end, FLOAT_MSG, |c| c.replace(s, e, &replacement));
                    }
                    Some(false) => {
                        if let Some(v) = digits(&recv) {
                            let replacement = format!("{v}.to_d");
                            cx.add_fix(start, end, INTEGER_MSG, |c| c.replace(s, e, &replacement));
                        }
                    }
                    None => {}
                }
            }
            _ => {}
        }
    }
}
