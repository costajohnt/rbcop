use crate::prelude::*;

pub const NAME: &str = "RSpec/MultipleMemoizedHelpers";
pub static COP: MultipleMemoizedHelpers = MultipleMemoizedHelpers;
pub struct MultipleMemoizedHelpers;

impl Cop for MultipleMemoizedHelpers {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let l = helpers::rspec::lang(cx);
        if helpers::rspec::plain_block_call(node).is_none() || !helpers::rspec::spec_group(&l, node) {
            return;
        }
        let allow_subject = cx.cfg.bool("AllowSubject") == Some(true);
        // all_helpers: this group's and every plain-block ancestor's lets (and subjects), by the
        // structure of their name (`variable_definition?`), nil for none
        let mut names: Vec<Option<String>> = Vec::new();
        let groups = std::iter::once(node).chain(cx.ancestors.iter().rev().filter(|a| helpers::rspec::plain_block_call(a).is_some()));
        for group in groups {
            let mut vars = helpers::rspec::lets_in_scope(&l, group);
            if !allow_subject {
                vars.extend(helpers::rspec::find_all_in_scope(&l, group, helpers::rspec::subject));
            }
            for v in vars {
                let name = helpers::rspec::variable_definition(cx, &l, &v).and_then(|n| helpers::parser_sexp::sexp(&n, cx.src));
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
        let count = names.len() as i64;
        let max = cx.cfg.int("Max").unwrap_or(5);
        if count <= max {
            return;
        }
        let (s, e) = helpers::rspec::node_range(node);
        cx.exclude_limit("Max", count);
        cx.add(s, e, format!("Example group has too many memoized helpers [{count}/{max}]"));
    }
}
