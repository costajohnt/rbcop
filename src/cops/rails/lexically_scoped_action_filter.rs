use crate::prelude::*;

pub const NAME: &str = "Rails/LexicallyScopedActionFilter";
pub static COP: LexicallyScopedActionFilter = LexicallyScopedActionFilter;
pub struct LexicallyScopedActionFilter;

const FILTERS: &[&str] = &[
    "after_action", "append_after_action", "append_around_action", "append_before_action", "around_action",
    "before_action", "prepend_after_action", "prepend_around_action", "prepend_before_action", "skip_after_action",
    "skip_around_action", "skip_before_action", "skip_action_callback",
];

/// `value` of a sym or str node (`str_content` for strs).
fn sym_or_str(n: &Node) -> Option<String> {
    helpers::rails::str_or_sym(n)
}

impl Cop for LexicallyScopedActionFilter {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        if call.receiver().is_some() || !FILTERS.contains(&method_name(&call)) {
            return;
        }
        let args = send_args(&call);
        let [_, hash] = args.as_slice() else { return };
        let Some(elems) = helpers::rails::hash_elements(hash) else { return };
        let [only] = elems.as_slice() else { return };
        let Some(pair) = only.as_assoc_node() else { return };
        if !helpers::rails::sym_value(&pair.key()).is_some_and(|k| k == "only" || k == "except") {
            return;
        }
        let methods_node = pair.value();
        // node.each_ancestor(:class, :module).first
        let Some(parent) = cx.ancestors.iter().rev().find(|a| a.as_class_node().is_some() || a.as_module_node().is_some()) else {
            return;
        };
        let (body, ty) = match parent.as_class_node() {
            Some(c) => (c.body(), "class"),
            None => (parent.as_module_node().unwrap().body(), "module"),
        };
        // parent.each_child_node(:begin).first: a multi-statement body.
        let stmts = helpers::rails::body_statements(body);
        let block: &[Node] = if stmts.len() > 1 { &stmts } else { &[] };
        let mut defined: Vec<String> = block
            .iter()
            .filter_map(|n| n.as_def_node())
            .filter(|d| d.receiver().is_none())
            .map(|d| String::from_utf8_lossy(d.name().as_slice()).into_owned())
            .collect();
        // delegated_action_methods: (send nil? :delegate (sym $_)+ (hash <(pair (sym :to) _) ...>))
        let mut extra: Vec<String> = Vec::new();
        for n in block {
            let Some(c) = helpers::rails::send_node(n) else { continue };
            if !helpers::rails::is_command(&c, "delegate") {
                continue;
            }
            let a = send_args(&c);
            let Some((last, syms)) = a.split_last() else { continue };
            if syms.is_empty()
                || !syms.iter().all(|s| helpers::rails::sym_value(s).is_some())
                || helpers::rails::hash_value(last, "to").is_none()
            {
                continue;
            }
            extra.extend(syms.iter().filter_map(helpers::rails::sym_value));
        }
        // aliased_action_methods
        // old name -> new name (None when the new name is a String: it never equals a Symbol)
        let mut aliases: Vec<(String, Option<String>)> = Vec::new();
        for n in block {
            if let Some(c) = helpers::rails::send_node(n)
                && method_name(&c) == "alias_method"
            {
                let a = send_args(&c);
                let (Some(first), Some(last)) = (a.first(), a.last()) else { return };
                let (Some(new), Some(old)) = (sym_or_str(first), sym_or_str(last)) else { return };
                // A str key never matches a def name (a Symbol).
                if last.as_symbol_node().is_some() {
                    aliases.retain(|(o, _)| *o != old);
                    aliases.push((old, first.as_symbol_node().map(|_| new)));
                }
            } else if let Some(al) = n.as_alias_method_node() {
                let (Some(new), Some(old)) = (helpers::rails::sym_value(&al.new_name()), helpers::rails::sym_value(&al.old_name())) else {
                    continue;
                };
                aliases.retain(|(o, _)| *o != old);
                aliases.push((old, Some(new)));
            }
        }
        let aliased: Vec<String> =
            defined.iter().filter_map(|d| aliases.iter().find(|(o, _)| o == d).and_then(|(_, n)| n.clone())).collect();
        defined.extend(extra);
        defined.extend(aliased);
        // array_values
        let values: Vec<String> = if let Some(a) = methods_node.as_array_node() {
            a.elements().iter().filter_map(|e| sym_or_str(&e)).collect()
        } else {
            sym_or_str(&methods_node).into_iter().collect()
        };
        let unmatched: Vec<String> = values.into_iter().filter(|v| !defined.contains(v)).collect();
        if unmatched.is_empty() {
            return;
        }
        let action = if unmatched.len() == 1 {
            format!("`{}` is", unmatched[0])
        } else {
            format!("`{}` are", unmatched.join("`, `"))
        };
        let (s, e) = send_range(&call);
        cx.add(s, e, format!("{action} not explicitly defined on the {ty}."));
    }
}
