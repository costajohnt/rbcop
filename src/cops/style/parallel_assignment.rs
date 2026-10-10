use crate::prelude::*;
use crate::source::Source;
use helpers::node_equality::same;
use ruby_prism::Visit;

pub const NAME: &str = "Style/ParallelAssignment";
pub static COP: ParallelAssignment = ParallelAssignment;
pub struct ParallelAssignment;

fn span(n: &Node) -> (usize, usize) {
    (n.location().start_offset(), n.location().end_offset())
}

/// A left-hand side as AssignmentSorter sees it.
enum Lhs {
    /// `var_name`: lvasgn/ivasgn/cvasgn/gvasgn name, or a casgn's constant name.
    Var(Vec<u8>),
    /// A setter send (`recv.a=`, `recv[args]=`): receiver range, getter name, argument ranges.
    Setter { recv: (usize, usize), getter: Vec<u8>, index: bool, args: Vec<(usize, usize)> },
    Other,
}

/// `MlhsNode#assignments`: flattened targets, a splat replaced by its target (`*` alone stays).
fn assignments<'pr>(lefts: Vec<Node<'pr>>, rest: Option<Node<'pr>>, rights: Vec<Node<'pr>>, out: &mut Vec<Node<'pr>>) {
    for n in lefts.into_iter().chain(rest).chain(rights) {
        if let Some(m) = n.as_multi_target_node() {
            assignments(m.lefts().iter().collect(), m.rest(), m.rights().iter().collect(), out);
        } else if let Some(s) = n.as_splat_node() {
            out.push(s.expression().unwrap_or(n));
        } else if n.as_implicit_rest_node().is_none() {
            out.push(n);
        }
    }
}

fn lhs_of(n: &Node) -> Lhs {
    let name = match n {
        Node::LocalVariableTargetNode { .. } => n.as_local_variable_target_node().unwrap().name(),
        Node::InstanceVariableTargetNode { .. } => n.as_instance_variable_target_node().unwrap().name(),
        Node::ClassVariableTargetNode { .. } => n.as_class_variable_target_node().unwrap().name(),
        Node::GlobalVariableTargetNode { .. } => n.as_global_variable_target_node().unwrap().name(),
        Node::ConstantTargetNode { .. } => n.as_constant_target_node().unwrap().name(),
        Node::ConstantPathTargetNode { .. } => match n.as_constant_path_target_node().unwrap().name() {
            Some(name) => name,
            None => return Lhs::Other,
        },
        Node::CallTargetNode { .. } => {
            let c = n.as_call_target_node().unwrap();
            if c.call_operator_loc().as_slice() == b"&." {
                return Lhs::Other;
            }
            let name = c.name().as_slice();
            let getter = name[..name.len() - 1].to_vec();
            return Lhs::Setter { recv: span(&c.receiver()), getter, index: false, args: vec![] };
        }
        Node::IndexTargetNode { .. } => {
            let c = n.as_index_target_node().unwrap();
            let mut args: Vec<(usize, usize)> =
                c.arguments().map(|a| a.arguments().iter().map(|x| span(&x)).collect()).unwrap_or_default();
            args.extend(c.block().map(|b| span(&b.as_node())));
            return Lhs::Setter { recv: span(&c.receiver()), getter: b"[]".to_vec(), index: true, args };
        }
        _ => return Lhs::Other,
    };
    Lhs::Var(name.as_slice().to_vec())
}

/// A right-hand side element; `SelfGetter` is `add_self_to_getters`' `s(:send, s(:self), var)`.
enum Rhs<'a, 'pr> {
    Real(&'a Node<'pr>),
    SelfGetter(Vec<u8>),
}

/// `uses_var?` / `matching_calls` searches over a subtree (the node included).
struct Search<'a> {
    src: &'a Source<'a>,
    var: Option<&'a [u8]>,
    setter: Option<&'a Lhs>,
    found: bool,
}

impl<'pr> Visit<'pr> for Search<'_> {
    fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
        self.check(&node);
    }
    fn visit_leaf_node_enter(&mut self, node: Node<'pr>) {
        self.check(&node);
    }
}

