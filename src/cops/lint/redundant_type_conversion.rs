use crate::prelude::*;
use helpers::select_by::plain_send;
use helpers::literal_node::parser_type;
use helpers::parser_parent::is_top_const;

pub const NAME: &str = "Lint/RedundantTypeConversion";
pub static COP: RedundantTypeConversion = RedundantTypeConversion;
pub struct RedundantTypeConversion;

/// LITERAL_NODE_TYPES
fn literal_types(method: &str) -> &'static [&'static str] {
    match method {
        "to_s" => &["str", "dstr"],
        "to_sym" => &["sym", "dsym"],
        "to_i" => &["int"],
        "to_f" => &["float"],
        "to_r" => &["rational"],
        "to_c" => &["complex"],
        "to_a" => &["array"],
        "to_h" => &["hash"],
        _ => &[],
    }
}

/// `(send {nil? (const {cbase nil?} :Kernel)} :name ...)`
fn type_constructor(node: &Node, name: &str) -> bool {
    plain_send(node).is_some_and(|c| method_name(&c) == name && c.receiver().is_none_or(|r| is_top_const(&r, "Kernel")))
}

/// `(send (const {cbase nil?} :Class) {methods} ...)`
fn class_send(node: &Node, class: &str, methods: &[&str]) -> bool {
    plain_send(node).is_some_and(|c| {
        methods.contains(&method_name(&c)) && c.receiver().is_some_and(|r| is_top_const(&r, class))
    })
}

fn constructor(method: &str, r: &Node) -> bool {
    match method {
        "to_s" => class_send(r, "String", &["new"]) || type_constructor(r, "String"),
        "to_i" => type_constructor(r, "Integer"),
        "to_f" => type_constructor(r, "Float"),
        "to_d" => type_constructor(r, "BigDecimal"),
        "to_r" => type_constructor(r, "Rational"),
        "to_c" => type_constructor(r, "Complex"),
        "to_a" => class_send(r, "Array", &["new", "[]"]) || type_constructor(r, "Array"),
        "to_h" => {
            // `(block (send (const {cbase nil?} :Hash) :new) ...)`
            let hash_new_block = r.as_call_node().is_some_and(|c| {
                c.block().and_then(|b| b.as_block_node()).is_some_and(|b| {
                    b.parameters().is_none_or(|p| p.as_block_parameters_node().is_some())
                }) && !is_csend(&c)
                    && method_name(&c) == "new"
                    && c.arguments().is_none()
                    && c.receiver().is_some_and(|x| is_top_const(&x, "Hash"))
            });
            hash_new_block || class_send(r, "Hash", &["new", "[]"]) || type_constructor(r, "Hash")
        }
        "to_set" => class_send(r, "Set", &["new", "[]"]),
        _ => false,
    }
}

/// `receiver.arguments.any? { (hash (pair (sym :exception) false)) }`
fn suppresses_exceptions(r: &Node) -> bool {
    let Some(c) = r.as_call_node() else { return false };
    if c.block().is_some_and(|b| b.as_block_node().is_some()) {
        // A block's `arguments` are its parameters: never a hash.
        return false;
    }
    send_args(&c).iter().any(|a| {
        let pairs: Vec<Node> = match (a.as_hash_node(), a.as_keyword_hash_node()) {
            (Some(h), _) => h.elements().iter().collect(),
            (_, Some(h)) => h.elements().iter().collect(),
            _ => return false,
        };
        let [pair] = pairs.as_slice() else { return false };
        pair.as_assoc_node().is_some_and(|p| {
            p.key().as_symbol_node().is_some_and(|s| s.unescaped() == b"exception") && p.value().as_false_node().is_some()
        })
    })
}

impl Cop for RedundantTypeConversion {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let method = method_name(&call);
        if !matches!(method, "to_s" | "to_sym" | "to_i" | "to_f" | "to_r" | "to_c" | "to_a" | "to_h" | "to_set" | "to_d") {
            return;
        }
        if call.arguments().is_some() || call.block().is_some_and(|b| b.as_block_argument_node().is_some()) {
            return;
        }
        // hash_or_set_with_block?: `node.parent&.any_block_type?` also holds for a block's sole
        // body statement.
        if matches!(method, "to_h" | "to_set")
            && (call.block().is_some()
                || helpers::parser_parent::parser_parent(cx.ancestors).is_some_and(|p| frame(p) == Frame::Block))
        {
            return;
        }
        let Some(mut receiver) = call.receiver() else { return };
        // find_receiver: unwrap `(begin x)` with one child.
        while let Some(p) = receiver.as_parentheses_node() {
            let Some(body) = p.body() else { break };
            receiver = match body.as_statements_node() {
                Some(s) => {
                    let mut it = s.body().iter();
                    match (it.next(), it.next()) {
                        (Some(only), None) => only,
                        _ => break,
                    }
                }
                None => body,
            };
        }
        // `call_type?`: send or csend, not a block.
        let receiver_call = receiver.as_call_node().filter(|c| !c.block().is_some_and(|b| b.as_block_node().is_some()));
        let hit = literal_types(method).contains(&parser_type(&receiver))
            || (constructor(method, &receiver) && !suppresses_exceptions(&receiver))
            || receiver_call.as_ref().is_some_and(|r| method_name(r) == method)
            || (method == "to_s" && receiver_call.as_ref().is_some_and(|r| matches!(method_name(r), "inspect" | "to_json")));
        if !hit {
            return;
        }
        let (Some(sel), Some(dot)) = (call.message_loc(), call.call_operator_loc()) else { return };
        let end = call.closing_loc().map_or(sel.end_offset(), |c| c.end_offset());
        let ds = dot.start_offset();
        cx.add_fix(sel.start_offset(), sel.end_offset(), format!("Redundant `{method}` detected."), |c| c.remove(ds, end));
    }
}
