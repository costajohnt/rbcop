use crate::prelude::*;
use ruby_prism::{CallNode, Visit};

pub const NAME: &str = "Lint/OutOfRangeRegexpRef";
pub static COP: OutOfRangeRegexpRef = OutOfRangeRegexpRef;
pub struct OutOfRangeRegexpRef;

const RECEIVER_METHODS: &[&str] = &["=~", "===", "match"];
const ARGUMENT_METHODS: &[&str] = &[
    "=~", "match", "grep", "gsub", "gsub!", "sub", "sub!", "[]", "slice", "slice!", "index", "rindex", "scan",
    "partition", "rpartition", "start_with?", "end_with?",
];

fn is_regexp(node: &Node) -> bool {
    matches!(kind::of(node), kind::REGULAR_EXPRESSION_NODE | kind::INTERPOLATED_REGULAR_EXPRESSION_NODE)
}

/// rubocop-ast `basic_literal?` minus `str`: int float sym true false nil.
fn basic_non_str_literal(node: &Node) -> bool {
    matches!(
        kind::of(node),
        kind::INTEGER_NODE
            | kind::FLOAT_NODE
            | kind::SYMBOL_NODE
            | kind::TRUE_NODE
            | kind::FALSE_NODE
            | kind::NIL_NODE
    )
}

/// The cop's walk in parser-gem order: `@valid_ref` is updated by `on_*` hooks before children
/// and by `after_send` after the send's children (but before an attached block).
struct Walk {
    valid_ref: Option<usize>,
    offenses: Vec<(usize, usize, String)>,
}

impl Walk {
    fn check_regexp(&mut self, node: &Node) -> Option<usize> {
        let (named, numbered) = helpers::regexp_captures::regexp_captures(node)?;
        let n = if named > 0 { named } else { numbered };
        self.valid_ref = Some(n);
        Some(n)
    }

    /// after_send / after_csend for a call `name` with these receiver and arguments.
    fn after_send(&mut self, name: &str, receiver: Option<Node>, first: Option<Node>) {
        if !RECEIVER_METHODS.contains(&name) && !ARGUMENT_METHODS.contains(&name) {
            return;
        }
        let regexp_receiver = receiver.as_ref().is_some_and(is_regexp);
        // backrefs_unaffected?
        if first.as_ref().is_some_and(basic_non_str_literal) && !regexp_receiver {
            return;
        }
        self.valid_ref = None;
        if let Some(f) = first.filter(|f| is_regexp(f) && ARGUMENT_METHODS.contains(&name)) {
            self.check_regexp(&f);
        } else if regexp_receiver {
            self.check_regexp(&receiver.unwrap());
        }
    }

    fn visit_index_write(&mut self, receiver: Option<Node<'_>>, args: Option<ruby_prism::ArgumentsNode<'_>>) {
        if let Some(r) = &receiver {
            self.visit(r);
        }
        if let Some(a) = &args {
            self.visit_arguments_node(a);
        }
        let first = args.and_then(|a| a.arguments().iter().next());
        self.after_send("[]", receiver, first);
    }
}

impl<'pr> Visit<'pr> for Walk {
    fn visit_call_node(&mut self, node: &CallNode<'pr>) {
        if let Some(r) = node.receiver() {
            self.visit(&r);
        }
        if let Some(a) = node.arguments() {
            self.visit_arguments_node(&a);
        }
        let block = node.block();
        if let Some(b) = block.as_ref().filter(|b| b.as_block_argument_node().is_some()) {
            self.visit(b);
        }
        self.after_send(method_name(node), node.receiver(), send_args(node).into_iter().next());
        if let Some(b) = block.filter(|b| b.as_block_node().is_some()) {
            self.visit(&b);
        }
    }

    fn visit_index_operator_write_node(&mut self, node: &ruby_prism::IndexOperatorWriteNode<'pr>) {
        self.visit_index_write(node.receiver(), node.arguments());
        self.visit(&node.value());
    }

