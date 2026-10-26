use crate::prelude::*;
use std::collections::HashSet;

pub const NAME: &str = "Style/ExplicitBlockArgument";
pub static COP: ExplicitBlockArgument = ExplicitBlockArgument;
pub struct ExplicitBlockArgument;

const MSG: &str = "Consider using explicit block argument in the surrounding method's signature over `yield`.";

/// Parser `args` children of block parameters; None for numbered / `it` parameters.
fn block_params<'pr>(params: Option<Node<'pr>>) -> Option<Vec<Node<'pr>>> {
    let Some(p) = params else { return Some(Vec::new()) };
    Some(helpers::block_args::children(&p.as_block_parameters_node()?))
}

/// `children.first` of a parser argument node, when it is a name.
fn param_name<'pr>(node: &Node<'pr>) -> Option<&'pr [u8]> {
    match node {
        Node::RequiredParameterNode { .. } => Some(node.as_required_parameter_node().unwrap().name().as_slice()),
        Node::OptionalParameterNode { .. } => Some(node.as_optional_parameter_node().unwrap().name().as_slice()),
        Node::RestParameterNode { .. } => node.as_rest_parameter_node().unwrap().name().map(|n| n.as_slice()),
        Node::RequiredKeywordParameterNode { .. } => {
            Some(node.as_required_keyword_parameter_node().unwrap().name().as_slice())
        }
        Node::OptionalKeywordParameterNode { .. } => {
            Some(node.as_optional_keyword_parameter_node().unwrap().name().as_slice())
        }
        Node::KeywordRestParameterNode { .. } => node.as_keyword_rest_parameter_node().unwrap().name().map(|n| n.as_slice()),
        Node::BlockParameterNode { .. } => node.as_block_parameter_node().unwrap().name().map(|n| n.as_slice()),
        Node::BlockLocalVariableNode { .. } => Some(node.as_block_local_variable_node().unwrap().name().as_slice()),
        _ => None,
    }
}

fn range(node: &Node) -> (usize, usize) {
    let l = node.location();
    (l.start_offset(), l.end_offset())
}

/// One `add_block_argument` edit, computed up front.
enum Edit {
    Insert(usize, usize, String),
    Replace(usize, usize, String),
    None,
}

impl Edit {
    fn apply(&self, c: &mut Corrector) {
        match self {
            Edit::Insert(s, e, t) => c.insert_after(*s, *e, t),
            Edit::Replace(s, e, t) => c.replace(*s, *e, t),
            Edit::None => {}
        }
    }
}

/// `block_name_taken?`: a parser `argument` or `lvasgn` anywhere in the def named `name`.
struct NameTaken<'a> {
    name: &'a [u8],
    found: bool,
}

impl<'pr> ruby_prism::Visit<'pr> for NameTaken<'_> {
    fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
        self.check(&node);
    }
    fn visit_leaf_node_enter(&mut self, node: Node<'pr>) {
        self.check(&node);
    }
    // A concrete child field: the visitor calls this without entering the node.
    fn visit_block_parameter_node(&mut self, node: &ruby_prism::BlockParameterNode<'pr>) {
        self.check(&node.as_node());
    }
}

impl NameTaken<'_> {
    fn check(&mut self, node: &Node) {
        let name = param_name(node).or_else(|| match node {
            Node::LocalVariableWriteNode { .. } => Some(node.as_local_variable_write_node().unwrap().name().as_slice()),
            Node::LocalVariableOrWriteNode { .. } => {
                Some(node.as_local_variable_or_write_node().unwrap().name().as_slice())
            }
            Node::LocalVariableAndWriteNode { .. } => {
                Some(node.as_local_variable_and_write_node().unwrap().name().as_slice())
            }
            Node::LocalVariableOperatorWriteNode { .. } => {
                Some(node.as_local_variable_operator_write_node().unwrap().name().as_slice())
            }
            Node::LocalVariableTargetNode { .. } => Some(node.as_local_variable_target_node().unwrap().name().as_slice()),
            _ => None,
        });
        self.found |= name == Some(self.name);
    }
}

