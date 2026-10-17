use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "RSpec/ReceiveCounts";
pub static COP: ReceiveCounts = ReceiveCounts;
pub struct ReceiveCounts;

/// Ruby's String#to_i: leading decimal digits (underscores between them), after an optional sign.
fn ruby_to_i(s: &str) -> i64 {
    let t = s.trim_start();
    let (neg, t) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let mut n: i64 = 0;
    let mut prev_digit = false;
    for c in t.chars() {
        if let Some(d) = c.to_digit(10) {
            n = n.saturating_mul(10).saturating_add(d as i64);
            prev_digit = true;
        } else if c == '_' && prev_digit {
            prev_digit = false;
        } else {
            break;
        }
    }
    if neg { -n } else { n }
}

impl Cop for ReceiveCounts {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send $(send _ {:exactly :at_least :at_most} (int {1 2})) :times)
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "times" || !send_args(&call).is_empty() {
            return;
        }
        let Some(counted) = call.receiver().and_then(|r| helpers::rspec::send_call(&r)).filter(|c| !is_csend(c)) else {
            return;
        };
        let method = method_name(&counted);
        if !matches!(method, "exactly" | "at_least" | "at_most") {
            return;
        }
        let [count] = &send_args(&counted)[..] else { return };
        if count.as_integer_node().is_none() || !matches!(int_literal_value(count), Some(1 | 2)) {
            return;
        }
        // stub?(offending_node.receiver): (send nil? :receive ...) in the chain before it
        let Some(recv) = counted.receiver() else { return };
        struct V(bool);
        impl<'pr> Visit<'pr> for V {
            fn visit_call_node(&mut self, n: &ruby_prism::CallNode<'pr>) {
                self.0 |= n.receiver().is_none() && method_name(n) == "receive";
                ruby_prism::visit_call_node(self, n);
            }
        }
        let mut v = V(false);
        v.visit(&recv);
        if !v.0 {
            return;
        }
        let matcher = if ruby_to_i(node_text(count)) == 1 { "once" } else { "twice" };
        let alternative = if method == "exactly" { format!(".{matcher}") } else { format!(".{method}(:{matcher})") };
        let Some(dot) = counted.call_operator_loc() else { return };
        let (s, e) = (dot.start_offset(), send_range(&call).1);
        let msg = format!("Use `{alternative}` instead of `{}`.", cx.src.slice(s, e));
        cx.add_fix(s, e, msg, |c| c.replace(s, e, &alternative));
    }
}
