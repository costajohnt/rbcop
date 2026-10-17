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
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        let list = |obsolete: &str, key: &str| -> Vec<String> {
            match cx.cfg.get(obsolete) {
                Some(v) if !matches!(v, Value::Null | Value::Bool(false)) => cx.cfg.strings(obsolete),
                _ => cx.cfg.strings(key),
            }
        };
        if list("Whitelist", "AllowedMethods").iter().any(|m| m == name) {
            return;
        }
        if !list("Blacklist", "ForbiddenMethods").iter().any(|m| m == name) {
            return;
        }
        let args = send_args(&call);
        if WITH_ARGUMENTS.contains(&name) && args.is_empty() {
            return;
        }
        // good_touch?
        if !is_csend(&call) && name == "touch" {
            if call.receiver().is_some_and(|r| helpers::rails::is_const(&r, "FileUtils")) {
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
        let sel = call.message_loc().unwrap();
        cx.add(sel.start_offset(), sel.end_offset(), format!("Avoid using `{name}` because it skips validations."));
    }
}
