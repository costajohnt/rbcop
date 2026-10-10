use crate::prelude::*;
use std::collections::HashMap;

pub const NAME: &str = "Lint/UselessSetterCall";
pub static COP: UselessSetterCall = UselessSetterCall;
pub struct UselessSetterCall;

/// MethodVariableTracker: variable name => holds a local (newly constructed) object.
#[derive(Default)]
struct Tracker {
    local: HashMap<Vec<u8>, bool>,
}

fn variable_name(n: &Node) -> Option<Vec<u8>> {
    let name = if let Some(v) = n.as_local_variable_read_node() {
        v.name()
    } else if let Some(v) = n.as_instance_variable_read_node() {
        v.name()
    } else if let Some(v) = n.as_class_variable_read_node() {
        v.name()
    } else if let Some(v) = n.as_global_variable_read_node() {
        v.name()
    } else {
        return None;
    };
    Some(name.as_slice().to_vec())
}

/// Name of a lvasgn/ivasgn/cvasgn/gvasgn target (ASSIGNMENT_TYPES).
fn target_name(n: &Node) -> Option<Vec<u8>> {
    let name = if let Some(v) = n.as_local_variable_target_node() {
        v.name()
    } else if let Some(v) = n.as_instance_variable_target_node() {
        v.name()
    } else if let Some(v) = n.as_class_variable_target_node() {
        v.name()
    } else if let Some(v) = n.as_global_variable_target_node() {
        v.name()
    } else {
        return None;
    };
    Some(name.as_slice().to_vec())
}

fn constructor(n: &Node) -> bool {
    use kind::*;
    match kind::of(n) {
        STRING_NODE | INTERPOLATED_STRING_NODE | X_STRING_NODE | INTERPOLATED_X_STRING_NODE | INTEGER_NODE
        | FLOAT_NODE | SYMBOL_NODE | INTERPOLATED_SYMBOL_NODE | ARRAY_NODE | HASH_NODE | REGULAR_EXPRESSION_NODE
        | INTERPOLATED_REGULAR_EXPRESSION_NODE | TRUE_NODE | RANGE_NODE | IMAGINARY_NODE | RATIONAL_NODE
        | FALSE_NODE | NIL_NODE | SOURCE_FILE_NODE | SOURCE_LINE_NODE => true,
        CALL_NODE => {
            let c = n.as_call_node().unwrap();
            !is_csend(&c) && method_name(&c) == "new" && !c.block().is_some_and(|b| b.as_block_node().is_some())
        }
        _ => false,
    }
}

impl Tracker {
    fn assign(&mut self, name: Vec<u8>, rhs: &Node) {
        let v = match variable_name(rhs) {
            Some(r) => self.local.get(&r).copied().unwrap_or(false),
            None => constructor(rhs),
        };
        self.local.insert(name, v);
    }

    /// One node of `scan`; true means skip its children.
    fn process(&mut self, n: &Node) -> bool {
        use kind::*;
        let k = kind::of(n);
        match k {
            MULTI_WRITE_NODE => {
                let m = n.as_multi_write_node().unwrap();
                let mut lhs: Vec<Node> = m.lefts().iter().collect();
                lhs.extend(m.rest().filter(|r| kind::of(r) != IMPLICIT_REST_NODE));
                lhs.extend(m.rights().iter());
                let value = m.value();
                let elements: Option<Vec<Node>> = value.as_array_node().map(|a| a.elements().iter().collect());
                for (i, l) in lhs.iter().enumerate() {
                    let Some(name) = target_name(l) else { continue };
                    match elements.as_ref().and_then(|e| e.get(i)) {
                        Some(rhs) => self.assign(name, rhs),
                        None => {
                            self.local.insert(name, true);
                        }
                    }
                }
                true
            }
            LOCAL_VARIABLE_OR_WRITE_NODE | LOCAL_VARIABLE_AND_WRITE_NODE | INSTANCE_VARIABLE_OR_WRITE_NODE
            | INSTANCE_VARIABLE_AND_WRITE_NODE | CLASS_VARIABLE_OR_WRITE_NODE | CLASS_VARIABLE_AND_WRITE_NODE
            | GLOBAL_VARIABLE_OR_WRITE_NODE | GLOBAL_VARIABLE_AND_WRITE_NODE => {
                let (name, value) = write_parts(n);
                self.assign(name, &value);
                true
            }
            LOCAL_VARIABLE_OPERATOR_WRITE_NODE
            | INSTANCE_VARIABLE_OPERATOR_WRITE_NODE
            | CLASS_VARIABLE_OPERATOR_WRITE_NODE
            | GLOBAL_VARIABLE_OPERATOR_WRITE_NODE => {
                let (name, _) = write_parts(n);
                self.local.insert(name, true);
                true
            }
            LOCAL_VARIABLE_WRITE_NODE
            | INSTANCE_VARIABLE_WRITE_NODE
            | CLASS_VARIABLE_WRITE_NODE
            | GLOBAL_VARIABLE_WRITE_NODE => {
                let (name, value) = write_parts(n);
                self.assign(name, &value);
                false
            }
            _ => false,
        }
    }
}