/// insert_argument: `, &name` after the last argument (and its trailing commas).
fn insert_argument(text: &str, last: &Node, name: &str) -> Edit {
    if last.as_block_parameter_node().is_some() {
        return Edit::None;
    }
    let (s, mut e) = range(last);
    while text.as_bytes().get(e) == Some(&b',') {
        e += 1;
    }
    let comma = if text[s..e].ends_with(',') { "" } else { "," };
    Edit::Insert(s, e, format!("{comma} &{name}"))
}

/// build_new_arguments_for_zsuper: how a def argument is passed on (None: it is not).
fn zsuper_arg(node: &Node) -> Option<String> {
    let name = |n: &[u8]| String::from_utf8_lossy(n).into_owned();
    match node {
        Node::RequiredParameterNode { .. } => Some(name(node.as_required_parameter_node().unwrap().name().as_slice())),
        Node::OptionalParameterNode { .. } => Some(name(node.as_optional_parameter_node().unwrap().name().as_slice())),
        Node::RequiredKeywordParameterNode { .. } => {
            let n = name(node.as_required_keyword_parameter_node().unwrap().name().as_slice());
            Some(format!("{n}: {n}"))
        }
        Node::OptionalKeywordParameterNode { .. } => {
            let n = name(node.as_optional_keyword_parameter_node().unwrap().name().as_slice());
            Some(format!("{n}: {n}"))
        }
        Node::RestParameterNode { .. } | Node::KeywordRestParameterNode { .. } => Some(node_text(node).to_string()),
        _ => None,
    }
}

impl Cop for ExplicitBlockArgument {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::YIELD_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let n = cx.ancestors.len();
        if n < 3 {
            return;
        }
        // (block $_ (args $...) (yield $...)): the yield is the whole block body.
        let Some(stmts) = cx.ancestors[n - 1].as_statements_node() else { return };
        if stmts.body().iter().count() != 1 {
            return;
        }
        let holder = &cx.ancestors[n - 2];
        let (params, block_end) = match holder {
            Node::BlockNode { .. } => {
                let b = holder.as_block_node().unwrap();
                (b.parameters(), b.location().end_offset())
            }
            Node::LambdaNode { .. } => {
                let l = holder.as_lambda_node().unwrap();
                (l.parameters(), l.location().end_offset())
            }
            _ => return,
        };
        let Some(block_args) = block_params(params) else { return };
        let yield_node = node.as_yield_node().unwrap();
        let yield_args: Vec<Node> = yield_node.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default();
        if yield_args.len() != block_args.len()
            || !yield_args.iter().zip(&block_args).all(|(y, b)| {
                y.as_local_variable_read_node().is_some_and(|l| Some(l.name().as_slice()) == param_name(b))
            })
        {
            return;
        }
        // The send node of the block and the parser block range.
        let (block_start, send_end) = if holder.as_lambda_node().is_some() {
            (holder.location().start_offset(), holder.as_lambda_node().unwrap().operator_loc().end_offset())
        } else {
            let owner = &cx.ancestors[n - 3];
            match owner {
                Node::CallNode { .. } => (owner.location().start_offset(), send_range(&owner.as_call_node().unwrap()).1),
                Node::SuperNode { .. } => {
                    let s = owner.as_super_node().unwrap();
                    let end = [
                        Some(s.keyword_loc().end_offset()),
                        s.arguments().map(|a| a.location().end_offset()),
                        s.rparen_loc().map(|r| r.end_offset()),
                    ]
                    .into_iter()
                    .flatten()
                    .max()
                    .unwrap();
                    (owner.location().start_offset(), end)
                }
                Node::ForwardingSuperNode { .. } => (owner.location().start_offset(), owner.location().start_offset() + 5),
                _ => return,
            }
        };
        let Some(def) = cx.ancestors.iter().rev().find_map(|a| a.as_def_node()) else { return };
        let def_params: Vec<Node> = def.parameters().map(|p| parameter_nodes(&p)).unwrap_or_default();
        // argument_forwarding?: a block parameter can't be added next to `...`.
        if def.parameters().and_then(|p| p.keyword_rest()).is_some_and(|k| k.as_forwarding_parameter_node().is_some()) {
            return;
        }
        let def_block_arg = def_params.last().and_then(|l| l.as_block_parameter_node());
        let block_name = match &def_block_arg {
            Some(b) => b.name().map_or(String::new(), |n| String::from_utf8_lossy(n.as_slice()).into_owned()),
            None => "block".to_string(),
        };
        let text = cx.src.text;
        let zsuper = holder.as_lambda_node().is_none() && cx.ancestors[n - 3].as_forwarding_super_node().is_some();
        // can_add_block_argument?
        let name_taken = def_block_arg.is_none() && {
            let mut v = NameTaken { name: block_name.as_bytes(), found: false };
            ruby_prism::Visit::visit(&mut v, &def.as_node());
            v.found
        };
        let forwardable = def_params.iter().all(|a| {
            let anonymous_rest = a.as_rest_parameter_node().is_some_and(|r| r.name().is_none())
                || a.as_keyword_rest_parameter_node().is_some_and(|r| r.name().is_none());
            a.as_multi_target_node().is_none() && !(anonymous_rest && cx.config.target_ruby < 3.2)
        });
        if name_taken || (zsuper && !forwardable) {
            cx.add_fix(block_start, block_end, MSG, |_| {});
            return;
        }

