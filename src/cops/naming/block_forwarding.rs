use crate::prelude::*;
use helpers::add_parentheses::{self as parens, ParenFix};
use ruby_prism::Visit;
use std::collections::HashMap;

pub const NAME: &str = "Naming/BlockForwarding";
pub static COP: BlockForwarding = BlockForwarding;
pub struct BlockForwarding;

/// A parser `any_block` around (or, for a call/super owning a `{ }`/`do` block, wrapping) the node.
fn block_like(node: &Node) -> bool {
    let owns_block = |b: Option<Node>| b.is_some_and(|b| b.as_block_node().is_some());
    match kind::of(node) {
        kind::BLOCK_NODE | kind::LAMBDA_NODE => true,
        kind::CALL_NODE => owns_block(node.as_call_node().unwrap().block()),
        kind::SUPER_NODE => owns_block(node.as_super_node().unwrap().block()),
        kind::FORWARDING_SUPER_NODE => owns_block(node.as_forwarding_super_node().unwrap().block().map(|b| b.as_node())),
        _ => false,
    }
}

/// `each_descendant(:block_pass)` in pre-order, with the parent's `add_parentheses` plan (None when
/// the parent is a parenthesized call) and whether a parser block encloses the block_pass.
/// (range, source or "" for `&:sym`, parent paren plan, inside a parser block)
type Found<'s> = ((usize, usize), &'s str, Option<ParenFix>, bool);

struct BlockPasses<'s> {
    text: &'s str,
    depth: usize,
    parents: HashMap<usize, Option<ParenFix>>,
    found: Vec<Found<'s>>,
}

impl<'s> BlockPasses<'s> {
    fn note_parent(&mut self, node: &Node, block: Option<Node>, parenthesized: bool) {
        if let Some(b) = block.filter(|b| b.as_block_argument_node().is_some()) {
            let fix = if parenthesized { None } else { parens::plan(node, self.text) };
            self.parents.insert(b.location().start_offset(), fix);
        }
    }

    fn nest(&mut self, node: &Node, f: impl FnOnce(&mut Self)) {
        let b = block_like(node);
        self.depth += b as usize;
        f(self);
        self.depth -= b as usize;
    }
}

impl<'pr, 's> Visit<'pr> for BlockPasses<'s>
where
    'pr: 's,
{
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'pr>) {
        let n = node.as_node();
        let paren = node.opening_loc().is_some_and(|l| l.as_slice() == b"(");
        self.note_parent(&n, node.block(), paren);
        self.nest(&n, |s| ruby_prism::visit_call_node(s, node));
    }
    fn visit_super_node(&mut self, node: &ruby_prism::SuperNode<'pr>) {
        let n = node.as_node();
        self.note_parent(&n, node.block(), node.lparen_loc().is_some());
        self.nest(&n, |s| ruby_prism::visit_super_node(s, node));
    }
    fn visit_forwarding_super_node(&mut self, node: &ruby_prism::ForwardingSuperNode<'pr>) {
        self.nest(&node.as_node(), |s| ruby_prism::visit_forwarding_super_node(s, node));
    }
    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode<'pr>) {
        self.nest(&node.as_node(), |s| ruby_prism::visit_block_node(s, node));
    }
    fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode<'pr>) {
        self.nest(&node.as_node(), |s| ruby_prism::visit_lambda_node(s, node));
    }
    fn visit_block_argument_node(&mut self, node: &ruby_prism::BlockArgumentNode<'pr>) {
        let l = node.location();
        let sym = node.expression().is_some_and(|e| e.as_symbol_node().is_some());
        let parent = self.parents.remove(&l.start_offset()).flatten();
        // `&:sym` never matches (`block_pass_node.children.first&.sym_type?`)
        let text: &'s str = if sym { "" } else { text(&l) };
        self.found.push(((l.start_offset(), l.end_offset()), text, parent, self.depth > 0));
        ruby_prism::visit_block_argument_node(self, node);
    }
}

/// `body.each_node(:lvar, :lvasgn).any? { |n| !n.parent.block_pass_type? && name == n.name }`
struct LocalUse<'n> {
    name: &'n [u8],
    in_pattern: bool,
    found: bool,
}

