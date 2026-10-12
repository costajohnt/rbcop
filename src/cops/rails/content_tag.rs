use crate::prelude::*;
use std::cell::Cell;

pub const NAME: &str = "Rails/ContentTag";
pub static COP: ContentTag = ContentTag;
pub struct ContentTag;

static NAME_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^[a-zA-Z-][a-zA-Z\-0-9]*$").unwrap());

/// Sends whose corrector ran (`@corrected_nodes`).
#[derive(Default)]
struct Corrected(Vec<(usize, usize)>);

/// `value.to_s` for the argument types that respond to `value`; None for the others.
fn value_to_s(n: &Node) -> Option<String> {
    if let Some(v) = helpers::rails::str_or_sym(n) {
        return Some(v);
    }
    if let Some(s) = n.as_string_node() {
        return Some(String::from_utf8_lossy(s.unescaped()).into_owned());
    }
    if let Some(d) = n.as_interpolated_string_node() {
        // DstrNode#value: child values, sources for the others.
        let mut out = String::new();
        for p in d.parts().iter() {
            match p.as_string_node() {
                Some(s) => out.push_str(&String::from_utf8_lossy(s.unescaped())),
                None => out.push_str(node_text(&p)),
            }
        }
        return Some(out);
    }
    if n.as_integer_node().is_some() {
        return int_literal_value(n).map(|i| i.to_string());
    }
    if let Some(f) = n.as_float_node() {
        return Some(helpers::ruby_string::float_to_s(f.value()));
    }
    None
}

impl Cop for ContentTag {
    fn correctable(&self) -> bool {
        true
    }

    fn min_rails(&self) -> f64 {
        5.1
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        if method_name(&call) != "tag" || call.receiver().is_some() {
            return;
        }
        let args = send_args(&call);
        if args.len() >= 3 {
            return;
        }
        let Some(first) = args.first() else { return };
        // allowed_argument?
        let variable = matches!(
            first,
            Node::LocalVariableReadNode { .. }
                | Node::InstanceVariableReadNode { .. }
                | Node::ClassVariableReadNode { .. }
                | Node::GlobalVariableReadNode { .. }
        );
        let send_or_const = helpers::rails::send_node(first).is_some()
            || first.as_constant_read_node().is_some()
            || first.as_constant_path_node().is_some()
            || first.as_splat_node().is_some();
        if variable || send_or_const {
            return;
        }
        let str_or_sym = helpers::rails::str_or_sym(first);
        if str_or_sym.as_ref().is_some_and(|v| !NAME_RE.is_match(v)) {
            return;
        }
        let Some(value) = value_to_s(first) else { return };
        // corrected_ancestor?: an enclosing send whose corrector ran.
        let corrected = std::mem::take(&mut cx.state::<Corrected>().0);
        let inside = helpers::rails::call_ancestors(cx.ancestors)
            .filter(|c| !is_csend(c))
            .any(|c| corrected.contains(&send_range(&c)));
        cx.state::<Corrected>().0 = corrected;
        if inside {
            return;
        }
        let preferred = helpers::inflector::underscore(&value);
        let msg = format!("Use `tag.{preferred}` instead of `tag({})`.", node_text(first));
        let (s, e) = send_range(&call);
        let sel = call.message_loc().unwrap().start_offset();
        let rest: Vec<&str> = args[1..].iter().map(|a| node_text(a)).collect();
        let replacement = format!("tag.{preferred}({})", rest.join(", "));
        // Deliberate difference: only a plain method name is spliced in as `tag.<name>`.
        static METHOD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A[a-z_][a-zA-Z0-9_]*[!?]?\z").unwrap());
        if !METHOD.is_match(&preferred) {
            let before = cx.out.len();
            cx.add_withheld_fix(s, e, msg);
            if cx.out.len() > before && cx.out.last().unwrap().correctable {
                cx.state::<Corrected>().0.push((s, e));
            }
            return;
        }
        let ran = Cell::new(false);
        cx.add_fix(s, e, msg, |c| {
            c.replace(sel, e, &replacement);
            ran.set(true);
        });
        if ran.get() {
            cx.state::<Corrected>().0.push((s, e));
        }
    }
}
