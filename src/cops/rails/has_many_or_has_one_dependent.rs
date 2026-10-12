use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Rails/HasManyOrHasOneDependent";
pub static COP: HasManyOrHasOneDependent = HasManyOrHasOneDependent;
pub struct HasManyOrHasOneDependent;

/// A parser node above the visited send: the send's own block, or a Prism ancestor index (a
/// BlockNode there stands for parser's block, whose send is the call before it).
#[derive(Clone, Copy, PartialEq)]
enum P {
    Own,
    Idx(usize),
}

struct Tree<'a, 'pr> {
    anc: &'a [Node<'pr>],
    node: &'a Node<'pr>,
}

impl<'pr> Tree<'_, 'pr> {
    fn parent(&self, p: P) -> Option<P> {
        let i = match p {
            P::Own => self.anc.len(),
            P::Idx(i) => i,
        };
        helpers::parser_parent::parser_parent_index(self.anc, i).map(P::Idx)
    }

    /// The node's parser parent: its own block, else the Prism parser parent.
    fn first_parent(&self) -> Option<P> {
        let own = self.node.as_call_node().and_then(|c| c.block()).is_some_and(|b| b.as_block_node().is_some());
        if own { Some(P::Own) } else { self.parent(P::Own) }
    }

    /// (block call, BlockNode) when `p` is a parser block (any kind; check numbered separately).
    fn block(&self, p: P) -> Option<(ruby_prism::CallNode<'pr>, ruby_prism::BlockNode<'pr>)> {
        match p {
            P::Own => {
                let c = self.node.as_call_node()?;
                let b = c.block()?.as_block_node()?;
                Some((c, b))
            }
            P::Idx(i) => {
                let b = self.anc[i].as_block_node()?;
                let c = self.anc.get(i.checked_sub(1)?)?.as_call_node()?;
                Some((c, b))
            }
        }
    }

    /// Visit the parser subtree of `p` (a block's subtree includes its send).
    fn visit_subtree<V: Visit<'pr>>(&self, p: P, v: &mut V) {
        match p {
            P::Own => v.visit(self.node),
            P::Idx(i) if self.block(p).is_some() => v.visit(&self.anc[i - 1]),
            P::Idx(i) => v.visit(&self.anc[i]),
        }
    }

    fn is_begin(&self, p: P) -> bool {
        match p {
            P::Own => false,
            P::Idx(i) => {
                self.anc[i].as_parentheses_node().is_some()
                    || self.anc[i].as_statements_node().is_some_and(|s| s.body().iter().count() > 1)
            }
        }
    }
}

fn not_numbered(b: &ruby_prism::BlockNode) -> bool {
    !b.parameters().is_some_and(|p| p.as_numbered_parameters_node().is_some() || p.as_it_parameters_node().is_some())
}

/// valid_options?: (after unwrapping `**{...}`) a `dependent:` pair or a non-nil `through:` pair.
/// None when the options are empty (`options.first.kwsplat_type?` raises).
fn valid_options(elems: &[Node]) -> Option<bool> {
    let first = elems.first()?;
    let pairs: Vec<ruby_prism::AssocNode> = match first.as_assoc_splat_node().and_then(|s| s.value()) {
        Some(v) if v.as_hash_node().is_some() => helpers::rails::hash_pairs(&v),
        _ => elems.iter().filter_map(|e| e.as_assoc_node()).collect(),
    };
    Some(pairs.iter().any(|p| {
        let k = helpers::rails::sym_value(&p.key());
        k.as_deref() == Some("dependent") || (k.as_deref() == Some("through") && p.value().as_nil_node().is_none())
    }))
}

impl Cop for HasManyOrHasOneDependent {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        if !matches!(method_name(&call), "has_many" | "has_one") {
            return;
        }
        let t = Tree { anc: cx.ancestors, node };
        let parent = t.first_parent();
        // active_resource?(node.parent) / readonly_model?(node): searches of the parent's subtree.
        if let Some(p) = parent {
            struct S(bool, bool);
            impl<'pr> Visit<'pr> for S {
                fn visit_constant_path_node(&mut self, n: &ruby_prism::ConstantPathNode<'pr>) {
                    if helpers::rails::is_const(&n.as_node(), "ActiveResource::Base") {
                        self.0 = true;
                    }
                    ruby_prism::visit_constant_path_node(self, n);
                }
                fn visit_def_node(&mut self, d: &ruby_prism::DefNode<'pr>) {
                    let body = helpers::rails::body_statements(d.body());
                    if d.receiver().is_none()
                        && d.name().as_slice() == b"readonly?"
                        && d.parameters().is_none()
                        && matches!(body.as_slice(), [b] if b.as_true_node().is_some())
                    {
                        self.1 = true;
                    }
                    ruby_prism::visit_def_node(self, d);
                }
            }
            let mut s = S(false, false);
            t.visit_subtree(p, &mut s);
            if s.0 || s.1 {
                return;
            }
        }
        let args = send_args(&call);
        if args.len() != 1 {
            // association_with_options?: (send _ {...} ... (hash $...))
            if let Some(elems) = args.last().and_then(helpers::rails::hash_elements) {
                match valid_options(&elems) {
                    None => return,
                    Some(true) => return,
                    Some(false) => {}
                }
            }
        }
        // valid_options_in_with_options_block?
        let Some(p) = parent else { return };
        let ext = t.block(p).is_some_and(|(c, b)| {
            not_numbered(&b)
                && b.parameters().is_none()
                && helpers::rails::is_command(&c, "has_many")
                && send_args(&c).len() == 1
        });
        let mut n = if t.is_begin(p) || ext { t.parent(p) } else { Some(p) };
        // contain_valid_options_in_with_options_block?
        while let Some(cur) = n {
            let Some((c, b)) = t.block(cur) else { break };
            let args = send_args(&c);
            let ok_params = b.parameters().is_none_or(|ps| {
                ps.as_block_parameters_node()
                    .and_then(|bp| bp.parameters())
                    .is_none_or(|pp| parameter_nodes(&pp).len() <= 1)
            });
            if !not_numbered(&b) || !helpers::rails::is_command(&c, "with_options") || !ok_params {
                break;
            }
            let [h] = args.as_slice() else { break };
            let Some(elems) = helpers::rails::hash_elements(h) else { break };
            match valid_options(&elems) {
                None => return,
                Some(true) => return,
                Some(false) => {}
            }
            n = t.parent(cur).and_then(|x| t.parent(x));
        }
        let sel = call.message_loc().unwrap();
        cx.add(sel.start_offset(), sel.end_offset(), "Specify a `:dependent` option.");
    }
}
