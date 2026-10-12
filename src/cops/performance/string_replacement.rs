use crate::helpers::regexp_metacharacter::{interpret_string_escapes, literal_before_final_newline};
use crate::prelude::*;

pub const NAME: &str = "Performance/StringReplacement";
pub static COP: StringReplacement = StringReplacement;
pub struct StringReplacement;

fn str_lit(node: &Node) -> bool {
    node.as_string_node().is_some() && !is_parser_dstr(node)
}

/// first_source for a regexp literal: its single-line text when it has one `str` part and no
/// options (anything else is accepted, i.e. skipped).
fn regexp_source<'pr>(node: &Node<'pr>) -> Option<&'pr str> {
    let re = node.as_regular_expression_node()?;
    if re.closing_loc().as_slice().len() != 1 {
        return None;
    }
    helpers::regexp_metacharacter::regexp_str(node)
}

impl Cop for StringReplacement {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let method = method_name(&call);
        if method != "gsub" && method != "gsub!" {
            return;
        }
        let args = send_args(&call);
        let [first, second] = args.as_slice() else { return };
        if !str_lit(second) {
            return;
        }
        // ${regexp str (send (const nil? :Regexp) {:new :compile} _)}
        let first_source: Vec<u8> = if str_lit(first) {
            first.as_string_node().unwrap().unescaped().to_vec()
        } else {
            let raw = if first.as_regular_expression_node().is_some() || first.as_interpolated_regular_expression_node().is_some() {
                regexp_source(first).map(str::to_string)
            } else if let Some(c) = first.as_call_node() {
                let regexp_ctor = !is_csend(&c)
                    && matches!(method_name(&c), "new" | "compile")
                    && c.receiver().and_then(|r| r.as_constant_read_node()).is_some_and(|r| r.name().as_slice() == b"Regexp")
                    && c.block().is_none();
                if !regexp_ctor {
                    return;
                }
                let ctor_args = call_args(&c);
                let [inner] = ctor_args.as_slice() else { return };
                if str_lit(inner) {
                    // A string's value, unescaped.
                    match String::from_utf8(inner.as_string_node().unwrap().unescaped().to_vec()) {
                        Ok(s) => Some(s),
                        Err(_) => return,
                    }
                } else {
                    regexp_source(inner).map(str::to_string)
                }
            } else {
                return;
            };
            let Some(raw) = raw else { return };
            if !literal_before_final_newline(&raw) {
                return;
            }
            interpret_string_escapes(&raw)
        };
        let second_value = second.as_string_node().unwrap().unescaped().to_vec();
        let chars = |b: &[u8]| String::from_utf8_lossy(b).chars().count();
        if chars(&second_value) > 1 || chars(&first_source) != 1 {
            return;
        }
        let delete = second_value.is_empty();
        let bang = if method.ends_with('!') { "!" } else { "" };
        let replacement = format!("{}{bang}", if delete { "delete" } else { "tr" });
        let sel = call.message_loc().unwrap();
        let end = send_range(&call).1;
        let first_is_str = str_lit(first);
        let first_span = (first.location().start_offset(), first.location().end_offset());
        let literal = helpers::percent_array::safe_string_literal(&first_source);
        let suffix = call.closing_loc().map_or(String::new(), |c| text(&c).to_string());
        let msg = format!("Use `{replacement}` instead of `{method}`.");
        cx.add_fix(sel.start_offset(), end, msg, |c| {
            c.replace(sel.start_offset(), sel.end_offset(), &replacement);
            if !first_is_str {
                c.replace(first_span.0, first_span.1, &literal);
            }
            if delete {
                c.replace(first_span.1, end, &suffix);
            }
        });
    }
}
