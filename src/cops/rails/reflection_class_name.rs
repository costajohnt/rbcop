use crate::prelude::*;

pub const NAME: &str = "Rails/ReflectionClassName";
pub static COP: ReflectionClassName = ReflectionClassName;
pub struct ReflectionClassName;

/// ALLOWED_REFLECTION_CLASS_TYPES: str, dstr, sym.
fn allowed_type(n: &Node) -> bool {
    n.as_string_node().is_some() || n.as_interpolated_string_node().is_some() || n.as_symbol_node().is_some()
}

impl Cop for ReflectionClassName {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send nil? {:has_many :has_one :belongs_to} _ _ ? (hash <$#reflection_class_name ...>))
        let Some(call) = helpers::rails::send(node) else { return };
        if call.receiver().is_some() || !matches!(method_name(&call), "has_many" | "has_one" | "belongs_to") {
            return;
        }
        let args = send_args(&call);
        if !(2..=3).contains(&args.len()) {
            return;
        }
        let hash = args.last().unwrap();
        if helpers::rails::hash_elements(hash).is_none() {
            return;
        }
        let send_value = helpers::rails::send_node;
        let const_recv = |c: &ruby_prism::CallNode| {
            c.receiver().is_some_and(|r| r.as_constant_read_node().is_some() || r.as_constant_path_node().is_some())
        };
        // (pair (sym :class_name) #reflection_class_value?)
        let Some(pair) = helpers::rails::hash_pairs(hash).into_iter().find(|p| {
            helpers::rails::sym_value(&p.key()).as_deref() == Some("class_name") && {
                let v = helpers::rails::pair_value(p);
                match send_value(&v) {
                    Some(c) => method_name(&c) != "to_s" || const_recv(&c),
                    None => !allowed_type(&v),
                }
            }
        }) else {
            return;
        };
        let value = helpers::rails::pair_value(&pair);
        if send_value(&value).is_some_and(|c| !const_recv(&c)) {
            return;
        }
        if let Some(lvar) = value.as_local_variable_read_node()
            && str_assigned(cx.ancestors, lvar.name().as_slice())
        {
            return;
        }
        // const_or_string: {$(const nil? _) (send $(const nil? _) :name) (send $(const nil? _) :to_s)}
        let unscoped = |n: &Node| n.as_constant_read_node().is_some();
        let replacement = if unscoped(&value) {
            Some(node_text(&value).to_string())
        } else {
            send_value(&value).and_then(|c| {
                let r = c.receiver()?;
                (matches!(method_name(&c), "name" | "to_s") && send_args(&c).is_empty() && unscoped(&r))
                    .then(|| node_text(&r).to_string())
            })
        };
        let (vs, ve) = (value.location().start_offset(), value.location().end_offset());
        let (s, e) = (pair.location().start_offset(), pair.location().end_offset());
        cx.add_fix(s, e, "Use a string value for `class_name`.", |c| {
            if let Some(r) = &replacement {
                c.replace(vs, ve, &helpers::ruby_string::inspect(r.as_bytes()));
            }
        });
    }
}

/// str_assigned?: some ancestor has a direct child `lvar = <str/dstr/sym>`.
fn str_assigned(ancestors: &[Node], name: &[u8]) -> bool {
    ancestors.iter().any(|a| {
        let Some(s) = a.as_statements_node() else { return false };
        s.body().iter().any(|st| {
            st.as_local_variable_write_node().is_some_and(|w| {
                let v = w.value();
                w.name().as_slice() == name
                    && (v.as_string_node().is_some() || v.as_interpolated_string_node().is_some() || v.as_symbol_node().is_some())
            })
        })
    })
}
