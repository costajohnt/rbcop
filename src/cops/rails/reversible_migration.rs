use crate::prelude::*;

pub const NAME: &str = "Rails/ReversibleMigration";
pub static COP: ReversibleMigration = ReversibleMigration;
pub struct ReversibleMigration;

/// all_hash_key?: `args` is a hash whose keys include all of `keys`.
fn all_hash_key(args: Option<&Node>, keys: &[&str]) -> bool {
    let Some(h) = args.filter(|a| helpers::rails::hash_elements(a).is_some()) else { return false };
    let names: Vec<String> = helpers::rails::hash_pairs(h).iter().filter_map(|p| helpers::rails::str_or_sym(&p.key())).collect();
    keys.iter().all(|k| names.iter().any(|n| n == k))
}

fn is_hash(n: &Node) -> bool {
    helpers::rails::hash_elements(n).is_some()
}

/// reversible_change_table_call?
fn reversible_call(call: &ruby_prism::CallNode, rails: f64) -> bool {
    let args = send_args(call);
    match method_name(call) {
        "change" => false,
        "remove" => rails >= 6.1 && all_hash_key(args.last(), &["type"]),
        "remove_index" => args.iter().any(|a| !is_hash(a)) || all_hash_key(args.last(), &["column"]),
        "change_default" | "change_column_default" | "change_table_comment" | "change_column_comment" => {
            all_hash_key(args.last(), &["from", "to"])
        }
        _ => true,
    }
}

/// within_reversible_or_up_only_block?: a parser `block` ancestor whose send is `reversible`, or
/// any block ancestor whose send is `up_only`. `own` counts the node's own block (a send's
/// parser parent).
fn in_reversible(anc: &[Node], own: Option<(&ruby_prism::CallNode, &Node)>) -> bool {
    let check = |call: &ruby_prism::CallNode, block: &Node| {
        let numbered = block
            .as_block_node()
            .and_then(|b| b.parameters())
            .is_some_and(|p| p.as_numbered_parameters_node().is_some() || p.as_it_parameters_node().is_some());
        (!numbered && method_name(call) == "reversible") || method_name(call) == "up_only"
    };
    own.is_some_and(|(c, b)| check(c, b))
        || (1..anc.len()).any(|i| anc[i].as_block_node().is_some() && anc[i - 1].as_call_node().is_some_and(|c| check(&c, &anc[i])))
}

fn msg(action: &str) -> String {
    format!("{action} is not reversible.")
}

impl Cop for ReversibleMigration {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        let anc = cx.ancestors;
        let in_change = anc.iter().any(|a| a.as_def_node().is_some_and(|d| d.receiver().is_none() && d.name().as_slice() == b"change"));
        if !helpers::rails::in_migration(anc) || !in_change {
            return;
        }
        let rails = cx.config.target_rails_version();
        let block = call.block().filter(|b| b.as_block_node().is_some());
        // on_block (the block is visited before its send).
        if let Some(b) = &block
            && !in_reversible(anc, None)
            && let Some(body) = b.as_block_node().unwrap().body()
            && helpers::rails::is_command(&call, "change_table")
            && let Some(table) = send_args(&call).first()
        {
            let stmts = helpers::rails::body_statements(Some(body));
            // ponytail: a lone non-send statement (an `if`, ...) is not searched for child sends.
            let sends: Vec<ruby_prism::CallNode> = stmts.iter().filter_map(helpers::rails::send_node).collect();
            let table_key = helpers::rails::node_key(cx.src, table);
            for s in &sends {
                let same = s.receiver().is_some_and(|r| helpers::rails::node_key(cx.src, &r) == table_key);
                if !same && reversible_call(s, rails) {
                    continue;
                }
                let m = method_name(s);
                let action = if m == "remove" {
                    if rails >= 6.1 { "t.remove (without type)".to_string() } else { "t.remove".to_string() }
                } else {
                    format!("change_table(with {m})")
                };
                let (a, z) = send_range(s);
                cx.add(a, z, msg(&action));
            }
        }
        if in_reversible(anc, block.as_ref().map(|b| (&call, b))) {
            return;
        }
        let (s, e) = send_range(&call);
        let name = method_name(&call);
        let args = send_args(&call);
        let bare = call.receiver().is_none();
        if bare && matches!(name, "change_column" | "execute") {
            cx.add(s, e, msg(name));
        }
        if bare && name == "drop_table" && let Some(last) = args.last() {
            let parent_block = call.block().is_some_and(|b| b.as_block_node().is_some())
                || helpers::rails::parent_index(anc).is_some_and(|p| anc[p].as_block_node().is_some() || anc[p].as_lambda_node().is_some());
            if !parent_block && last.as_block_argument_node().is_none() {
                cx.add(s, e, msg("drop_table(without block)"));
            }
        }
        if !reversible_call(&call, rails) {
            cx.add(s, e, msg(&format!("{name}(without :from and :to)")));
        }
        if !bare {
            return;
        }
        match name {
            "remove_column" if args.len() < 3 => cx.add(s, e, msg("remove_column(without type)")),
            "remove_foreign_key" if args.len() == 2 && is_hash(&args[1]) && !all_hash_key(Some(&args[1]), &["to_table"]) => {
                cx.add(s, e, msg("remove_foreign_key(without table)"))
            }
            "remove_columns" if !args.is_empty() && !(all_hash_key(args.last(), &["type"]) && rails >= 6.1) => {
                cx.add(s, e, msg(if rails >= 6.1 { "remove_columns(without type)" } else { "remove_columns" }))
            }
            "remove_index" if args.len() == 2 && is_hash(&args[1]) && !all_hash_key(Some(&args[1]), &["column"]) => {
                cx.add(s, e, msg("remove_index(without column)"))
            }
            _ => {}
        }
    }
}