impl Search<'_> {
    fn check(&mut self, n: &Node) {
        if let Some(var) = self.var {
            let name = match n {
                Node::LocalVariableReadNode { .. } => Some(n.as_local_variable_read_node().unwrap().name()),
                Node::InstanceVariableReadNode { .. } => Some(n.as_instance_variable_read_node().unwrap().name()),
                Node::ClassVariableReadNode { .. } => Some(n.as_class_variable_read_node().unwrap().name()),
                Node::GlobalVariableReadNode { .. } => Some(n.as_global_variable_read_node().unwrap().name()),
                Node::ConstantReadNode { .. } => Some(n.as_constant_read_node().unwrap().name()),
                Node::ConstantPathNode { .. } => n.as_constant_path_node().unwrap().name(),
                _ => None,
            };
            if name.is_some_and(|nm| nm.as_slice() == var) {
                self.found = true;
            }
        }
        if let Some(Lhs::Setter { recv, getter, index, args }) = self.setter
            && let Some(c) = n.as_call_node()
            && !is_csend(&c)
            && c.name().as_slice() == getter.as_slice()
            && c.receiver().is_some_and(|r| same(self.src, span(&r), *recv))
        {
            if !index {
                self.found = true;
                return;
            }
            let mut call_args: Vec<(usize, usize)> = call_args(&c).iter().map(span).collect();
            call_args.extend(c.block().filter(|b| b.as_block_argument_node().is_some()).map(|b| span(&b)));
            if call_args.len() == args.len() && call_args.iter().zip(args).all(|(a, b)| same(self.src, *a, *b)) {
                self.found = true;
            }
        }
    }
}

/// AssignmentSorter#dependency?
fn dependency(src: &Source, lhs: &Lhs, rhs: &Rhs) -> bool {
    match rhs {
        Rhs::SelfGetter(name) => match lhs {
            Lhs::Setter { recv, getter, index: false, .. } => {
                getter == name && src.text[recv.0..recv.1].trim() == "self"
            }
            _ => false,
        },
        Rhs::Real(node) => {
            let mut s = Search { src, var: None, setter: None, found: false };
            match lhs {
                Lhs::Var(v) => s.var = Some(v),
                Lhs::Setter { .. } => s.setter = Some(lhs),
                Lhs::Other => return false,
            }
            s.visit(node);
            s.found
        }
    }
}

/// AssignmentSorter#tsort over the `keep` indices: indices in assignment order, None on a cycle.
fn tsort(src: &Source, keep: &[usize], lhs: &[Lhs], rhs: &[Rhs]) -> Option<Vec<usize>> {
    let n = lhs.len();
    let mut deps: Vec<Vec<usize>> = (0..n)
        .map(|i| keep.iter().copied().filter(|&j| j != i && dependency(src, &lhs[i], &rhs[j])).collect())
        .collect();
    let mut remaining: Vec<usize> = keep.to_vec();
    let mut result = vec![];
    while let Some(pos) = remaining.iter().position(|&i| deps[i].is_empty()) {
        let m = remaining.remove(pos);
        result.push(m);
        for d in deps.iter_mut() {
            d.retain(|&x| x != m);
        }
    }
    remaining.is_empty().then_some(result)
}

struct Heredoc(bool);

impl<'pr> Visit<'pr> for Heredoc {
    fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
        self.0 |= is_heredoc(&node);
    }
    fn visit_leaf_node_enter(&mut self, node: Node<'pr>) {
        self.0 |= is_heredoc(&node);
    }
}

/// GenericCorrector#source
fn rhs_source(n: &Node) -> String {
    if let Some(s) = n.as_string_node()
        && s.opening_loc().is_none()
    {
        let value = String::from_utf8_lossy(s.unescaped());
        return format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"));
    }
    if let Some(s) = n.as_symbol_node()
        && s.opening_loc().is_none()
    {
        return helpers::ruby_string::symbol_inspect(s.unescaped());
    }
    node_text(n).to_string()
}

#[derive(Default)]
struct State {
    ignored: Vec<(usize, usize)>,
}

