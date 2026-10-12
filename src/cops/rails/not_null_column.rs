use crate::prelude::*;

pub const NAME: &str = "Rails/NotNullColumn";
pub static COP: NotNullColumn = NotNullColumn;
pub struct NotNullColumn;

/// check_pairs: unless a non-nil `default:` is given, the first `null: false` pair.
fn check_pairs(cx: &mut Cx, hash: &Node) {
    let pairs = helpers::rails::hash_pairs(hash);
    if pairs.iter().any(|p| helpers::rails::sym_value(&p.key()).as_deref() == Some("default") && p.value().as_nil_node().is_none()) {
        return;
    }
    if let Some(p) = pairs.iter().find(|p| helpers::rails::sym_value(&p.key()).as_deref() == Some("null") && p.value().as_false_node().is_some()) {
        cx.add(p.location().start_offset(), p.location().end_offset(), "Do not add a NOT NULL column without a default value.");
    }
}

/// check_column: virtual columns and (on MySQL) text columns are skipped.
fn check_column(cx: &mut Cx, ty: Option<&Node>, hash: &Node) {
    if let Some(t) = ty
        && let Some(v) = helpers::rails::str_or_sym(t)
    {
        if v == "virtual" {
            return;
        }
        if v == "text" && helpers::rails::database(cx).as_deref() == Some("mysql") {
            return;
        }
    }
    check_pairs(cx, hash);
}

impl Cop for NotNullColumn {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        let args = send_args(&call);
        let is_hash = |n: &Node| helpers::rails::hash_elements(n).is_some();
        if call.receiver().is_none() {
            match (method_name(&call), args.as_slice()) {
                ("add_column", [_, _, ty, h]) if is_hash(h) => check_column(cx, Some(ty), h),
                ("add_reference", [_, _, h]) if is_hash(h) => check_pairs(cx, h),
                _ => {}
            }
        }
        // on_block: (block (send nil? :change_table ...) (args (arg $_)) _)
        if !helpers::rails::is_command(&call, "change_table") {
            return;
        }
        let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return };
        let Some(params) = block.parameters().and_then(|p| p.as_block_parameters_node()) else { return };
        let Some(ps) = params.parameters() else { return };
        let nodes = parameter_nodes(&ps);
        let [only] = nodes.as_slice() else { return };
        let Some(table) = only.as_required_parameter_node().map(|r| r.name().as_slice().to_vec()) else { return };
        if params.locals().iter().next().is_some() {
            return;
        }
        let Some(body) = block.body() else { return };
        for child in helpers::rails::body_statements(Some(body)) {
            let Some(c) = helpers::rails::send_node(&child) else { continue };
            let on_table = c.receiver().is_some_and(|r| r.as_local_variable_read_node().is_some_and(|l| l.name().as_slice() == table.as_slice()));
            if !on_table {
                continue;
            }
            let a = send_args(&c);
            let m = method_name(&c);
            // (send (lvar $_) :column _ $_ (hash $...))
            if m == "column"
                && let [_, ty, h] = a.as_slice()
                && is_hash(h)
            {
                check_column(cx, Some(ty), h);
            }
            // (send (lvar $_) $_ _ (hash $...)): the method name is the "type" (no `value`).
            if let [_, h] = a.as_slice()
                && is_hash(h)
            {
                check_column(cx, None, h);
            }
            // (send (lvar $_) :add_reference _ _ (hash $...))
            if m == "add_reference"
                && let [_, _, h] = a.as_slice()
                && is_hash(h)
            {
                check_pairs(cx, h);
            }
        }
    }
}
