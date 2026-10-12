use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Rails/FindByOrAssignmentMemoization";
pub static COP: FindByOrAssignmentMemoization = FindByOrAssignmentMemoization;
pub struct FindByOrAssignmentMemoization;

const MSG: &str = "Avoid memoizing `find_by` results with `||=`.";

/// (or_asgn (ivasgn $_) $(send _ :find_by ...)): (ivar name, find_by source)
fn memoization<'pr>(n: &Node<'pr>) -> Option<(String, &'pr str)> {
    let w = n.as_instance_variable_or_write_node()?;
    let c = helpers::rails::send_node(&w.value())?;
    let v = w.value();
    let text = node_text(&v);
    (method_name(&c) == "find_by").then(|| (String::from_utf8_lossy(w.name().as_slice()).into_owned(), text))
}

/// instance_variable_assigned?: an ivasgn of the name in any `def initialize` of the file.
#[derive(Default)]
struct Assigned(Option<Vec<String>>);

fn initialize_ivars(program: &Node) -> Vec<String> {
    struct F(bool, Vec<String>);
    impl<'pr> Visit<'pr> for F {
        fn visit_def_node(&mut self, d: &ruby_prism::DefNode<'pr>) {
            let was = self.0;
            if d.receiver().is_none() && d.name().as_slice() == b"initialize" {
                self.0 = true;
            }
            ruby_prism::visit_def_node(self, d);
            self.0 = was;
        }
        fn visit_instance_variable_write_node(&mut self, n: &ruby_prism::InstanceVariableWriteNode<'pr>) {
            if self.0 {
                self.1.push(String::from_utf8_lossy(n.name().as_slice()).into_owned());
            }
            ruby_prism::visit_instance_variable_write_node(self, n);
        }
        fn visit_instance_variable_or_write_node(&mut self, n: &ruby_prism::InstanceVariableOrWriteNode<'pr>) {
            if self.0 {
                self.1.push(String::from_utf8_lossy(n.name().as_slice()).into_owned());
            }
            ruby_prism::visit_instance_variable_or_write_node(self, n);
        }
        fn visit_instance_variable_and_write_node(&mut self, n: &ruby_prism::InstanceVariableAndWriteNode<'pr>) {
            if self.0 {
                self.1.push(String::from_utf8_lossy(n.name().as_slice()).into_owned());
            }
            ruby_prism::visit_instance_variable_and_write_node(self, n);
        }
        fn visit_instance_variable_operator_write_node(&mut self, n: &ruby_prism::InstanceVariableOperatorWriteNode<'pr>) {
            if self.0 {
                self.1.push(String::from_utf8_lossy(n.name().as_slice()).into_owned());
            }
            ruby_prism::visit_instance_variable_operator_write_node(self, n);
        }
        fn visit_instance_variable_target_node(&mut self, n: &ruby_prism::InstanceVariableTargetNode<'pr>) {
            if self.0 {
                self.1.push(String::from_utf8_lossy(n.name().as_slice()).into_owned());
            }
        }
    }
    let mut f = F(false, Vec::new());
    f.visit(program);
    f.1
}

fn assigned(cx: &mut Cx, name: &str) -> bool {
    let anc = cx.ancestors;
    let list = cx.state::<Assigned>().0.get_or_insert_with(|| anc.first().map(initialize_ivars).unwrap_or_default());
    list.iter().any(|n| n == name)
}

impl Cop for FindByOrAssignmentMemoization {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE, kind::INSTANCE_VARIABLE_OR_WRITE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if let Some(def) = node.as_def_node() {
            if def.receiver().is_some() {
                return;
            }
            let stmts = helpers::rails::body_statements(def.body());
            let [body] = stmts.as_slice() else { return };
            let Some((var, find_by)) = memoization(body) else { return };
            if assigned(cx, &var) {
                return;
            }
            let (s, e) = (body.location().start_offset(), body.location().end_offset());
            let replacement = format!("return {var} if defined?({var})\n\n{var} = {find_by}");
            let replacement = replacement.trim_end().to_string();
            // An endless def becomes a regular one.
            let endless = def.equal_loc().map(|eq| (eq.start_offset(), node.location().end_offset()));
            cx.add_fix(s, e, MSG, |c| {
                c.replace(s, e, &replacement);
                if let Some((eq, end)) = endless {
                    c.replace(eq, s, "\n");
                    c.insert_after(node.location().start_offset(), end, "\nend");
                }
            });
            return;
        }
        // on_send: the find_by's parent is the or_asgn; no `if` above it.
        let Some((var, find_by)) = memoization(node) else { return };
        if cx.ancestors.iter().any(|a| a.as_if_node().is_some() || a.as_unless_node().is_some()) || assigned(cx, &var) {
            return;
        }
        let (s, e) = (node.location().start_offset(), node.location().end_offset());
        let replacement = format!("if defined?({var})\n  {var}\nelse\n  {var} = {find_by}\nend");
        cx.add_fix(s, e, MSG, |c| c.replace(s, e, &replacement));
    }
}
