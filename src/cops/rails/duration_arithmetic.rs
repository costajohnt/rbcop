use crate::prelude::*;

pub const NAME: &str = "Rails/DurationArithmetic";
pub static COP: DurationArithmetic = DurationArithmetic;
pub struct DurationArithmetic;

const DURATIONS: &[&str] = &[
    "second", "seconds", "minute", "minutes", "hour", "hours", "day", "days", "week", "weeks", "fortnight",
    "fortnights", "month", "months", "year", "years",
];

/// (send (const {nil? cbase} :Time) :current) / (send (send (const {nil? cbase} :Time) :zone) :now)
fn time_current(n: &Node) -> bool {
    let Some(c) = helpers::rails::send_node(n) else { return false };
    if !send_args(&c).is_empty() {
        return false;
    }
    let Some(r) = c.receiver() else { return false };
    match method_name(&c) {
        "current" => helpers::rails::is_const(&r, "Time"),
        "now" => helpers::rails::send_node(&r).is_some_and(|z| {
            method_name(&z) == "zone" && send_args(&z).is_empty() && z.receiver().is_some_and(|t| helpers::rails::is_const(&t, "Time"))
        }),
        _ => false,
    }
}

/// (send { int float (send nil _) } DURATIONS)
fn duration(n: &Node) -> bool {
    let Some(c) = helpers::rails::send_node(n) else { return false };
    if !DURATIONS.contains(&method_name(&c)) || !send_args(&c).is_empty() {
        return false;
    }
    c.receiver().is_some_and(|r| {
        r.as_integer_node().is_some()
            || r.as_float_node().is_some()
            || helpers::rails::send_node(&r).is_some_and(|x| x.receiver().is_none() && send_args(&x).is_empty())
    })
}

impl Cop for DurationArithmetic {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        let op = method_name(&call);
        if !matches!(op, "+" | "-") || !call.receiver().is_some_and(|r| time_current(&r)) {
            return;
        }
        let args = send_args(&call);
        let [d] = args.as_slice() else { return };
        if !duration(d) {
            return;
        }
        let replacement = format!("{}.{}", node_text(d), if op == "-" { "ago" } else { "from_now" });
        let (s, e) = send_range(&call);
        cx.add_fix(s, e, "Do not add or subtract duration.", |c| c.replace(s, e, &replacement));
    }
}
