use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Naming/RescuedExceptionsVariableName";
pub static COP: RescuedExceptionsVariableName = RescuedExceptionsVariableName;
pub struct RescuedExceptionsVariableName;

impl Cop for RescuedExceptionsVariableName {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::RESCUE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let rescue = node.as_rescue_node().unwrap();
        let Some(var) = rescue.reference() else { return };
        let offending = match &var {
            Node::LocalVariableTargetNode { .. } => var.as_local_variable_target_node().unwrap().name(),
            Node::InstanceVariableTargetNode { .. } => var.as_instance_variable_target_node().unwrap().name(),
            Node::ClassVariableTargetNode { .. } => var.as_class_variable_target_node().unwrap().name(),
            Node::GlobalVariableTargetNode { .. } => var.as_global_variable_target_node().unwrap().name(),
            Node::ConstantTargetNode { .. } => var.as_constant_target_node().unwrap().name(),
            Node::ConstantPathTargetNode { .. } => match var.as_constant_path_target_node().unwrap().name() {
                Some(n) => n,
                None => return,
            },
            _ => return,
        };
        let offending = String::from_utf8_lossy(offending.as_slice()).into_owned();
        let at = node.location().start_offset();
        // node.each_ancestor(:resbody).any? (Prism nests later clauses under earlier ones)
        let nested = cx.ancestors.iter().any(|a| match a {
            Node::RescueNode { .. } => {
                let (s, e) = helpers::resbody::range(cx.src, &a.as_rescue_node().unwrap());
                s <= at && at < e
            }
            Node::RescueModifierNode { .. } => {
                let (s, e) = helpers::resbody::modifier_range(&a.as_rescue_modifier_node().unwrap());
                s <= at && at < e
            }
            _ => false,
        });
        if nested {
            return;
        }
        let base = cx.cfg.str("PreferredName").unwrap_or("e").to_string();
        let preferred = if offending.starts_with('_') { format!("_{base}") } else { base.clone() };
        if preferred == offending {
            return;
        }
        // shadowed_variable_name?: an lvar named PreferredName anywhere in this clause
        let mut lvars = LvarNamed { name: base.as_bytes(), found: false };
        for e in rescue.exceptions().iter() {
            lvars.visit(&e);
        }
        if let Some(s) = rescue.statements() {
            lvars.visit_statements_node(&s);
        }
        if lvars.found {
            return;
        }

        let loc = var.location();
        let mut fix = Fixer { name: offending.as_bytes(), stopped: false, edits: Vec::new() };
        if let Some(s) = rescue.statements() {
            fix.run(&s.as_node());
        }
        if !fix.stopped
            && let Some(siblings) = kwbegin_right_siblings(cx.ancestors)
        {
            for s in siblings {
                fix.run(&s);
                if fix.stopped {
                    break;
                }
            }
        }
        let edits = fix.edits;
        let msg = format!("Use `{preferred}` instead of `{offending}`.");
        cx.add_fix(loc.start_offset(), loc.end_offset(), msg, |c| {
            c.replace(loc.start_offset(), loc.end_offset(), &preferred);
            for (s, e, omitted) in edits {
                if omitted {
                    c.replace(s, e, &format!(": {preferred}"));
                } else {
                    c.replace(s, e, &preferred);
                }
            }
        });
    }
}

/// `node.parent.each_ancestor(:kwbegin).first.right_siblings`, for a kwbegin among 2+ statements.
// ponytail: other parents (call arguments, ...) are treated as having no right siblings.
fn kwbegin_right_siblings<'pr>(ancestors: &[Node<'pr>]) -> Option<Vec<Node<'pr>>> {
    let i = ancestors.iter().rposition(|a| a.as_begin_node().is_some_and(|b| b.begin_keyword_loc().is_some()))?;
    let start = ancestors[i].location().start_offset();
    let stmts = ancestors.get(i.checked_sub(1)?)?.as_statements_node()?;
    let body: Vec<Node> = stmts.body().iter().collect();
    let idx = body.iter().position(|n| n.location().start_offset() == start)?;
    Some(body.into_iter().skip(idx + 1).collect())
}

struct LvarNamed<'a> {
    name: &'a [u8],
    found: bool,
}

