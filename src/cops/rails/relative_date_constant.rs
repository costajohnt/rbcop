use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Rails/RelativeDateConstant";
pub static COP: RelativeDateConstant = RelativeDateConstant;
pub struct RelativeDateConstant;

const METHODS: &[&str] = &["since", "from_now", "after", "ago", "until", "before", "yesterday", "tomorrow"];

/// nested_relative_date: post-order over the node's children (blocks skipped), the first
/// relative-date send found.
fn nested_relative_date(node: &Node) -> Option<String> {
    struct F(Option<String>);
    impl<'pr> Visit<'pr> for F {
        fn visit_call_node(&mut self, c: &ruby_prism::CallNode<'pr>) {
            if c.block().is_some_and(|b| b.as_block_node().is_some()) {
                return;
            }
            ruby_prism::visit_call_node(self, c);
            if self.0.is_none() && !is_csend(c) && send_args(c).is_empty() && METHODS.contains(&method_name(c)) {
                self.0 = Some(method_name(c).to_string());
            }
        }
        fn visit_lambda_node(&mut self, _: &ruby_prism::LambdaNode<'pr>) {}
    }
    let mut f = F(None);
    f.visit(node);
    f.0
}

fn msg(m: &str) -> String {
    format!("Do not assign `{m}` to constants as it will be evaluated only once.")
}

impl Cop for RelativeDateConstant {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[
            kind::CONSTANT_WRITE_NODE,
            kind::CONSTANT_PATH_WRITE_NODE,
            kind::MULTI_WRITE_NODE,
            kind::CONSTANT_OR_WRITE_NODE,
            kind::CONSTANT_PATH_OR_WRITE_NODE,
        ]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let (s, e) = (node.location().start_offset(), node.location().end_offset());
        match node {
            Node::ConstantWriteNode { .. } | Node::ConstantPathWriteNode { .. } => {
                let (value, plain) = match node.as_constant_write_node() {
                    Some(w) => (w.value(), Some(w)),
                    None => (node.as_constant_path_write_node().unwrap().value(), None),
                };
                let found = match node.as_constant_path_write_node() {
                    Some(w) => nested_relative_date(&w.target().as_node()).or_else(|| nested_relative_date(&value)),
                    None => nested_relative_date(&value),
                };
                let Some(m) = found else { return };
                let fix = plain.map(|w| {
                    let indent = " ".repeat(cx.src.column(s));
                    let name = String::from_utf8_lossy(w.name().as_slice()).to_lowercase();
                    [format!("def self.{name}"), format!("{indent}{}", node_text(&value)), "end".to_string()].join(&format!("\n{indent}"))
                });
                cx.add_fix(s, e, msg(&m), |c| {
                    if let Some(f) = &fix {
                        c.replace(s, e, f);
                    }
                });
            }
            Node::MultiWriteNode { .. } => {
                let mw = node.as_multi_write_node().unwrap();
                let Some(rhs) = mw.value().as_array_node() else { return };
                let values: Vec<Node> = rhs.elements().iter().collect();
                let mut names: Vec<Node> = mw.lefts().iter().collect();
                if let Some(r) = mw.rest() {
                    names.push(r);
                }
                names.extend(mw.rights().iter());
                for (i, name) in names.iter().enumerate() {
                    if name.as_constant_target_node().is_none() && name.as_constant_path_target_node().is_none() {
                        continue;
                    }
                    let Some(value) = values.get(i) else { continue };
                    let Some(m) = nested_relative_date(value) else { continue };
                    cx.add_with(name.location().start_offset(), value.location().end_offset(), msg(&m), false);
                }
            }
            _ => {
                // (:or_asgn (casgn _ _) (send _ $RELATIVE_DATE_METHODS))
                let value = match node.as_constant_or_write_node() {
                    Some(w) => w.value(),
                    None => node.as_constant_path_or_write_node().unwrap().value(),
                };
                let Some(c) = helpers::rails::send_node(&value) else { return };
                let m = method_name(&c);
                if METHODS.contains(&m) && send_args(&c).is_empty() {
                    cx.add_with(s, e, msg(m), false);
                }
            }
        }
    }
}
