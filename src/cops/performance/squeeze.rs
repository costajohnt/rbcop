use crate::helpers::regexp_metacharacter::{interpret_string_escapes, literal, regexp_str};
use crate::prelude::*;

pub const NAME: &str = "Performance/Squeeze";
pub static COP: Squeeze = Squeeze;
pub struct Squeeze;

impl Cop for Squeeze {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (call $!nil? ${:gsub :gsub!} (regexp (str $#repeating_literal?) (regopt)) (str $_))
        let call = node.as_call_node().unwrap();
        let bad = method_name(&call);
        if bad != "gsub" && bad != "gsub!" {
            return;
        }
        let Some(recv) = call.receiver() else { return };
        let args = send_args(&call);
        let [re, replace] = args.as_slice() else { return };
        let Some(re_str) = regexp_str(re) else { return };
        // /\A(?:#{LITERAL_REGEX})\+\z/: one literal unit, then `+`.
        let Some(unit) = re_str.strip_suffix('+').filter(|u| literal(u) && is_single_unit(u)) else { return };
        let Some(s) = replace.as_string_node().filter(|_| !is_parser_dstr(replace)) else { return };
        let value = s.unescaped().to_vec();
        if interpret_string_escapes(unit) != value {
            return;
        }
        let good = if bad == "gsub" { "squeeze" } else { "squeeze!" };
        let sel = call.message_loc().unwrap();
        let dot = call.call_operator_loc().map_or("", |d| text(&d));
        let new_code = format!(
            "{}{dot}{good}({})",
            node_text(&recv),
            helpers::percent_array::safe_string_literal(&value)
        );
        let (start, end) = send_range(&call);
        cx.add_fix(sel.start_offset(), sel.end_offset(), format!("Use `{good}` instead of `{bad}`."), |c| {
            c.replace(start, end, &new_code)
        });
    }
}

/// Exactly one LITERAL_REGEX unit: one character, or a backslash and one character.
fn is_single_unit(s: &str) -> bool {
    let n = s.chars().count();
    n == 1 || (n == 2 && s.starts_with('\\'))
}
