use crate::prelude::*;

pub const NAME: &str = "FactoryBot/AttributeDefinedStatically";
pub static COP: AttributeDefinedStatically = AttributeDefinedStatically;
pub struct AttributeDefinedStatically;

impl Cop for AttributeDefinedStatically {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // factory_attributes: (block (send _ #attribute_defining_method? ...) _ { (begin $...) $(send ...) })
        let Some(call) = helpers::rspec::plain_block_call(node) else { return };
        if is_csend(&call) || !helpers::factory_bot::ATTRIBUTE_DEFINING_METHODS.contains(&method_name(&call)) {
            return;
        }
        let block = helpers::rspec::block_of(&call).unwrap();
        let Some(body) = helpers::rspec::parser_body(block.body()) else { return };
        let attributes: Vec<Node> = match body.as_statements_node() {
            Some(s) => s.body().iter().collect(),
            None if helpers::rspec::send_call(&body).is_some_and(|c| !is_csend(&c)) => vec![body],
            None => return,
        };
        // the block's first parameter (receiver_matches_first_block_argument?)
        // (an optional parameter's node_parts hold its default too: no lvar matches it)
        let optional_first = block.parameters().and_then(|p| p.as_block_parameters_node()).and_then(|p| p.parameters()).is_some_and(|p| {
            p.requireds().iter().next().is_none() && p.optionals().iter().next().is_some()
                || (p.requireds().iter().next().is_none()
                    && p.optionals().iter().next().is_none()
                    && p.rest().is_none()
                    && p.posts().iter().next().is_none()
                    && p.keywords().iter().next().is_some_and(|k| k.as_optional_keyword_parameter_node().is_some()))
        });
        let first_param = block
            .parameters()
            .and_then(|p| p.as_block_parameters_node())
            .and_then(|p| helpers::rspec::first_block_param(&p))
            .and_then(|(_, _, n)| n)
            .filter(|_| !optional_first);
        for attribute in &attributes {
            // value_matcher: (send _ !#reserved_method? $...); a non-send, a reserved method or an
            // attribute whose values are all block passes counts as `proc?`
            let Some(attr) = helpers::rspec::send_call(attribute).filter(|c| !is_csend(c)) else { continue };
            let offensive_receiver = match attr.receiver() {
                None => true,
                Some(r) if r.as_self_node().is_some() => true,
                Some(r) => r
                    .as_local_variable_read_node()
                    .is_some_and(|l| first_param.as_deref().is_some_and(|f| l.name().as_slice() == f.as_bytes())),
            };
            if !offensive_receiver {
                continue;
            }
            let name = method_name(&attr);
            let args = send_args(&attr);
            if helpers::factory_bot::RESERVED_METHODS.contains(&name) || args.iter().all(|a| a.as_block_argument_node().is_some()) {
                continue;
            }
            // association?(attribute.first_argument): (hash <(pair (sym :factory) _) ...>)
            let first = &args[0];
            if helpers::rails::hash_elements(first).is_some_and(|ps| {
                ps.iter().filter_map(|p| p.as_assoc_node()).any(|p| {
                    helpers::rspec::is_sym(cx, &p.key()) && helpers::rails::sym_value(&p.key()).as_deref() == Some("factory")
                })
            }) {
                continue;
            }
            let (lb, rb) = if first.as_keyword_hash_node().is_some() { ("{ { ", " } }") } else { ("{ ", " }") };
            let (s, e) = send_range(&attr);
            let edits: Vec<(usize, usize, String)> = match (attr.opening_loc(), attr.closing_loc()) {
                (Some(o), Some(c)) if o.as_slice() == b"(" => vec![
                    (o.start_offset(), o.end_offset(), format!(" {lb}")),
                    (c.start_offset(), c.end_offset(), rb.to_string()),
                ],
                _ => {
                    let (fs, fe) = helpers::rspec::node_range(first);
                    vec![(fs, fs, lb.to_string()), (fe, fe, rb.to_string())]
                }
            };
            let parenthesized = attr.opening_loc().is_some_and(|o| o.as_slice() == b"(");
            let (fs, fe) = helpers::rspec::node_range(first);
            cx.add_fix(s, e, "Use a block to declare attribute values.", |c| {
                if parenthesized {
                    for (a, b, t) in &edits {
                        c.replace(*a, *b, t);
                    }
                } else {
                    c.insert_before(fs, fe, lb);
                    c.insert_after(fs, fe, rb);
                }
            });
        }
    }
}
