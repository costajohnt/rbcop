use crate::prelude::*;
use helpers::minitest::*;

pub const NAME: &str = "Minitest/LifecycleHooksOrder";
pub static COP: LifecycleHooksOrder = LifecycleHooksOrder;
pub struct LifecycleHooksOrder;

impl Cop for LifecycleHooksOrder {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let class = node.as_class_node().unwrap();
        if !test_class(&class) {
            return;
        }
        let (body, idx) = class_def_nodes(&class);
        // hooks_and_test_cases: lifecycle hooks and public test methods, in order.
        let list: Vec<usize> = idx
            .into_iter()
            .filter(|&i| {
                let d = body[i].as_def_node().unwrap();
                lifecycle_hook(&body[i]) || (test_method_shape(&d) && !non_public_in(&body, i))
            })
            .collect();
        let name = |i: usize| String::from_utf8_lossy(body[i].as_def_node().unwrap().name().as_slice()).into_owned();
        let position = |i: usize| LIFECYCLE_HOOKS.iter().position(|h| *h == name(i)).unwrap_or(LIFECYCLE_HOOKS.len() + 1);
        let mut previous: Option<(usize, usize)> = None;
        for &i in &list {
            let index = position(i);
            if let Some((prev_index, prev)) = previous
                && index < prev_index
            {
                let src = cx.src;
                let anc0 = &cx.ancestors[0];
                let nodes = cx.state::<Associated>().0.get_or_insert_with(|| {
                    let mut a = Associator::new(src, comment_association::skipped_directives(src));
                    ruby_prism::Visit::visit(&mut a, anc0);
                    a.nodes
                });
                let with_comments = |n: &Node| {
                    let (mut start, mut end) = range(n);
                    let comments = nodes.iter().find(|x| x.0 == start && x.1 == end).into_iter().flat_map(|x| x.2.iter());
                    for &c in comments {
                        start = start.min(src.comments[c].start);
                        end = end.max(src.comments[c].end);
                    }
                    helpers::whole_lines::range_by_whole_lines(src.text, start, end, true)
                };
                let prev_range = with_comments(&body[prev]);
                let node_range = with_comments(&body[i]);
                let moved = src.slice(node_range.0, node_range.1).to_string();
                let (s, e) = range(&body[i]);
                let msg = format!("`{}` is supposed to appear before `{}`.", name(i), name(prev));
                cx.add_fix(s, e, msg, |c| {
                    c.insert_before(prev_range.0, prev_range.1, &moved);
                    c.remove(node_range.0, node_range.1);
                });
            }
            previous = Some((index, i));
        }
    }
}

use helpers::comment_association::{self, Associator};

#[derive(Default)]
struct Associated(Option<Vec<(usize, usize, Vec<usize>)>>);
