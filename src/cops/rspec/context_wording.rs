use crate::prelude::*;

pub const NAME: &str = "RSpec/ContextWording";
pub static COP: ContextWording = ContextWording;
pub struct ContextWording;

/// AllowedPattern#cop_config_patterns_values (strings and regexps as configured).
fn configured(cx: &Cx) -> Vec<Value> {
    let mut out = Vec::new();
    for key in ["AllowedPatterns", "IgnoredPatterns"] {
        match cx.cfg.get(key) {
            Some(Value::Seq(items)) => out.extend(items.iter().cloned()),
            None | Some(Value::Null) => {}
            Some(v) => out.push(v.clone()),
        }
    }
    out
}

impl Cop for ContextWording {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (block (send #rspec? { :context :shared_context } $(any_str ...) ...) ...)
        let Some(call) = helpers::rspec::plain_block_call(node) else { return };
        if !matches!(method_name(&call), "context" | "shared_context") || !helpers::rspec::rspec_recv(&call) {
            return;
        }
        let Some(context) = send_args(&call).into_iter().next() else { return };
        let description = match kind::of(&context) {
            kind::STRING_NODE | kind::INTERPOLATED_STRING_NODE => helpers::rspec::dstr_value(&context),
            // xstr: `context.value.value`, its first `str` child
            kind::X_STRING_NODE => String::from_utf8_lossy(context.as_x_string_node().unwrap().unescaped()).into_owned(),
            kind::INTERPOLATED_X_STRING_NODE => {
                let Some(first) = context.as_interpolated_x_string_node().unwrap().parts().iter().next() else { return };
                match first.as_string_node() {
                    Some(s) => String::from_utf8_lossy(s.unescaped()).into_owned(),
                    None => return,
                }
            }
            _ => return,
        };
        // Array(cop_config.fetch('Prefixes', [])), each a String or RuboCop raises
        let prefixes: Vec<String> = match cx.cfg.get("Prefixes") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Seq(items)) => {
                let mut v = Vec::new();
                for i in items.iter() {
                    match i {
                        Value::Str(s) => v.push(s.clone()),
                        _ => return,
                    }
                }
                v
            }
            Some(Value::Str(s)) => vec![s.clone()],
            Some(_) => return,
        };
        let configured = configured(cx);
        let mut sources: Vec<(String, String)> = Vec::new(); // (regexp literal, inspected)
        for v in &configured {
            match v {
                Value::Regex(r) => sources.push((r.clone(), r.clone())),
                v => {
                    let s = v.as_string().unwrap_or_default();
                    let inspected = helpers::ruby_string::inspect(s.as_bytes());
                    let inspected = format!("/{}/", &inspected[1..inspected.len() - 1]);
                    sources.push((s, inspected));
                }
            }
        }
        for p in &prefixes {
            let src = format!("^{}\\b", helpers::ruby_string::regexp_escape(p));
            sources.push((src.clone(), format!("/{}/", src.replace('/', "\\/"))));
        }
        if sources.iter().any(|(r, _)| crate::config::value::ruby_regex(r).is_some_and(|re| re.is_match(&description))) {
            return;
        }
        let msg = if sources.is_empty() {
            "Current settings will always report an offense. Please add allowed words to `Prefixes` or `AllowedPatterns`.".to_string()
        } else {
            let mut inspected: Vec<String> = sources.into_iter().map(|(_, i)| i).collect();
            let patterns = if inspected.len() == 1 {
                inspected.pop().unwrap()
            } else {
                let last = inspected.pop().unwrap();
                inspected.push(format!("or {last}"));
                inspected.join(", ")
            };
            format!("Context description should match {patterns}.")
        };
        let (s, e) = helpers::rspec::node_range(&context);
        cx.add(s, e, msg);
    }
}
