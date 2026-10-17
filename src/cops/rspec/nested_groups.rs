use crate::prelude::*;

pub const NAME: &str = "RSpec/NestedGroups";
pub static COP: NestedGroups = NestedGroups;
pub struct NestedGroups;

struct Ctx<'a> {
    l: &'a helpers::rspec::Language,
    max: i64,
    allowed: Vec<String>,
    found: Vec<((usize, usize), i64)>,
}

/// find_nested_example_groups: example groups nested deeper than Max, through `block` and `begin`
/// children only.
fn walk(cx: &mut Ctx, node: &Node, nesting: i64) {
    let group = helpers::rspec::example_group(cx.l, node);
    if group && nesting > cx.max {
        cx.found.push((send_range(&node.as_call_node().unwrap()), nesting));
    }
    let plain = helpers::rspec::plain_block_call(node);
    let next = if group && plain.as_ref().is_some_and(|c| !cx.allowed.iter().any(|a| a == method_name(c))) {
        nesting + 1
    } else {
        nesting
    };
    // each_child_node(:block, :begin): a block's body; a begin's statements
    let children: Vec<Node> = if let Some(c) = helpers::rspec::any_block_call(node) {
        helpers::rspec::parser_body(helpers::rspec::block_body(&c)).into_iter().collect()
    } else {
        helpers::rspec::begin_children(node).unwrap_or_default()
    };
    for child in children {
        if helpers::rspec::plain_block_call(&child).is_some() || helpers::rspec::begin_children(&child).is_some() {
            walk(cx, &child, next);
        }
    }
}

impl Cop for NestedGroups {
    fn kinds(&self) -> &'static [usize] {
        &[kind::PROGRAM_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let l = helpers::rspec::lang(cx);
        let max = match cx.cfg.get("MaxNesting") {
            Some(v) => helpers::rspec::config_integer(Some(v)).unwrap_or(3),
            None => helpers::rspec::config_integer(cx.cfg.get("Max")).unwrap_or(3),
        };
        let mut c = Ctx { l: &l, max, allowed: cx.cfg.strings("AllowedGroups"), found: Vec::new() };
        for group in helpers::rspec::top_level_groups(&l, node) {
            walk(&mut c, &group, 1);
        }
        for ((s, e), nesting) in c.found {
            cx.add(s, e, format!("Maximum example group nesting exceeded [{nesting}/{max}]."));
        }
    }
}