impl Cop for ParallelAssignment {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::MULTI_WRITE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let me = span(node);
        if cx.state::<State>().ignored.iter().any(|&(s, e)| s <= me.0 && me.1 <= e) {
            return;
        }
        let m = node.as_multi_write_node().unwrap();
        let value = m.value();
        let rescue = value.as_rescue_modifier_node();
        let rhs = rescue.as_ref().map_or(value, |r| r.expression());
        let Some(array) = rhs.as_array_node() else { return };
        let elements: Vec<Node> = array.elements().iter().collect();
        let mut targets = vec![];
        assignments(m.lefts().iter().collect(), m.rest(), m.rights().iter().collect(), &mut targets);
        if targets.len() == 1 || targets.iter().any(|t| t.as_splat_node().is_some()) {
            return;
        }
        if elements.iter().any(|e| e.as_splat_node().is_some()) || targets.len() != elements.len() {
            return;
        }
        let src = cx.src;
        let lhs: Vec<Lhs> = targets.iter().map(lhs_of).collect();
        // AssignmentSorter keys a Hash by `[lhs, rhs]` pairs, which compare structurally: repeated
        // pairs (`_, _ = X, X`) collapse into their first occurrence.
        let keep: Vec<usize> = (0..targets.len())
            .filter(|&i| {
                !(0..i).any(|j| {
                    same(src, span(&targets[i]), span(&targets[j])) && same(src, span(&elements[i]), span(&elements[j]))
                })
            })
            .collect();
        // add_self_to_getters: `(send nil? $_)` (no receiver, no arguments).
        let with_self: Vec<Rhs> = elements
            .iter()
            .map(|e| match e.as_call_node() {
                Some(c) if c.receiver().is_none() && c.arguments().is_none() && c.block().is_none() && !is_csend(&c) => {
                    Rhs::SelfGetter(c.name().as_slice().to_vec())
                }
                _ => Rhs::Real(e),
            })
            .collect();
        if tsort(src, &keep, &lhs, &with_self).is_none() {
            return;
        }
        let mut h = Heredoc(false);
        for e in &elements {
            h.visit(e);
        }
        if h.0 {
            return;
        }
        let range = (me.0, span(&rhs).1);
        cx.state::<State>().ignored.push(me);

        // Corrector.
        let plain: Vec<Rhs> = elements.iter().map(Rhs::Real).collect();
        let order = tsort(src, &keep, &lhs, &plain).unwrap();
        let assignment: Vec<String> = order
            .iter()
            .map(|&i| format!("{} = {}", node_text(&targets[i]), rhs_source(&elements[i])))
            .collect();
        let offset = " ".repeat(src.column(me.0));
        let width = cx
            .cfg
            .int("IndentationWidth")
            .or_else(|| cx.config.for_cop("Layout/IndentationWidth").int("Width"))
            .unwrap_or(2);
        let indentation = format!("{offset}{}", " ".repeat(width.max(0) as usize));
        let anc = cx.ancestors;
        let parent = helpers::parser_parent::parser_parent(anc);
        let modifier = parent.and_then(|p| {
            let kw = if let Some(i) = p.as_if_node() {
                i.if_keyword_loc().filter(|k| k.as_slice() == b"if" && i.end_keyword_loc().is_none())
            } else if let Some(u) = p.as_unless_node() {
                u.end_keyword_loc().is_none().then(|| u.keyword_loc())
            } else if let Some(w) = p.as_while_node() {
                w.closing_loc().is_none().then(|| w.keyword_loc())
            } else if let Some(u) = p.as_until_node() {
                u.closing_loc().is_none().then(|| u.keyword_loc())
            } else {
                None
            };
            kw.map(|k| (k.start_offset(), span(p)))
        });
        // `node.parent&.rescue_type?`: the sole statement of a begin/def body with a rescue clause.
        let parent_rescue = parent.and_then(|p| p.as_begin_node()).is_some_and(|b| b.rescue_clause().is_some());
        let text: &str = src.text;
        let (cs, ce, correction) = if let Some((kw, (ps, pe))) = modifier {
            let c = format!(
                "{}\n{indentation}{}\n{offset}end",
                &text[kw..pe],
                assignment.join(&format!("\n{indentation}"))
            );
            (ps, pe, c)
        } else if let (Some(r), false) = (&rescue, parent_rescue) {
            let result = node_text(&r.rescue_expression());
            let in_def = parent.is_some_and(|p| p.as_def_node().is_some_and(|d| d.receiver().is_none()));
            let c = if in_def {
                format!("{}\nrescue\n{offset}{result}", assignment.join(&format!("\n{offset}")))
            } else {
                format!(
                    "begin\n{indentation}{}\n{offset}rescue\n{indentation}{result}\n{offset}end",
                    assignment.join(&format!("\n{indentation}"))
                )
            };
            (me.0, me.1, c)
        } else {
            (me.0, me.1, assignment.join(&format!("\n{offset}")))
        };
        cx.add_fix(range.0, range.1, "Do not use parallel assignment.", |c| c.replace(cs, ce, &correction));
    }
}
