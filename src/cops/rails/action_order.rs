use crate::prelude::*;
use helpers::comment_association::{self, Associator};
use ruby_prism::Visit;

pub const NAME: &str = "Rails/ActionOrder";
pub static COP: ActionOrder = ActionOrder;
pub struct ActionOrder;

/// A `def` among the expected actions: name, range, whether it counts (public, no non-public
/// modifier), and its correction target (the nearest enclosing `if`, else itself).
struct Action {
    name: String,
    range: (usize, usize),
    public: bool,
    target: (usize, usize),
}

struct Finder<'a> {
    actions: &'a [String],
    found: Vec<Action>,
    ifs: Vec<(usize, usize)>,
    /// The def that is the sole argument of a visibility call, with that call's name.
    inline: Option<((usize, usize), String)>,
}

impl Finder<'_> {
    fn def(&mut self, d: &ruby_prism::DefNode, siblings: &[Node], idx: usize) {
        if d.receiver().is_some() {
            return;
        }
        let name = String::from_utf8_lossy(d.name().as_slice()).into_owned();
        if !self.actions.contains(&name) {
            return;
        }
        let range = (d.location().start_offset(), d.location().end_offset());
        let public = match &self.inline {
            Some((r, m)) if *r == range => m == "public",
            _ => {
                let node = d.as_node();
                helpers::visibility_help::node_visibility(&node, None, siblings, idx) == "public"
            }
        };
        let target = self.ifs.last().copied().unwrap_or(range);
        self.found.push(Action { name, range, public, target });
    }
}

impl<'pr> Visit<'pr> for Finder<'_> {
    fn visit_statements_node(&mut self, s: &ruby_prism::StatementsNode<'pr>) {
        let body: Vec<Node> = s.body().iter().collect();
        let siblings: &[Node] = if body.len() >= 2 { &body } else { &[] };
        for (i, n) in body.iter().enumerate() {
            if let Some(d) = n.as_def_node() {
                self.def(&d, siblings, i);
            }
            self.visit(n);
        }
    }
    fn visit_call_node(&mut self, c: &ruby_prism::CallNode<'pr>) {
        let args = send_args(c);
        if c.receiver().is_none()
            && matches!(method_name(c), "private" | "protected" | "public" | "private_class_method")
            && c.block().is_none_or(|b| b.as_block_node().is_none())
            && args.len() == 1
            && let Some(d) = args[0].as_def_node()
        {
            self.inline = Some(((d.location().start_offset(), d.location().end_offset()), method_name(c).to_string()));
            self.def(&d, &[], 0);
        } else {
            for a in &args {
                if let Some(d) = a.as_def_node() {
                    self.def(&d, &[], 0);
                }
            }
        }
        ruby_prism::visit_call_node(self, c);
    }
    fn visit_if_node(&mut self, n: &ruby_prism::IfNode<'pr>) {
        let node = n.as_node();
        self.ifs.push((node.location().start_offset(), helpers::if_node::parser_end(&node)));
        ruby_prism::visit_if_node(self, n);
        self.ifs.pop();
    }
    fn visit_unless_node(&mut self, n: &ruby_prism::UnlessNode<'pr>) {
        self.ifs.push((n.location().start_offset(), n.location().end_offset()));
        ruby_prism::visit_unless_node(self, n);
        self.ifs.pop();
    }
}

#[derive(Default)]
struct Associated(Option<Vec<(usize, usize, Vec<usize>)>>);

impl Cop for ActionOrder {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let expected = cx.cfg.strings("ExpectedOrder");
        let class = node.as_class_node().unwrap();
        let ifs: Vec<(usize, usize)> = cx
            .ancestors
            .iter()
            .filter(|a| a.as_if_node().is_some() || a.as_unless_node().is_some())
            .map(|a| (a.location().start_offset(), helpers::if_node::parser_end(a)))
            .collect();
        let mut f = Finder { actions: &expected, found: Vec::new(), ifs, inline: None };
        // A class body that is a single def is not inside a statement list.
        if let Some(body) = class.body() {
            f.visit(&body);
        }
        let found = f.found;
        let src = cx.src;
        let index = |n: &str| expected.iter().position(|e| e == n).unwrap_or(usize::MAX);
        for w in found.windows(2) {
            let (prev, cur) = (&w[0], &w[1]);
            if !cur.public || index(&cur.name) >= index(&prev.name) {
                continue;
            }
            let anc = cx.ancestors;
            let nodes = cx.state::<Associated>().0.get_or_insert_with(|| {
                let mut a = Associator::new(src, comment_association::skipped_directives(src));
                a.visit(&anc[0]);
                a.nodes
            });
            let with_comments = |(s0, e0): (usize, usize)| {
                let (mut start, mut end) = (s0, e0);
                for n in nodes.iter().filter(|n| n.0 == s0 && n.1 == e0).take(1) {
                    for &c in &n.2 {
                        start = start.min(src.comments[c].start);
                        end = end.max(src.comments[c].end);
                    }
                }
                helpers::whole_lines::range_by_whole_lines(&src.text, start, end, true)
            };
            let current = with_comments(cur.target);
            let previous = with_comments(prev.target);
            let moved = src.slice(current.0, current.1).to_string();
            let msg = format!("Action `{}` should appear before `{}`.", cur.name, prev.name);
            cx.add_fix(cur.range.0, cur.range.1, msg, |c| {
                c.insert_before(previous.0, previous.1, &moved);
                c.remove(current.0, current.1);
            });
        }
    }
}
