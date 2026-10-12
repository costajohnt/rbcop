use crate::prelude::*;

pub const NAME: &str = "RSpecRails/InferredSpecType";
pub static COP: InferredSpecType = InferredSpecType;
pub struct InferredSpecType;

impl Cop for InferredSpecType {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // describe_with_type: (block (send #rspec? #ExampleGroups.all ... (hash <$(pair (sym :type) sym) ...>)) ...)
        let Some(call) = helpers::rspec::plain_block_call(node) else { return };
        let l = helpers::rspec::lang(cx);
        if !helpers::rspec::rspec_recv(&call) || !l.example_group(method_name(&call)) {
            return;
        }
        let args = send_args(&call);
        let Some(hash) = args.last() else { return };
        let Some(elems) = helpers::rails::hash_elements(hash) else { return };
        let Some(i) = elems.iter().position(|e| {
            e.as_assoc_node().is_some_and(|p| {
                helpers::rspec::is_sym(cx, &p.key())
                    && helpers::rails::sym_value(&p.key()).as_deref() == Some("type")
                    && helpers::rspec::is_sym(cx, &p.value())
            })
        }) else {
            return;
        };
        let pair = elems[i].as_assoc_node().unwrap();
        // inferred_type?: the type the file path implies, inspected, is the value's source
        let path = cx.src.absolute_path();
        let inferences = cx.cfg.get("Inferences").and_then(Value::as_map).cloned().unwrap_or_default();
        let inferred = inferences
            .0
            .iter()
            .find(|(prefix, _)| path.contains(&format!("spec/{prefix}/")))
            .and_then(|(_, t)| t.as_string());
        let Some(inferred) = inferred else { return };
        if helpers::ruby_string::symbol_inspect(inferred.as_bytes()) != node_text(&pair.value()) {
            return;
        }
        let pairs = elems.iter().filter(|e| e.as_assoc_node().is_some()).count();
        // detect_removable_node and remove_range
        let (target, remove) = if pairs == 1 {
            let h = helpers::rspec::node_range(hash);
            // the hash's left sibling: the previous argument (the method name when it is the first,
            // which raises)
            if args.len() < 2 {
                return;
            }
            let prev = helpers::rspec::node_range(&args[args.len() - 2]);
            (h, (prev.1, h.1))
        } else {
            let p = helpers::rspec::node_range(&elems[i]);
            if i > 0 {
                (p, (helpers::rspec::node_range(&elems[i - 1]).1, p.1))
            } else if let Some(next) = elems.get(i + 1) {
                (p, (p.0, helpers::rspec::node_range(next).0))
            } else {
                return;
            }
        };
        cx.add_fix(target.0, target.1, "Remove redundant spec type.", |c| c.remove(remove.0, remove.1));
    }
}