        // add_block_argument(send_node)
        let send_edit = if holder.as_lambda_node().is_some() {
            Edit::Insert(send_end - 2, send_end, format!("(&{block_name})"))
        } else {
            let owner = &cx.ancestors[n - 3];
            let args: Vec<Node> = match owner {
                Node::CallNode { .. } => call_args(&owner.as_call_node().unwrap()),
                Node::SuperNode { .. } => {
                    owner.as_super_node().unwrap().arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default()
                }
                _ => Vec::new(),
            };
            let index_call = owner.as_call_node().filter(|c| {
                method_name(c) == "[]" && !is_csend(c) && c.call_operator_loc().is_none() && c.opening_loc().is_some()
            });
            if let Some(last) = args.last() {
                insert_argument(text, last, &block_name)
            } else if let Some(c) = index_call {
                // index_call?: `foo[] { yield }` takes the block argument inside the brackets.
                let (o, cl) = (c.opening_loc().unwrap(), c.closing_loc().unwrap());
                Edit::Replace(o.start_offset(), cl.end_offset(), format!("[&{block_name}]"))
            } else {
                let start = owner.location().start_offset();
                let (new_args, parens) = match owner {
                    Node::ForwardingSuperNode { .. } => {
                        let mut a: Vec<String> = def_params.iter().filter_map(zsuper_arg).collect();
                        a.push(format!("&{block_name}"));
                        (a.join(", "), None)
                    }
                    Node::SuperNode { .. } => {
                        let s = owner.as_super_node().unwrap();
                        (format!("&{block_name}"), s.lparen_loc().zip(s.rparen_loc()))
                    }
                    _ => {
                        let c = owner.as_call_node().unwrap();
                        let parens = c.opening_loc().filter(|o| o.as_slice() == b"(").zip(c.closing_loc());
                        (format!("&{block_name}"), parens)
                    }
                };
                // correct_call_node
                match parens {
                    Some((l, r)) => Edit::Replace(l.start_offset(), r.end_offset(), format!("({new_args})")),
                    None => Edit::Insert(start, send_end, format!("({new_args})")),
                }
            }
        };

        // add_block_argument(def_node), only for the first offense in that def.
        let def_start = def.location().start_offset();
        let first_in_def = cx.state::<HashSet<usize>>().insert(def_start);
        let def_edit = if !first_in_def {
            Edit::None
        } else if let Some(last) = def_params.last() {
            insert_argument(text, last, &block_name)
        } else if let (Some(l), Some(r)) = (def.lparen_loc(), def.rparen_loc()) {
            Edit::Replace(l.start_offset(), r.end_offset(), format!("(&{block_name})"))
        } else {
            let name = def.name_loc();
            Edit::Insert(name.start_offset(), name.end_offset(), format!("(&{block_name})"))
        };

        cx.add_fix(block_start, block_end, MSG, |c| {
            c.remove(send_end, block_end);
            send_edit.apply(c);
            def_edit.apply(c);
        });
    }
}
