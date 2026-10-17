use crate::prelude::*;

pub const NAME: &str = "RSpecRails/TravelAround";
pub static COP: TravelAround = TravelAround;
pub struct TravelAround;

/// (around block range, its first parameter's name, its column)
type Around = ((usize, usize), Option<Vec<u8>>, usize);

fn travel_method(name: &str) -> bool {
    matches!(name, "freeze_time" | "travel" | "travel_to")
}

/// extract_surrounding_around_block: the nearest plain-block ancestor `(block (send _ :around
/// (sym :each)?) ...)` the node is a direct statement of. Returns (around range, its first
/// parameter's name).
fn surrounding_around(cx: &Cx, anc: &[Node]) -> Option<Around> {
    // direct_child_of_around_body?: the parser parent is the block, or a begin directly in it
    let pi = helpers::parser_parent::parser_parent_index(anc, anc.len())?;
    let bi = if anc[pi].as_block_node().is_some() {
        pi
    } else if anc[pi].as_statements_node().is_some_and(|s| s.body().iter().count() > 1) {
        helpers::parser_parent::parser_parent_index(anc, pi).filter(|&b| anc[b].as_block_node().is_some())?
    } else {
        return None;
    };
    // each_ancestor(:block).find: the first matching around; only the direct one can be a parent
    for i in (0..anc.len()).rev() {
        let Some(c) = helpers::rspec::plain_block_call(&anc[i]) else { continue };
        let is_around = !is_csend(&c)
            && method_name(&c) == "around"
            && match &send_args(&c)[..] {
                [] => true,
                [a] => helpers::rspec::is_sym(cx, a) && helpers::rails::sym_value(a).as_deref() == Some("each"),
                _ => false,
            };
        if !is_around {
            continue;
        }
        // direct child of this around block?
        if bi == i + 1 {
            let block = helpers::rspec::block_of(&c).unwrap();
            let name = block
                .parameters()
                .and_then(|p| p.as_block_parameters_node())
                .and_then(|p| helpers::rspec::first_block_param(&p))
                .and_then(|(_, _, n)| n)
                .map(|n| n.into_bytes());
            return Some((helpers::rspec::node_range(&anc[i]), name, cx.src.column(anc[i].location().start_offset())));
        }
    }
    None
}

impl Cop for TravelAround {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let src = cx.src;
        let anc = cx.ancestors;
        if let Some(block) = helpers::rspec::block_of(&call) {
            // extract_run_in_travel: (block $(send nil? TRAVEL_METHOD_NAMES ...) (args ...) (send (lvar _) :run))
            if !helpers::rspec::is_plain(&block) || call.receiver().is_some() || !travel_method(method_name(&call)) {
                return;
            }
            let Some(body) = helpers::rspec::parser_body(block.body()) else { return };
            let Some(run) = helpers::rspec::send_call(&body).filter(|r| !is_csend(r) && method_name(r) == "run" && send_args(r).is_empty()) else {
                return;
            };
            let Some(lvar) = run.receiver().and_then(|r| r.as_local_variable_read_node()) else { return };
            let Some(((as_, ae), name, column)) = surrounding_around(cx, anc) else { return };
            // matching_run_receiver?
            if name.as_deref() != Some(lvar.name().as_slice()) {
                return;
            }
            let (s, e) = helpers::rspec::node_range(node);
            let body_src = node_text(&body).to_string();
            let travel_src = src.slice(send_range(&call).0, send_range(&call).1).to_string();
            let insert = format!("before {{ {travel_src} }}\n\n{}", " ".repeat(column));
            cx.add_fix(s, e, "Prefer to travel in `before` rather than `around`.", |c| {
                c.replace(s, e, &body_src);
                c.insert_before(as_, ae, &insert);
            });
            return;
        }
        // extract_travel_with_block_pass: $(send _ TRAVEL_METHOD_NAMES (block_pass $lvar))
        if is_csend(&call) || !travel_method(method_name(&call)) || call.arguments().is_some() {
            return;
        }
        let Some(lvar) = call
            .block()
            .and_then(|b| b.as_block_argument_node())
            .and_then(|b| b.expression())
            .and_then(|x| x.as_local_variable_read_node().map(|l| l.name().as_slice().to_vec()))
        else {
            return;
        };
        let Some(((as_, ae), name, column)) = surrounding_around(cx, anc) else { return };
        if name.as_deref() != Some(lvar.as_slice()) {
            return;
        }
        let (s, e) = send_range(&call);
        let replacement = format!("{}.run", String::from_utf8_lossy(&lvar));
        let insert = format!("before {{ {} }}\n\n{}", method_name(&call), " ".repeat(column));
        cx.add_fix(s, e, "Prefer to travel in `before` rather than `around`.", |c| {
            c.replace(s, e, &replacement);
            c.insert_before(as_, ae, &insert);
        });
    }
}
