use crate::helpers::gemspec_help::spec_block_var;
use crate::prelude::*;
use ruby_prism::{CallNode, Visit};

pub const NAME: &str = "Gemspec/RequireMFA";
pub static COP: RequireMFA = RequireMFA;
pub struct RequireMFA;

const MSG: &str = "`metadata['rubygems_mfa_required']` must be set to `'true'`.";
const KEY: &[u8] = b"rubygems_mfa_required";

fn parser_str<'pr>(node: &Node<'pr>) -> Option<ruby_prism::StringNode<'pr>> {
    node.as_string_node().filter(|_| !is_parser_dstr(node))
}

/// `{(str "rubygems_mfa_required") (sym :rubygems_mfa_required)}`
fn is_key(node: &Node) -> bool {
    parser_str(node).is_some_and(|s| s.unescaped() == KEY) || node.as_symbol_node().is_some_and(|s| s.unescaped() == KEY)
}

/// `(send (send _ :metadata) :[]= $_ $_)`
fn indexed_metadata<'pr>(call: &CallNode<'pr>) -> Option<(Node<'pr>, Node<'pr>)> {
    if is_csend(call) || method_name(call) != "[]=" || call.block().is_some() {
        return None;
    }
    let inner = call.receiver()?;
    let inner = inner.as_call_node()?;
    if is_csend(&inner) || method_name(&inner) != "metadata" || !send_args(&inner).is_empty() || inner.block().is_some() {
        return None;
    }
    let [k, v] = <[Node; 2]>::try_from(send_args(call)).ok()?;
    Some((k, v))
}

/// `(send _ :metadata= $_)`
fn metadata_setter<'pr>(call: &CallNode<'pr>) -> Option<Node<'pr>> {
    if is_csend(call) || method_name(call) != "metadata=" || call.block().is_some() {
        return None;
    }
    let [v] = <[Node; 1]>::try_from(send_args(call)).ok()?;
    Some(v)
}

#[derive(Default)]
struct Search<'pr> {
    /// first gem specification block variable (`gem_specification(node)`)
    spec_var: Option<&'pr [u8]>,
    /// `metadata(node)`: first matching assignment's value
    metadata: Option<Node<'pr>>,
    /// `metadata_assignment(node).to_a.last`
    last_assignment: Option<(usize, usize)>,
}

impl<'pr> Visit<'pr> for Search<'pr> {
    fn visit_call_node(&mut self, node: &CallNode<'pr>) {
        if self.spec_var.is_none() {
            self.spec_var = spec_block_var(node);
        }
        let assigns = if let Some(v) = metadata_setter(node) {
            if self.metadata.is_none() {
                self.metadata = Some(v);
            }
            true
        } else if let Some((k, v)) = indexed_metadata(node) {
            let any_key = parser_str(&k).is_some() || k.as_symbol_node().is_some();
            if self.metadata.is_none() && is_key(&k) {
                self.metadata = Some(v);
            }
            any_key
        } else {
            false
        };
        if assigns {
            let l = node.location();
            self.last_assignment = Some((l.start_offset(), l.end_offset()));
        }
        ruby_prism::visit_call_node(self, node);
    }
}

/// `rubygems_mfa_required(metadata_value).first`: first `(pair KEY $_)` in pre-order.
#[derive(Default)]
struct Pair<'pr> {
    value: Option<Node<'pr>>,
}

impl<'pr> Visit<'pr> for Pair<'pr> {
    fn visit_assoc_node(&mut self, node: &ruby_prism::AssocNode<'pr>) {
        if self.value.is_none() && is_key(&node.key()) {
            self.value = Some(node.value());
        }
        ruby_prism::visit_assoc_node(self, node);
    }
}

/// Range of an mfa value and whether it is `(str "true")`.
fn mfa(v: &Node) -> (usize, usize, bool) {
    let l = v.location();
    (l.start_offset(), l.end_offset(), parser_str(v).is_some_and(|s| s.unescaped() == b"true"))
}

impl Cop for RequireMFA {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE, kind::LAMBDA_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // on_block: plain blocks only (numblock/itblock have no handler).
        let (block_params, end_loc) = if let Some(call) = node.as_call_node() {
            let Some(b) = call.block().and_then(|b| b.as_block_node()) else { return };
            (b.parameters(), b.closing_loc())
        } else {
            let l = node.as_lambda_node().unwrap();
            (l.parameters(), l.closing_loc())
        };
        if block_params.is_some_and(|p| p.as_block_parameters_node().is_none()) {
            return;
        }
        let mut search = Search::default();
        search.visit(node);
        let Some(block_var) = search.spec_var else { return };
        let mfa_value = match &search.metadata {
            None => None,
            Some(m) if parser_str(m).is_some() => Some(mfa(m)),
            Some(m) => {
                let mut p = Pair::default();
                p.visit(m);
                p.value.as_ref().map(mfa)
            }
        };
        if let Some((s, e, is_true)) = mfa_value {
            if !is_true {
                cx.add_fix(s, e, MSG, |c| c.replace(s, e, "'true'"));
            }
            return;
        }
        let l = node.location();
        let (s, e) = (l.start_offset(), l.end_offset());
        match &search.metadata {
            // `return unless metadata.hash_type?`: offense without an edit
            Some(m) => match m.as_hash_node() {
                None => cx.add_fix(s, e, MSG, |_| {}),
                Some(hash) => {
                    if let Some(last) = hash.elements().iter().filter(|p| p.as_assoc_node().is_some()).last() {
                        let ll = last.location();
                        let (ps, pe) = (ll.start_offset(), ll.end_offset());
                        cx.add_fix(s, e, MSG, |c| c.insert_after(ps, pe, ",\n'rubygems_mfa_required' => 'true'"));
                    } else {
                        let cl = hash.closing_loc();
                        let (cs, ce) = (cl.start_offset(), cl.end_offset());
                        cx.add_fix(s, e, MSG, |c| c.insert_before(cs, ce, "'rubygems_mfa_required' => 'true'"));
                    }
                }
            },
            None => {
                let var = String::from_utf8_lossy(block_var);
                let directive = format!("{var}.metadata['rubygems_mfa_required'] = 'true'");
                if let Some((as_, ae)) = search.last_assignment {
                    let text = format!("\n{directive}");
                    cx.add_fix(s, e, MSG, |c| c.insert_after(as_, ae, &text));
                } else {
                    let (cs, ce) = (end_loc.start_offset(), end_loc.end_offset());
                    let text = format!("{directive}\n");
                    cx.add_fix(s, e, MSG, |c| c.insert_before(cs, ce, &text));
                }
            }
        }
    }
}
