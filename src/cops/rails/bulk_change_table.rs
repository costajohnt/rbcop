use crate::prelude::*;

pub const NAME: &str = "Rails/BulkChangeTable";
pub static COP: BulkChangeTable = BulkChangeTable;
pub struct BulkChangeTable;

#[rustfmt::skip]
const COMBINABLE_TRANSFORMATIONS: &[&str] = &[
    "primary_key", "column", "string", "text", "integer", "bigint", "float", "decimal", "numeric", "datetime",
    "timestamp", "time", "date", "binary", "boolean", "json", "virtual", "remove", "change", "timestamps",
    "remove_timestamps",
];
#[rustfmt::skip]
const COMBINABLE_ALTER_METHODS: &[&str] = &[
    "add_column", "remove_column", "remove_columns", "change_column", "add_timestamps", "remove_timestamps",
];

/// The database and the target Rails version decide which methods combine.
fn lists(cx: &Cx) -> Option<(Vec<&'static str>, Vec<&'static str>)> {
    let rails = cx.config.target_rails_version();
    let mut t = COMBINABLE_TRANSFORMATIONS.to_vec();
    let mut a = COMBINABLE_ALTER_METHODS.to_vec();
    match helpers::rails::database(cx).as_deref() {
        Some("mysql") => {
            t.extend(["rename", "index", "remove_index"]);
            a.extend(["rename_column", "add_index", "remove_index"]);
        }
        Some("postgresql") if rails >= 5.2 => {
            t.push("change_default");
            a.push("change_column_default");
            if rails >= 6.1 {
                t.push("change_null");
                a.push("change_column_null");
            }
        }
        _ => return None,
    }
    Some((t, a))
}

/// A BasicLiteralNode's `value.to_s` (sym, str, dstr, numbers); None for anything else.
fn literal(n: Option<&Node>) -> Option<String> {
    helpers::rails::node_value(n?)
}

impl Cop for BulkChangeTable {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE, kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if let Some(d) = node.as_def_node() {
            return on_def(cx, &d);
        }
        let Some(call) = helpers::rails::send(node) else { return };
        // on_send asks support_bulk_alter? (the database) of every send first
        let Some((transformations, _)) = lists(cx) else { return };
        if !helpers::rails::is_command(&call, "change_table") {
            return;
        }
        // include_bulk_options?
        let args = send_args(&call);
        if args.get(1).is_some_and(|o| {
            helpers::rails::hash_pairs(o).iter().any(|p| p.key().as_symbol_node().is_some() && helpers::rails::sym_value(&p.key()).as_deref() == Some("bulk"))
        }) {
            return;
        }
        let Some(body) = call.block().and_then(|b| b.as_block_node()).and_then(|b| b.body()) else { return };
        // ponytail: a lone non-send statement (an `if`, ...) is not searched for child sends.
        let sends: Vec<ruby_prism::CallNode> =
            helpers::rails::body_statements(Some(body)).iter().filter_map(helpers::rails::send_node).collect();
        let count: usize = sends
            .iter()
            .map(|s| {
                if method_name(s) == "remove" {
                    send_args(s).iter().filter(|a| helpers::rails::hash_elements(a).is_none()).count()
                } else {
                    usize::from(transformations.contains(&method_name(s)))
                }
            })
            .sum();
        if count > 1 {
            let (s, e) = send_range(&call);
            cx.add(s, e, "You can combine alter queries using `bulk: true` options.");
        }
    }
}

fn on_def(cx: &mut Cx, d: &ruby_prism::DefNode) {
    if d.receiver().is_some() || !matches!(d.name().as_slice(), b"change" | b"up" | b"down") {
        return;
    }
    let Some((_, alter)) = lists(cx) else { return };
    // node.body.child_nodes: the statements of a multi-statement body.
    let Some(stmts) = d.body().and_then(|b| b.as_statements_node()) else { return };
    let stmts: Vec<Node> = stmts.body().iter().collect();
    if stmts.len() < 2 {
        return;
    }
    // AlterMethodsRecorder
    type Entry = ((usize, usize), String);
    let mut run: Vec<Entry> = Vec::new();
    let mut offensive: Vec<Entry> = Vec::new();
    let flush = |run: &mut Vec<Entry>, off: &mut Vec<Entry>| {
        if run.len() > 1 {
            off.push(run.swap_remove(0));
        }
        run.clear();
    };
    for s in &stmts {
        let Some(c) = helpers::rails::send_node(s).filter(|c| alter.contains(&method_name(c))) else {
            flush(&mut run, &mut offensive);
            continue;
        };
        match literal(send_args(&c).first()) {
            Some(table) => {
                if !run.iter().all(|(_, t)| *t == table) {
                    flush(&mut run, &mut offensive);
                }
                run.push((send_range(&c), table));
            }
            None => flush(&mut run, &mut offensive),
        }
    }
    flush(&mut run, &mut offensive);
    for ((s, e), table) in offensive {
        cx.add(s, e, format!("You can use `change_table :{table}, bulk: true` to combine alter queries."));
    }
}