/// Name and value of a variable write node.
fn write_parts<'pr>(n: &Node<'pr>) -> (Vec<u8>, Node<'pr>) {
    macro_rules! try_as {
        ($($m:ident),*) => {
            $(if let Some(w) = n.$m() {
                return (w.name().as_slice().to_vec(), w.value());
            })*
        };
    }
    try_as!(
        as_local_variable_write_node,
        as_local_variable_or_write_node,
        as_local_variable_and_write_node,
        as_local_variable_operator_write_node,
        as_instance_variable_write_node,
        as_instance_variable_or_write_node,
        as_instance_variable_and_write_node,
        as_instance_variable_operator_write_node,
        as_class_variable_write_node,
        as_class_variable_or_write_node,
        as_class_variable_and_write_node,
        as_class_variable_operator_write_node,
        as_global_variable_write_node,
        as_global_variable_or_write_node,
        as_global_variable_and_write_node,
        as_global_variable_operator_write_node
    );
    unreachable!()
}

struct Scan<'a> {
    tracker: &'a mut Tracker,
    skip_depth: usize,
    depth: usize,
}

impl<'pr> ruby_prism::Visit<'pr> for Scan<'_> {
    fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
        self.depth += 1;
        if self.skip_depth == 0 && self.tracker.process(&node) {
            self.skip_depth = self.depth;
        }
    }
    fn visit_branch_node_leave(&mut self) {
        if self.skip_depth == self.depth {
            self.skip_depth = 0;
        }
        self.depth -= 1;
    }
}

impl Cop for UselessSetterCall {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let def = node.as_def_node().unwrap();
        let Some(body) = def.body() else { return };
        let last = match body.as_statements_node() {
            Some(s) => match s.body().iter().last() {
                Some(l) => l,
                None => return,
            },
            None => def.body().unwrap(),
        };
        let Some(call) = last.as_call_node() else { return };
        if is_csend(&call) || call.equal_loc().is_none() || call.block().is_some_and(|b| b.as_block_node().is_some()) {
            return;
        }
        let Some(recv) = call.receiver() else { return };
        let Some(lvar) = recv.as_local_variable_read_node() else { return };
        let mut tracker = Tracker::default();
        let mut scan = Scan { tracker: &mut tracker, skip_depth: 0, depth: 0 };
        ruby_prism::Visit::visit(&mut scan, &body);
        if !tracker.local.get(lvar.name().as_slice()).copied().unwrap_or(false) {
            return;
        }
        let rl = recv.location();
        let name = node_text(&recv).to_string();
        let ll = last.location();
        let indent = " ".repeat(cx.src.column(ll.start_offset()));
        let msg = format!("Useless setter call to local variable `{name}`.");
        let (ls, le) = (ll.start_offset(), ll.end_offset());
        cx.add_fix(rl.start_offset(), rl.end_offset(), msg, |c| c.insert_after(ls, le, &format!("\n{indent}{name}")));
    }
}