    fn visit_index_or_write_node(&mut self, node: &ruby_prism::IndexOrWriteNode<'pr>) {
        self.visit_index_write(node.receiver(), node.arguments());
        self.visit(&node.value());
    }

    fn visit_index_and_write_node(&mut self, node: &ruby_prism::IndexAndWriteNode<'pr>) {
        self.visit_index_write(node.receiver(), node.arguments());
        self.visit(&node.value());
    }

    /// parser `match_with_lvasgn`: no send, so no after_send.
    fn visit_match_write_node(&mut self, node: &ruby_prism::MatchWriteNode<'pr>) {
        let call = node.call();
        if let Some(r) = call.receiver() {
            self.check_regexp(&r);
            self.visit(&r);
        }
        if let Some(a) = call.arguments() {
            self.visit_arguments_node(&a);
        }
    }

    fn visit_when_node(&mut self, node: &ruby_prism::WhenNode<'pr>) {
        let regexps: Vec<Node> = node.conditions().iter().filter(is_regexp).collect();
        self.valid_ref = regexps.iter().filter_map(|r| self.check_regexp(r)).max();
        ruby_prism::visit_when_node(self, node);
    }

    fn visit_in_node(&mut self, node: &ruby_prism::InNode<'pr>) {
        // Prism wraps a guarded pattern in an IfNode/UnlessNode; parser keeps pattern and guard apart.
        let pat = node.pattern();
        let (pattern, guard) = if let Some(i) = pat.as_if_node() {
            (i.statements().and_then(|s| s.body().iter().next()), Some(i.predicate()))
        } else if let Some(u) = pat.as_unless_node() {
            (u.statements().and_then(|s| s.body().iter().next()), Some(u.predicate()))
        } else {
            (Some(pat), None)
        };
        let mut found = None;
        if let Some(p) = &pattern {
            let mut c = CheckIn { walk: self, max: None };
            if is_regexp(p) {
                c.check(p);
            } else {
                c.visit(p);
            }
            found = c.max;
        }
        self.valid_ref = found;
        if let Some(p) = &pattern {
            self.visit(p);
        }
        if let Some(g) = &guard {
            self.visit(g);
        }
        if let Some(s) = node.statements() {
            self.visit_statements_node(&s);
        }
    }

    fn visit_numbered_reference_read_node(&mut self, node: &ruby_prism::NumberedReferenceReadNode<'pr>) {
        let backref = node.number() as usize;
        let Some(valid) = self.valid_ref.filter(|&v| backref > v) else { return };
        let count = if valid == 0 { "no".to_string() } else { valid.to_string() };
        let group = if valid == 1 { "group" } else { "groups" };
        let loc = node.location();
        let msg = format!("${backref} is out of range ({count} regexp capture {group} detected).");
        self.offenses.push((loc.start_offset(), loc.end_offset(), msg));
    }
}

/// Runs check_regexp on each regexp of a pattern, in order, keeping the max.
struct CheckIn<'a> {
    walk: &'a mut Walk,
    max: Option<usize>,
}

impl CheckIn<'_> {
    fn check(&mut self, node: &Node) {
        if let Some(n) = self.walk.check_regexp(node) {
            self.max = Some(self.max.map_or(n, |m| m.max(n)));
        }
    }
}

impl<'pr> Visit<'pr> for CheckIn<'_> {
    fn visit_regular_expression_node(&mut self, node: &ruby_prism::RegularExpressionNode<'pr>) {
        self.check(&node.as_node());
    }

    fn visit_interpolated_regular_expression_node(&mut self, node: &ruby_prism::InterpolatedRegularExpressionNode<'pr>) {
        self.check(&node.as_node());
        ruby_prism::visit_interpolated_regular_expression_node(self, node);
    }
}

impl Cop for OutOfRangeRegexpRef {
    fn kinds(&self) -> &'static [usize] {
        &[kind::PROGRAM_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let mut w = Walk { valid_ref: Some(0), offenses: Vec::new() };
        w.visit(node);
        for (s, e, msg) in w.offenses {
            cx.add(s, e, msg);
        }
    }
}