impl<'pr> Visit<'pr> for LvarNamed<'_> {
    fn visit_local_variable_read_node(&mut self, node: &ruby_prism::LocalVariableReadNode<'pr>) {
        self.found |= node.name().as_slice() == self.name;
    }
}

/// correct_node: rename `lvar`s until the variable is reassigned (`lvasgn` / `masgn` of it).
struct Fixer<'a> {
    name: &'a [u8],
    stopped: bool,
    /// (start, end, value_omission) edits
    edits: Vec<(usize, usize, bool)>,
}

impl Fixer<'_> {
    fn run(&mut self, node: &Node) {
        self.stopped = false;
        self.visit(node);
    }

    /// A matching `lvasgn`/`masgn`: correct its rhs, then stop.
    fn reassigned(&mut self, rhs: Option<Node>) {
        if let Some(rhs) = rhs {
            self.visit(&rhs);
        }
        self.stopped = true;
    }
}

struct TargetNamed<'a> {
    name: &'a [u8],
    found: bool,
}

impl<'pr> Visit<'pr> for TargetNamed<'_> {
    fn visit_local_variable_target_node(&mut self, node: &ruby_prism::LocalVariableTargetNode<'pr>) {
        self.found |= node.name().as_slice() == self.name;
    }
    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode<'pr>) {
        self.found |= node.name().as_slice() == self.name;
        ruby_prism::visit_local_variable_write_node(self, node);
    }
}

impl<'pr> Visit<'pr> for Fixer<'_> {
    fn visit_local_variable_read_node(&mut self, node: &ruby_prism::LocalVariableReadNode<'pr>) {
        if !self.stopped && node.name().as_slice() == self.name {
            let l = node.location();
            self.edits.push((l.start_offset(), l.end_offset(), false));
        }
    }
    fn visit_assoc_node(&mut self, node: &ruby_prism::AssocNode<'pr>) {
        if self.stopped {
            return;
        }
        // value omission `{ex:}`: replace the label colon with `: new_name`
        let omitted = node.value().as_implicit_node().is_some_and(|i| {
            i.value().as_local_variable_read_node().is_some_and(|l| l.name().as_slice() == self.name)
        });
        if omitted {
            let key = node.key().location();
            self.edits.push((key.end_offset() - 1, key.end_offset(), true));
            return;
        }
        ruby_prism::visit_assoc_node(self, node);
    }
    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode<'pr>) {
        if self.stopped {
            return;
        }
        if node.name().as_slice() == self.name {
            return self.reassigned(Some(node.value()));
        }
        ruby_prism::visit_local_variable_write_node(self, node);
    }
    fn visit_local_variable_operator_write_node(&mut self, node: &ruby_prism::LocalVariableOperatorWriteNode<'pr>) {
        if self.stopped {
            return;
        }
        if node.name().as_slice() == self.name {
            return self.reassigned(None);
        }
        ruby_prism::visit_local_variable_operator_write_node(self, node);
    }
    fn visit_local_variable_or_write_node(&mut self, node: &ruby_prism::LocalVariableOrWriteNode<'pr>) {
        if self.stopped {
            return;
        }
        if node.name().as_slice() == self.name {
            return self.reassigned(None);
        }
        ruby_prism::visit_local_variable_or_write_node(self, node);
    }
    fn visit_local_variable_and_write_node(&mut self, node: &ruby_prism::LocalVariableAndWriteNode<'pr>) {
        if self.stopped {
            return;
        }
        if node.name().as_slice() == self.name {
            return self.reassigned(None);
        }
        ruby_prism::visit_local_variable_and_write_node(self, node);
    }
    fn visit_local_variable_target_node(&mut self, node: &ruby_prism::LocalVariableTargetNode<'pr>) {
        if !self.stopped && node.name().as_slice() == self.name {
            self.stopped = true;
        }
    }
    fn visit_multi_write_node(&mut self, node: &ruby_prism::MultiWriteNode<'pr>) {
        if self.stopped {
            return;
        }
        let mut t = TargetNamed { name: self.name, found: false };
        t.visit_multi_write_node(node);
        if t.found {
            return self.reassigned(Some(node.value()));
        }
        ruby_prism::visit_multi_write_node(self, node);
    }
}
