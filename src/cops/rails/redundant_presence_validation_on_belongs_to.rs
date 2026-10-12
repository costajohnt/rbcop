use crate::prelude::*;

pub const NAME: &str = "Rails/RedundantPresenceValidationOnBelongsTo";
pub static COP: RedundantPresenceValidationOnBelongsTo = RedundantPresenceValidationOnBelongsTo;
pub struct RedundantPresenceValidationOnBelongsTo;

const NON_VALIDATION_OPTIONS: &[&str] = &["if", "unless", "on", "allow_blank", "allow_nil", "strict"];

fn range(n: &Node) -> (usize, usize) {
    (n.location().start_offset(), n.location().end_offset())
}

/// A pair `(pair (sym key) _)`'s key name.
fn pair_key(p: &ruby_prism::AssocNode) -> Option<String> {
    helpers::rails::sym_value(&p.key())
}

/// `(send nil? :belongs_to ...)`: its arguments.
fn belongs_to<'pr>(n: &Node<'pr>) -> Option<Vec<Node<'pr>>> {
    let c = helpers::rails::send_node(n)?;
    helpers::rails::is_command(&c, "belongs_to").then(|| send_args(&c))
}

fn is_hash(n: &Node) -> bool {
    helpers::rails::hash_elements(n).is_some()
}

fn has_pair(hash: &Node, f: impl Fn(&str, &Node) -> bool) -> bool {
    helpers::rails::hash_pairs(hash).iter().any(|p| pair_key(p).is_some_and(|k| f(&k, &helpers::rails::pair_value(p))))
}

/// optional?: `(send nil? :belongs_to _ ... #optional_option?)`
fn optional(args: &[Node]) -> bool {
    args.len() >= 2
        && args.last().is_some_and(|h| {
            is_hash(h)
                && (has_pair(h, |k, v| k == "optional" && v.as_true_node().is_some())
                    || has_pair(h, |k, v| k == "required" && v.as_false_node().is_some()))
        })
}

/// belongs_to_for?: the first matching belongs_to in the statement list, by association or by
/// foreign key for `*_id` keys.
fn belongs_to_for<'pr>(stmts: &[Node<'pr>], key: &str) -> Option<Vec<Node<'pr>>> {
    let sym_is = |n: &Node, k: &str| helpers::rails::sym_value(n).as_deref() == Some(k);
    stmts.iter().find_map(|s| {
        let args = belongs_to(s)?;
        let first = args.first()?;
        let hit = if let Some(assoc) = key.strip_suffix("_id") {
            let without_fk = sym_is(first, assoc)
                && match args.len() {
                    1 => true,
                    2 => !(is_hash(&args[1]) && has_pair(&args[1], |k, _| k == "foreign_key")),
                    _ => !is_hash(&args[1]),
                };
            let with_fk = args.last().is_some_and(|h| {
                is_hash(h) && has_pair(h, |k, v| k == "foreign_key" && sym_is(v, key))
            });
            without_fk || with_fk
        } else {
            sym_is(first, key)
        };
        hit.then_some(args)
    })
}

impl Cop for RedundantPresenceValidationOnBelongsTo {
    fn correctable(&self) -> bool {
        true
    }

    fn min_rails(&self) -> f64 {
        5.0
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        if !helpers::rails::is_command(&call, "validates") {
            return;
        }
        // presence_validation?: (sym $_)+ then a hash with `presence: true`, no `strict: true|Const`, no `if:`.
        let args = send_args(&call);
        let Some((options, keys_nodes)) = args.split_last() else { return };
        if keys_nodes.is_empty() || !is_hash(options) {
            return;
        }
        let Some(all_keys) = keys_nodes.iter().map(helpers::rails::sym_value).collect::<Option<Vec<String>>>() else {
            return;
        };
        let pairs = helpers::rails::hash_pairs(options);
        let Some(presence) = pairs
            .iter()
            .find(|p| pair_key(p).as_deref() == Some("presence") && helpers::rails::pair_value(p).as_true_node().is_some())
        else {
            return;
        };
        let strict = |v: &Node| v.as_true_node().is_some() || v.as_constant_read_node().is_some() || v.as_constant_path_node().is_some();
        if has_pair(options, |k, v| k == "strict" && strict(v)) || has_pair(options, |k, _| k == "if") {
            return;
        }
        let used: Vec<String> = pairs.iter().filter_map(pair_key).collect();
        let remaining = used.iter().any(|k| !NON_VALIDATION_OPTIONS.contains(&k.as_str()) && k != "presence");
        if !remaining && pairs.len() > 1 {
            return;
        }
        // non_optional_belongs_to(node.parent, ...): the parent must be a parser `begin`.
        let anc = cx.ancestors;
        let Some(stmts) = anc.last().and_then(|p| p.as_statements_node()) else { return };
        let stmts: Vec<Node> = stmts.body().iter().collect();
        let kwbegin = anc.len() >= 2 && anc[anc.len() - 2].as_begin_node().is_some();
        if stmts.len() < 2 || kwbegin {
            return;
        }
        let keys: Vec<String> = all_keys
            .iter()
            .filter(|k| belongs_to_for(&stmts, k).is_some_and(|a| !optional(&a)))
            .cloned()
            .collect();
        if keys.is_empty() {
            return;
        }
        let display: Vec<String> = keys.iter().map(|k| format!("`{k}`")).collect();
        let msg = format!("Remove explicit presence validation for {}.", display.join("/"));
        let text = cx.src.text;
        let (ps, pe) = range(&presence.as_node());
        let whole = helpers::whole_lines::range_by_whole_lines(text, range(node).0, send_range(&call).1, true);
        // remove_keys_from_validation
        let key_ranges: Vec<(usize, usize)> = keys
            .iter()
            .filter_map(|k| keys_nodes.iter().find(|n| helpers::rails::sym_value(n).as_deref() == Some(k)))
            .map(|n| {
                let (s, e) = range(n);
                let (s, e) = helpers::rails::range_with_surrounding_comma(text, s, e, false, true);
                helpers::range_help::range_with_surrounding_space(
                    text,
                    s,
                    e,
                    helpers::range_help::Surrounding { side: helpers::range_help::Side::Right, ..Default::default() },
                )
            })
            .collect();
        let only_option = helpers::rails::hash_elements(options).is_some_and(|e| e.len() == 1);
        let all = keys == all_keys;
        let extracted = (!only_option && !all).then(|| {
            let col = cx.src.column(range(node).0);
            let others: Vec<&str> = pairs.iter().filter(|p| pair_key(p).as_deref() != Some("presence")).map(|p| node_text(&p.as_node())).collect();
            let syms: Vec<String> = keys.iter().map(|k| helpers::ruby_string::symbol_inspect(k.as_bytes())).collect();
            format!("{}validates {}, {}\n", " ".repeat(col), syms.join(", "), others.join(", "))
        });
        let presence_range = helpers::rails::left_space_and_comma(text, ps, pe);
        cx.add_fix(ps, pe, msg, |c| {
            if only_option && all {
                c.remove(whole.0, whole.1);
            } else if all {
                c.remove(presence_range.0, presence_range.1);
            } else {
                for &(s, e) in &key_ranges {
                    c.remove(s, e);
                }
                if let Some(src) = &extracted {
                    c.insert_after(whole.0, whole.1, src);
                }
            }
        });
    }
}
