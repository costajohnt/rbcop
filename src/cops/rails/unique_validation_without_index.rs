use crate::prelude::*;
use std::collections::BTreeSet;

pub const NAME: &str = "Rails/UniqueValidationWithoutIndex";
pub static COP: UniqueValidationWithoutIndex = UniqueValidationWithoutIndex;
pub struct UniqueValidationWithoutIndex;

/// The value of the first pair whose key is the symbol `key`.
fn sym_pair<'pr>(hash: &Node<'pr>, key: &str) -> Option<Node<'pr>> {
    helpers::rails::hash_pairs(hash)
        .iter()
        .find(|p| p.key().as_symbol_node().is_some() && helpers::rails::sym_value(&p.key()).as_deref() == Some(key))
        .map(helpers::rails::pair_value)
}

fn has_sym_key(hash: &Node, keys: &[&str]) -> bool {
    helpers::rails::hash_pairs(hash)
        .iter()
        .any(|p| p.key().as_symbol_node().is_some() && helpers::rails::sym_value(&p.key()).is_some_and(|k| keys.contains(&k.as_str())))
}

/// A plain `str` or `sym` node's value.
fn str_sym(n: &Node) -> Option<String> {
    if n.as_string_node().is_some() { helpers::rails::str_value(n) } else { helpers::rails::sym_value(n) }
}

/// column_names_from_scope
fn scope_names(uniqueness: &Node) -> Option<Vec<String>> {
    helpers::rails::hash_elements(uniqueness)?;
    let mut scope = sym_pair(uniqueness, "scope")?;
    // unfreeze_scope
    if let Some(c) = helpers::rails::send_node(&scope)
        && method_name(&c) == "freeze"
        && let Some(r) = c.receiver()
    {
        scope = r;
    }
    if let Some(v) = str_sym(&scope) {
        return Some(vec![v]);
    }
    scope.as_array_node()?.elements().iter().map(|e| str_sym(&e)).collect()
}

impl Cop for UniqueValidationWithoutIndex {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        if method_name(&call) != "validates" {
            return;
        }
        let Some(schema) = helpers::schema::schema() else { return };
        let args = send_args(&call);
        let Some(options) = args.last().filter(|l| helpers::rails::hash_elements(l).is_some()) else { return };
        let Some(uniqueness) = sym_pair(options, "uniqueness") else { return };
        if uniqueness.as_false_node().is_some() || uniqueness.as_nil_node().is_some() {
            return;
        }
        // condition_part?
        if has_sym_key(options, &["if", "unless"])
            || (helpers::rails::hash_elements(&uniqueness).is_some() && has_sym_key(&uniqueness, &["if", "unless", "conditions"]))
        {
            return;
        }
        let anc = cx.ancestors;
        let Some(ci) = anc.iter().rposition(|a| a.as_class_node().is_some()) else { return };
        let table_name = helpers::schema::table_name(anc, ci);
        let table = schema.table_by(&table_name);
        // column_names
        let Some(first) = args.first().and_then(str_sym) else { return };
        let mut names = vec![first];
        if let Some(scope) = scope_names(&uniqueness) {
            names.extend(scope);
        }
        let mut columns: BTreeSet<String> = BTreeSet::new();
        for n in &names {
            let Some(resolved) = helpers::schema::resolve_relation_into_column(n, &anc[ci], table) else { return };
            columns.extend(resolved);
        }
        let Some(table) = table else { return };
        let indexed = table.indices.iter().chain(schema.add_indices_by(&table_name)).any(|i| {
            i.unique
                && (i.columns.iter().map(|c| c.clone().unwrap_or_default()).collect::<BTreeSet<String>>() == columns
                    && i.columns.iter().all(Option::is_some)
                    || i.expression.as_ref().is_some_and(|e| columns.iter().all(|c| e.contains(c.as_str()))))
        });
        if indexed {
            return;
        }
        let (s, e) = send_range(&call);
        cx.add(s, e, "Uniqueness validation should have a unique index on the database column.");
    }
}
