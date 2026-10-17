use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Rails/ReadWriteAttribute";
pub static COP: ReadWriteAttribute = ReadWriteAttribute;
pub struct ReadWriteAttribute;

/// (send nil? :read_attribute _) / (send nil? :write_attribute _ _)
fn read_write<'pr>(n: &ruby_prism::CallNode<'pr>) -> bool {
    if is_csend(n) || n.receiver().is_some() || n.block().is_some_and(|b| b.as_block_node().is_some()) {
        return false;
    }
    let args = send_args(n);
    matches!((method_name(n), args.len()), ("read_attribute", 1) | ("write_attribute", 2))
}

/// `first_arg.value.to_s` for argument types that have a value.
fn value_to_s(n: &Node) -> Option<String> {
    if let Some(v) = helpers::rails::str_or_sym(n) {
        return Some(v);
    }
    if let Some(s) = n.as_string_node() {
        return Some(String::from_utf8_lossy(s.unescaped()).into_owned());
    }
    if let Some(d) = n.as_interpolated_string_node() {
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
    n.as_float_node().map(|f| helpers::ruby_string::float_to_s(f.value()))
}

/// within_shadowing_method?: the enclosing def is named after the attribute.
fn shadowing(call: &ruby_prism::CallNode, def: Option<&[u8]>) -> bool {
    let args = send_args(call);
    let Some(v) = args.first().and_then(value_to_s) else { return false };
    let Some(def) = def else { return false };
    let name = if method_name(call) == "write_attribute" { format!("{v}=") } else { v };
    def == name.as_bytes()
}

impl Cop for ReadWriteAttribute {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if !read_write(&call) {
            return;
        }
        let def = cx.ancestors.iter().rev().find_map(|a| a.as_def_node()).map(|d| d.name().as_slice().to_vec());
        if shadowing(&call, def.as_deref()) {
            return;
        }
        let args = send_args(&call);
        let replacement = if method_name(&call) == "read_attribute" {
            format!("self[{}]", node_text(&args[0]))
        } else {
            format!("self[{}] = {}", node_text(&args[0]), node_text(&args[1]))
        };
        let (s, e) = send_range(&call);
        let src = cx.src;
        let msg = if src.line_of(s) == src.line_of(e) {
            format!("Prefer `{replacement}`.")
        } else if method_name(&call) == "read_attribute" {
            "Prefer `self[:attr]`.".to_string()
        } else {
            "Prefer `self[:attr] = val`.".to_string()
        };
        // nested_read_write_attribute?: a non-shadowing read/write_attribute below this one.
        struct Nested<'d>(Option<&'d [u8]>, bool);
        impl<'pr> Visit<'pr> for Nested<'_> {
            fn visit_call_node(&mut self, c: &ruby_prism::CallNode<'pr>) {
                if read_write(c) && !shadowing(c, self.0) {
                    self.1 = true;
                }
                ruby_prism::visit_call_node(self, c);
            }
        }
        let mut nested = Nested(def.as_deref(), false);
        for a in &args {
            nested.visit(a);
        }
        let nested = nested.1;
        cx.add_fix(s, e, msg, |c| {
            if !nested {
                c.replace(s, e, &replacement);
            }
        });
    }
}
