use crate::prelude::*;
use helpers::range_help::{Surrounding, range_with_surrounding_space};
use helpers::ruby_string::{float_to_s, inspect};

pub const NAME: &str = "Style/RedundantArgument";
pub static COP: RedundantArgument = RedundantArgument;
pub struct RedundantArgument;

/// `arg.inspect` of a config value.
fn value_inspect(v: &Value) -> Option<String> {
    Some(match v {
        Value::Null => "nil".into(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => float_to_s(*f),
        Value::Str(s) => inspect(s.as_bytes()),
        _ => return None,
    })
}

/// What `argument_matched?` compares: `value.inspect` for nodes with a `value` (str, dstr, int,
/// float), the source otherwise. Inspected config values never contain control characters, so
/// `exclude_cntrl_character?` is always true.
fn argument_text(arg: &Node) -> String {
    if let Some(s) = arg.as_string_node() {
        return inspect(s.unescaped());
    }
    if let Some(d) = arg.as_interpolated_string_node() {
        let mut value = Vec::new();
        for p in d.parts().iter() {
            match p.as_string_node() {
                Some(s) => value.extend_from_slice(s.unescaped()),
                None => value.extend_from_slice(node_text(&p).as_bytes()),
            }
        }
        return inspect(&value);
    }
    if let Some(v) = int_literal_value(arg) {
        return v.to_string();
    }
    if let Some(f) = arg.as_float_node() {
        return float_to_s(f.value());
    }
    node_text(arg).to_string()
}

impl Cop for RedundantArgument {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if !matches!(name, "exit" | "exit!") && call.receiver().is_none() {
            return;
        }
        let args = send_args(&call);
        if args.len() != 1 {
            return;
        }
        let Some(expected) = cx.cfg.get("Methods").and_then(|m| m.as_map()).and_then(|m| m.get(name)) else { return };
        let Some(expected) = value_inspect(expected) else { return };
        if argument_text(&args[0]) != expected {
            return;
        }
        let (start, end) = match (call.opening_loc(), call.closing_loc()) {
            (Some(o), Some(c)) => (o.start_offset(), c.end_offset()),
            _ => {
                let l = args[0].location();
                range_with_surrounding_space(
                    cx.src.text,
                    l.start_offset(),
                    l.end_offset(),
                    Surrounding { newlines: false, ..Default::default() },
                )
            }
        };
        let msg = format!("Argument {} is redundant because it is implied by default.", node_text(&args[0]));
        cx.add_fix(start, end, msg, |c| c.remove(start, end));
    }
}
