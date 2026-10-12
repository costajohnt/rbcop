use crate::prelude::*;

pub const NAME: &str = "Rails/PluralizationGrammar";
pub static COP: PluralizationGrammar = PluralizationGrammar;
pub struct PluralizationGrammar;

const SINGULAR: &[(&str, &str)] = &[
    ("second", "seconds"),
    ("minute", "minutes"),
    ("hour", "hours"),
    ("day", "days"),
    ("week", "weeks"),
    ("fortnight", "fortnights"),
    ("month", "months"),
    ("year", "years"),
    ("byte", "bytes"),
    ("kilobyte", "kilobytes"),
    ("megabyte", "megabytes"),
    ("gigabyte", "gigabytes"),
    ("terabyte", "terabytes"),
    ("petabyte", "petabytes"),
    ("exabyte", "exabytes"),
    ("zettabyte", "zettabytes"),
];

fn correct(m: &str) -> &'static str {
    SINGULAR.iter().find_map(|(s, p)| if m.ends_with('s') { (*p == m).then_some(*s) } else { (*s == m).then_some(*p) }).unwrap_or("")
}

impl Cop for PluralizationGrammar {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        let m = method_name(&call);
        if !SINGULAR.iter().any(|(s, p)| *s == m || *p == m) {
            return;
        }
        let Some(recv) = call.receiver() else { return };
        // literal_number?: int or float; `number.abs == 1` is singular.
        let (shown, one) = if recv.as_integer_node().is_some() {
            let Some(v) = int_literal_value(&recv) else { return };
            (v.to_string(), v.abs() == 1)
        } else if let Some(f) = recv.as_float_node() {
            (helpers::ruby_string::float_to_s(f.value()), f.value().abs() == 1.0)
        } else {
            return;
        };
        let plural = m.ends_with('s');
        if !((one && plural) || (!one && !plural)) {
            return;
        }
        let sel = call.message_loc().unwrap();
        let (ss, se) = (sel.start_offset(), sel.end_offset());
        let fixed = correct(text(&sel));
        let (s, e) = send_range(&call);
        cx.add_fix(s, e, format!("Prefer `{shown}.{}`.", correct(m)), |c| c.replace(ss, se, fixed));
    }
}