impl<'pr> Visit<'pr> for LocalUse<'_> {
    fn visit_local_variable_read_node(&mut self, node: &ruby_prism::LocalVariableReadNode<'pr>) {
        self.found |= node.name().as_slice() == self.name;
    }
    fn visit_block_argument_node(&mut self, node: &ruby_prism::BlockArgumentNode<'pr>) {
        match node.expression() {
            Some(e) if e.as_local_variable_read_node().is_some() => {}
            _ => ruby_prism::visit_block_argument_node(self, node),
        }
    }
    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode<'pr>) {
        self.found |= node.name().as_slice() == self.name;
        ruby_prism::visit_local_variable_write_node(self, node);
    }
    fn visit_local_variable_operator_write_node(&mut self, node: &ruby_prism::LocalVariableOperatorWriteNode<'pr>) {
        self.found |= node.name().as_slice() == self.name;
        ruby_prism::visit_local_variable_operator_write_node(self, node);
    }
    fn visit_local_variable_or_write_node(&mut self, node: &ruby_prism::LocalVariableOrWriteNode<'pr>) {
        self.found |= node.name().as_slice() == self.name;
        ruby_prism::visit_local_variable_or_write_node(self, node);
    }
    fn visit_local_variable_and_write_node(&mut self, node: &ruby_prism::LocalVariableAndWriteNode<'pr>) {
        self.found |= node.name().as_slice() == self.name;
        ruby_prism::visit_local_variable_and_write_node(self, node);
    }
    fn visit_local_variable_target_node(&mut self, node: &ruby_prism::LocalVariableTargetNode<'pr>) {
        // Inside a pattern this is parser `match_var`, not `lvasgn`.
        self.found |= !self.in_pattern && node.name().as_slice() == self.name;
    }
    fn visit_match_write_node(&mut self, node: &ruby_prism::MatchWriteNode<'pr>) {
        // parser `match_with_lvasgn` has no lvasgn children
        self.visit_call_node(&node.call());
    }
    fn visit_in_node(&mut self, node: &ruby_prism::InNode<'pr>) {
        let saved = self.in_pattern;
        self.in_pattern = true;
        self.visit(&node.pattern());
        self.in_pattern = saved;
        if let Some(s) = node.statements() {
            self.visit_statements_node(&s);
        }
    }
    fn visit_match_predicate_node(&mut self, node: &ruby_prism::MatchPredicateNode<'pr>) {
        self.visit(&node.value());
        let saved = std::mem::replace(&mut self.in_pattern, true);
        self.visit(&node.pattern());
        self.in_pattern = saved;
    }
    fn visit_match_required_node(&mut self, node: &ruby_prism::MatchRequiredNode<'pr>) {
        self.visit(&node.value());
        let saved = std::mem::replace(&mut self.in_pattern, true);
        self.visit(&node.pattern());
        self.in_pattern = saved;
    }
}

fn uses_local(body: Option<Node>, name: &str) -> bool {
    let Some(body) = body else { return false };
    let mut v = LocalUse { name: name.as_bytes(), in_pattern: false, found: false };
    v.visit(&body);
    v.found
}

impl Cop for BlockForwarding {
    fn correctable(&self) -> bool {
        true
    }

    fn ruby_range(&self) -> (f64, f64) {
        (3.1, f64::MAX)
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let def = node.as_def_node().unwrap();
        let Some(params) = def.parameters() else { return };
        let args = parameter_nodes(&params);
        let Some(last) = args.last() else { return };
        let anonymous = cx.cfg.str("EnforcedStyle") != Some("explicit");
        let blockarg = last.as_block_parameter_node();
        let last_text = node_text(last);
        let expected = if anonymous {
            blockarg.is_none_or(|b| b.name().is_none())
                || params.keywords().iter().any(|k| {
                    k.as_required_keyword_parameter_node().is_some() || k.as_optional_keyword_parameter_node().is_some()
                })
                || uses_local(def.body(), last_text.get(1..).unwrap_or(""))
        } else {
            blockarg.is_none_or(|b| b.name().is_some())
        };
        if expected {
            return;
        }
        let src = cx.src;
        let mut v = BlockPasses { text: src.text, depth: 0, parents: HashMap::new(), found: Vec::new() };
        v.visit(node);
        let outer_block = cx.ancestors.iter().any(block_like);
        let old_ruby = cx.config.target_ruby <= 3.3;
        if old_ruby && v.found.iter().any(|f| f.3 || outer_block) {
            return;
        }
        let name = cx.cfg.str("BlockForwardingName").unwrap_or("block").to_string();
        let msg = format!("Use {} block forwarding.", if anonymous { "anonymous" } else { "explicit" });
        let explicit_fix = (!anonymous && !uses_local(def.body(), &name)).then(|| format!("&{name}"));
        let mut offenses: Vec<((usize, usize), Option<ParenFix>)> =
            v.found.into_iter().filter(|f| f.1 == last_text).map(|f| (f.0, f.2)).collect();
        let l = last.location();
        let params_fix = if def.lparen_loc().is_some() { None } else { parens::plan(&params.as_node(), src.text) };
        offenses.push(((l.start_offset(), l.end_offset()), params_fix));
        for ((start, end), fix) in offenses {
            let explicit_fix = explicit_fix.clone();
            cx.add_fix(start, end, msg.clone(), move |c| {
                if anonymous {
                    c.replace(start, end, "&");
                    if let Some(f) = &fix {
                        parens::apply(c, f);
                    }
                } else if let Some(r) = &explicit_fix {
                    c.replace(start, end, r);
                }
            });
        }
    }
}
