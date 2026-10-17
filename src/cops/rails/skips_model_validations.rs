use crate::prelude::*;

pub const NAME: &str = "Rails/SkipsModelValidations";
pub static COP: SkipsModelValidations = SkipsModelValidations;
pub struct SkipsModelValidations;

const WITH_ARGUMENTS: &[&str] = &[
    "decrement!", "decrement_counter", "increment!", "increment_counter", "insert", "insert!", "insert_all",
    "insert_all!", "toggle!", "update_all", "update_attribute", "update_column", "update_columns", "update_counters",
    "upsert", "upsert_all",
];

impl Cop for SkipsModelValidations {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE, kind::CALL_OR_WRITE_NODE, kind::CALL_AND_WRITE_NODE, kind::CALL_OPERATOR_WRITE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // An op-assign on a call (`a.touch ||= 1`) wraps a send in the parser AST, and on_send
        // fires on it: the read name, no arguments.
        let (name, receiver, csend, sel, args) = match kind::of(node) {
            kind::CALL_OR_WRITE_NODE => {
                let n = node.as_call_or_write_node().unwrap();
                (n.read_name(), n.receiver(), n.call_operator_loc(), n.message_loc(), Vec::new())
            }
            kind::CALL_AND_WRITE_NODE => {
                let n = node.as_call_and_write_node().unwrap();
                (n.read_name(), n.receiver(), n.call_operator_loc(), n.message_loc(), Vec::new())
            }
            kind::CALL_OPERATOR_WRITE_NODE => {
                let n = node.as_call_operator_write_node().unwrap();
                (n.read_name(), n.receiver(), n.call_operator_loc(), n.message_loc(), Vec::new())
            }
            _ => {
                let call = node.as_call_node().unwrap();
                (call.name(), call.receiver(), call.call_operator_loc(), call.message_loc(), send_args(&call))
            }
        };
        let Ok(name) = std::str::from_utf8(name.as_slice()) else { return };
        let Some(sel) = sel else { return };
        let csend = csend.is_some_and(|l| l.as_slice() == b"&.");
        // `(obsolete || cop_config[key] || []).include?(name)`: a substring test on a String.
        let listed = |obsolete: &str, key: &str| match cx.cfg.get(obsolete) {
            Some(v) if !matches!(v, Value::Null | Value::Bool(false)) => cx.cfg.includes(obsolete, name),
            _ => cx.cfg.includes_or_empty(key, name),
        };
        if listed("Whitelist", "AllowedMethods") {
            return;
        }
        if !listed("Blacklist", "ForbiddenMethods") {
            return;
        }
        if WITH_ARGUMENTS.contains(&name) && args.is_empty() {
            return;
        }
        // good_touch?
        if !csend && name == "touch" {
            if receiver.is_some_and(|r| helpers::rails::is_const(&r, "FileUtils")) {
                return;
            }
            if matches!(args.as_slice(), [b] if b.as_true_node().is_some() || b.as_false_node().is_some()) {
                return;
            }
        }
        // good_insert?: a second argument that is not a hash, or a hash with a key other than
        // returning/unique_by.
        if matches!(name, "insert" | "insert!") && args.len() >= 2 {
            let second = &args[1];
            let good = match helpers::rails::hash_elements(second) {
                None => true,
                Some(_) => helpers::rails::hash_pairs(second).iter().any(|p| {
                    helpers::rails::sym_value(&p.key()).is_some_and(|k| k != "returning" && k != "unique_by")
                }),
            };
            if good {
                return;
            }
        }
        cx.add(sel.start_offset(), sel.end_offset(), format!("Avoid using `{name}` because it skips validations."));
    }
}
