use crate::prelude::*;
use helpers::allowed::{allowed_method, matches_allowed_pattern};
use helpers::macro_scope::in_macro_scope;

pub const NAME: &str = "Style/MethodCallWithArgsParentheses";
pub static COP: MethodCallWithArgsParentheses = MethodCallWithArgsParentheses;
pub struct MethodCallWithArgsParentheses;

const OPERATOR_METHODS: &[&str] = &[
    "|", "^", "&", "<=>", "==", "===", "=~", ">", ">=", "<", "<=", "<<", ">>", "+", "-", "*", "/", "%", "**", "~",
    "+@", "-@", "!@", "~@", "[]", "[]=", "!", "!=", "!~", "`",
];

/// The parts of a parser send/csend/yield node this cop looks at.
struct Dispatch<'pr> {
    name: &'pr str,
    /// `loc.selector` (`loc.keyword` for yield)
    selector: Option<(usize, usize)>,
    args: Vec<Node<'pr>>,
    /// `loc.end` is `)`
    parenthesized: bool,
    range: (usize, usize),
    receiver: bool,
    setter: bool,
    /// `loc.begin` / `loc.end` parentheses
    open: Option<(usize, usize)>,
    close: Option<(usize, usize)>,
    call: Option<ruby_prism::CallNode<'pr>>,
}

fn dispatch<'pr>(node: &Node<'pr>) -> Option<Dispatch<'pr>> {
    let span = |l: ruby_prism::Location| (l.start_offset(), l.end_offset());
    if let Some(call) = node.as_call_node() {
        return Some(Dispatch {
            name: method_name(&call),
            selector: call.message_loc().map(span),
            args: send_args(&call),
            parenthesized: call.closing_loc().is_some_and(|l| l.as_slice() == b")"),
            range: send_range(&call),
            receiver: call.receiver().is_some(),
            setter: call.equal_loc().is_some(),
            open: call.opening_loc().filter(|l| l.as_slice() == b"(").map(span),
            close: call.closing_loc().filter(|l| l.as_slice() == b")").map(span),
            call: Some(call),
        });
    }
    let y = node.as_yield_node()?;
    Some(Dispatch {
        name: "yield",
        selector: Some(span(y.keyword_loc())),
        args: y.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default(),
        parenthesized: y.rparen_loc().is_some(),
        range: span(y.location()),
        receiver: false,
        setter: false,
        open: y.lparen_loc().map(span),
        close: y.rparen_loc().map(span),
        call: None,
    })
}

impl Cop for MethodCallWithArgsParentheses {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE, kind::YIELD_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(d) = dispatch(node) else { return };
        if cx.cfg.str("EnforcedStyle") == Some("omit_parentheses") {
            omit::check(cx, node, &d);
            return;
        }
        require_parentheses(cx, node, &d);
    }

    fn on_file(&self, cx: &mut Cx) {
        omit::report(cx);
    }
}

/// The `omit_parentheses` style (OmitParentheses).
mod omit {
    use super::{Dispatch, OPERATOR_METHODS, args_parenthesized, next_char_end};
    use crate::helpers::parser_parent::parser_parent_index;
    use crate::helpers::range_help::{Side, Surrounding, range_with_surrounding_space};
    use crate::helpers::reparsed_equivalence::verified_by_reparse_with;
    use crate::prelude::*;
    use ruby_prism::Visit;

    const MSG: &str = "Omit parentheses for method calls with arguments.";

    /// A pending offense: `(`..`)`, the args-begin range and its replacement.
    #[derive(Clone, Copy)]
    struct Pending {
        open: (usize, usize),
        close: (usize, usize),
        args_begin: (usize, usize),
        replacement: &'static str,
    }

    #[derive(Default)]
    struct State(Vec<Pending>);

    fn apply(c: &mut Corrector, p: Pending) {
        c.replace(p.args_begin.0, p.args_begin.1, p.replacement);
        c.remove(p.close.0, p.close.1);
    }

    /// `on_investigation_end`: report the pending offenses whose corrections reparse equivalently.
    pub fn report(cx: &mut Cx) {
        let pending = std::mem::take(&mut cx.state::<State>().0);
        if pending.is_empty() {
            return;
        }
        let src = cx.src;
        let mut buf = Vec::new();
        let (result, ..) = crate::runner::parse_for_target(src.text, &mut buf, src.version, src.target_ruby);
        let program = result.node();
        let verified =
            verified_by_reparse_with(src, &program, &pending, |p| (p.open.0, p.close.1), apply, true);
        for p in verified {
            cx.add_fix(p.open.0, p.close.1, MSG, |c| apply(c, p));
        }
    }

    /// A parser parent of the call: its own block, or a Prism ancestor (index into `cx.ancestors`).
    #[derive(Clone, Copy, PartialEq)]
    enum P {
        /// The call itself.
        Call,
        OwnBlock,
        Anc(usize),
    }

    struct Ctx<'a, 'pr> {
        anc: &'a [Node<'pr>],
        node: &'a Node<'pr>,
        own_block: Option<ruby_prism::BlockNode<'pr>>,
        /// The send's source range.
        call_range: (usize, usize),
    }

    impl<'pr> Ctx<'_, 'pr> {
        fn parent(&self) -> Option<P> {
            if self.own_block.is_some() {
                return Some(P::OwnBlock);
            }
            parser_parent_index(self.anc, self.anc.len()).map(P::Anc)
        }
        fn parent_of(&self, p: P) -> Option<P> {
            match p {
                P::Call => self.parent(),
                P::OwnBlock => parser_parent_index(self.anc, self.anc.len()).map(P::Anc),
                P::Anc(i) => parser_parent_index(self.anc, i).map(P::Anc),
            }
        }
        fn node_of(&self, p: P) -> Option<&Node<'pr>> {
            match p {
                P::Call => Some(self.node),
                P::OwnBlock => None,
                P::Anc(i) => Some(&self.anc[i]),
            }
        }
        fn is_block(&self, p: P) -> bool {
            p == P::OwnBlock
                || self.node_of(p).is_some_and(|n| matches!(kind::of(n), kind::BLOCK_NODE | kind::LAMBDA_NODE))
        }
        /// `node.parent&.any_block_type? ? node.parent.parent : node.parent`
        fn parent_skipping_block(&self, x: P) -> Option<P> {
            let p = self.parent_of(x)?;
            if self.is_block(p) { self.parent_of(p) } else { Some(p) }
        }
        fn is_assignment(&self, p: P) -> bool {
            self.node_of(p).is_some_and(is_assignment)
        }
        /// `assigned_before?(node, target)`: `node` is an assignment whose operator comes before
        /// `target` starts.
        fn assigned_before(&self, node: P, target: P) -> bool {
            let Some(n) = self.node_of(node).filter(|n| is_assignment(n)) else { return false };
            let op = match n.as_call_node() {
                Some(c) => c.equal_loc().map(|l| l.start_offset()),
                None => assignment_parts(n).map(|(op, _)| op.start_offset()),
            };
            op.is_some_and(|op| op < self.range_of(target).0)
        }
        /// Parser source range of a parent.
        fn range_of(&self, p: P) -> (usize, usize) {
            match p {
                P::Call => self.call_range,
                P::OwnBlock => span(self.node),
                P::Anc(i) => {
                    let n = &self.anc[i];
                    if kind::of(n) == kind::BLOCK_NODE && i > 0 {
                        (self.anc[i - 1].location().start_offset(), n.location().end_offset())
                    } else {
                        span(n)
                    }
                }
            }
        }
        fn kind_of(&self, p: P) -> Option<usize> {
            self.node_of(p).map(kind::of)
        }
    }

    fn span(n: &Node) -> (usize, usize) {
        (n.location().start_offset(), n.location().end_offset())
    }

    fn is_conditional(k: usize) -> bool {
        matches!(
            k,
            kind::IF_NODE
                | kind::UNLESS_NODE
                | kind::WHILE_NODE
                | kind::UNTIL_NODE
                | kind::CASE_NODE
                | kind::CASE_MATCH_NODE
        )
    }

    /// `node.type?(:call, :super, :yield)` (`call` is send/csend).
    fn is_call_super_yield(n: &Node) -> bool {
        matches!(kind::of(n), kind::CALL_NODE | kind::SUPER_NODE | kind::YIELD_NODE)
    }

    /// `child` is the receiver or an index argument of `a.b op= v` / `a[i] op= v`, whose parser
    /// target is a `send`.
    fn in_write_target(p: &Node, child: (usize, usize)) -> bool {
        let value = match p {
            Node::IndexOperatorWriteNode { .. } => p.as_index_operator_write_node().unwrap().value(),
            Node::IndexOrWriteNode { .. } => p.as_index_or_write_node().unwrap().value(),
            Node::IndexAndWriteNode { .. } => p.as_index_and_write_node().unwrap().value(),
            Node::CallOperatorWriteNode { .. } => p.as_call_operator_write_node().unwrap().value(),
            Node::CallOrWriteNode { .. } => p.as_call_or_write_node().unwrap().value(),
            Node::CallAndWriteNode { .. } => p.as_call_and_write_node().unwrap().value(),
            _ => return false,
        };
        span(&value) != child
    }

    /// `logical_operator?`: `&&` / `||`.
    fn logical_operator(n: &Node) -> bool {
        match n {
            Node::AndNode { .. } => n.as_and_node().unwrap().operator_loc().as_slice() == b"&&",
            Node::OrNode { .. } => n.as_or_node().unwrap().operator_loc().as_slice() == b"||",
            _ => false,
        }
    }

    /// `splat?`: parser `splat`, `kwsplat` or `block_pass`. Anonymous `*` / `**` are parser's
    /// `forwarded_restarg` / `forwarded_kwrestarg`, not splats; anonymous `&` is still a `block_pass`.
    fn is_splat(n: &Node) -> bool {
        match n {
            Node::SplatNode { .. } => n.as_splat_node().unwrap().expression().is_some(),
            Node::AssocSplatNode { .. } => n.as_assoc_splat_node().unwrap().value().is_some(),
            Node::BlockArgumentNode { .. } => true,
            _ => false,
        }
    }

    fn is_ternary(n: &Node) -> bool {
        n.as_if_node().is_some_and(|i| i.if_keyword_loc().is_none())
    }

    fn is_numeric(n: &Node) -> bool {
        matches!(kind::of(n), kind::INTEGER_NODE | kind::FLOAT_NODE | kind::RATIONAL_NODE | kind::IMAGINARY_NODE)
    }

    /// `unary_operation?` on a send.
    fn unary_operation(n: &Node) -> bool {
        n.as_call_node().is_some_and(|c| {
            !is_csend(&c)
                && OPERATOR_METHODS.contains(&method_name(&c))
                && c.message_loc().is_some_and(|m| m.start_offset() == n.location().start_offset())
        })
    }

    /// Facts about the descendants of the parser send (receiver and arguments, not its block).
    #[derive(Default)]
    struct Desc {
        /// forwarded_args, any_block, ambiguous literal or logical operator
        ambiguous: bool,
        braced_hash: bool,
    }

    impl<'pr> Visit<'pr> for Desc {
        fn visit_branch_node_enter(&mut self, n: Node<'pr>) {
            self.check(&n);
        }
        fn visit_leaf_node_enter(&mut self, n: Node<'pr>) {
            self.check(&n);
        }
    }

    impl Desc {
        fn check(&mut self, n: &Node) {
            if n.as_hash_node().is_some() {
                self.braced_hash = true;
            }
            let regexp_slash = match n {
                Node::RegularExpressionNode { .. } => {
                    n.as_regular_expression_node().unwrap().opening_loc().as_slice() == b"/"
                }
                Node::InterpolatedRegularExpressionNode { .. } => {
                    n.as_interpolated_regular_expression_node().unwrap().opening_loc().as_slice() == b"/"
                }
                _ => false,
            };
            // unary_literal?: a signed number, or the operand of a unary operator send.
            let unary = (is_numeric(n) && matches!(node_text(n).as_bytes().first(), Some(b'+' | b'-')))
                || (unary_operation(n) && n.as_call_node().is_some_and(|c| c.receiver().is_some()));
            if matches!(kind::of(n), kind::FORWARDING_ARGUMENTS_NODE | kind::BLOCK_NODE | kind::LAMBDA_NODE)
                || is_splat(n)
                || is_ternary(n)
                || regexp_slash
                || unary
                || logical_operator(n)
            {
                self.ambiguous = true;
            }
        }
    }

    fn descendants(d: &Dispatch) -> Desc {
        let mut desc = Desc::default();
        if let Some(r) = d.call.as_ref().and_then(|c| c.receiver()) {
            desc.visit(&r);
        }
        for a in &d.args {
            desc.visit(a);
        }
        desc
    }

    pub fn check(cx: &mut Cx, node: &Node, d: &Dispatch) {
        let (Some(open), Some(close)) = (d.open, d.close) else { return };
        let anc = cx.ancestors;
        let own_block = d.call.as_ref().and_then(|c| c.block()).and_then(|b| b.as_block_node());
        let ctx = Ctx { anc, node, own_block, call_range: d.range };
        // inside_endless_method_def?
        if !d.args.is_empty() && anc.iter().any(|a| a.as_def_node().is_some_and(|f| f.equal_loc().is_some())) {
            return;
        }
        if require_parentheses_for_hash_value_omission(cx, &ctx, d) {
            return;
        }
        // syntax_like_method_call?
        let implicit_call = d.name == "call" && d.selector.is_none();
        if implicit_call || OPERATOR_METHODS.contains(&d.name) {
            return;
        }
        // method_call_before_constant_resolution?
        if ctx.parent().and_then(|p| ctx.kind_of(p)) == Some(kind::CONSTANT_PATH_NODE) {
            return;
        }
        if legitimate_call_with_parentheses(cx, &ctx, d) {
            return;
        }
        // allowed_camel_case_method_call?
        if d.name.starts_with(|c: char| c.is_ascii_uppercase())
            && (d.args.is_empty() || cx.cfg.bool("AllowParenthesesInCamelCaseMethod").unwrap_or(false))
        {
            return;
        }
        // allowed_string_interpolation_method_call?
        if cx.cfg.bool("AllowParenthesesInStringInterpolation").unwrap_or(false) && inside_string_interpolation(anc) {
            return;
        }
        let src = cx.src;
        let text = src.text;
        // code_after_closing_parenthesis_on_its_own_line? (1.92)
        if let Some(last) = d.args.last()
            && src.line_of(last.location().end_offset()) != src.line_of(close.0)
        {
            let rest = rest_of_line(text, close.1);
            if !rest.is_empty() && !rest.starts_with('#') {
                return;
            }
        }
        // comment_before_first_argument? (1.92)
        if let Some(first) = d.args.first() {
            let lines = src.line_of(open.0)..src.line_of(first.location().start_offset());
            if src.comments.iter().any(|c| lines.contains(&src.line_of(c.start))) {
                return;
            }
        }
        let args_parenthesized = args_parenthesized(d);
        if args_parenthesized {
            return;
        }
        let Some(sel) = d.selector else { return };
        let args_begin = (sel.1, next_char_end(text.as_bytes(), sel.1, 1));
        // parentheses_at_the_end_of_multiline_call? (1.92: nothing after the `(` on its line)
        let multiline = src.line_of(d.range.0) != src.line_of(d.range.1);
        let (args_begin, replacement) = if multiline && rest_of_line(text, open.1).is_empty() {
            let opts = Surrounding { side: Side::Right, newlines: false, ..Default::default() };
            (range_with_surrounding_space(text, args_begin.0, args_begin.1, opts), " \\")
        } else {
            (args_begin, " ")
        };
        cx.state::<State>().0.push(Pending { open, close, args_begin, replacement });
    }

    /// `source_line[(column + 1)..].strip` after the character ending at `pos`.
    fn rest_of_line(text: &str, pos: usize) -> &str {
        let end = text[pos..].find('\n').map_or(text.len(), |i| pos + i);
        text[pos..end].trim_matches(|c: char| c.is_ascii_whitespace() || c == '\0' || c == '\x0b')
    }

    fn require_parentheses_for_hash_value_omission(cx: &Cx, ctx: &Ctx, d: &Dispatch) -> bool {
        let Some(last) = d.args.last() else { return false };
        let elements: Vec<Node> = match last {
            Node::HashNode { .. } => last.as_hash_node().unwrap().elements().iter().collect(),
            Node::KeywordHashNode { .. } => last.as_keyword_hash_node().unwrap().elements().iter().collect(),
            _ => return false,
        };
        let Some(pair) = elements.iter().rev().find(|p| p.as_assoc_node().is_some()) else { return false };
        if !node_text(pair).ends_with(':') {
            return false;
        }
        let Some(parent) = ctx.parent() else { return false };
        if ctx.kind_of(parent).is_some_and(is_conditional) {
            return true;
        }
        if single_line(cx, ctx, parent) {
            return true;
        }
        let r = ctx.range_of(parent);
        // !last_expression?
        if ctx.node_of(parent).is_some_and(is_assignment) {
            ctx.parent_of(parent).is_some_and(|gp| right_sibling(ctx, gp, r))
        } else {
            right_sibling(ctx, parent, d.range)
        }
    }

    /// Whether the child of parent `p` at `child` has a (truthy) right sibling in parser.
    // ponytail: sends, blocks and statement lists only; other parents count as having none.
    fn right_sibling(ctx: &Ctx, p: P, child: (usize, usize)) -> bool {
        let Some(n) = ctx.node_of(p) else {
            // The send of its own block: the block's `args` follow.
            return true;
        };
        if let Some(c) = n.as_call_node() {
            if c.receiver().is_some_and(|r| span(&r) == child) {
                return true;
            }
            let args = send_args(&c);
            return args.iter().position(|a| span(a) == child).is_some_and(|i| i + 1 < args.len());
        }
        let stmts = match n {
            Node::StatementsNode { .. } => n.as_statements_node(),
            Node::ParenthesesNode { .. } => {
                n.as_parentheses_node().unwrap().body().and_then(|b| b.as_statements_node())
            }
            _ => None,
        };
        if let Some(s) = stmts {
            let body: Vec<Node> = s.body().iter().collect();
            return body.iter().position(|a| span(a) == child).is_some_and(|i| i + 1 < body.len());
        }
        // The body of parser's `(rescue body resbody ...)` / `(ensure body ensure_body)`.
        if let Some(b) = n.as_begin_node()
            && b.statements().is_some_and(|s| {
                let r = span(&s.as_node());
                r.0 <= child.0 && child.1 <= r.1
            })
        {
            return b.rescue_clause().is_some()
                || b.ensure_clause().is_some_and(|e| e.statements().is_some());
        }
        false
    }

    /// `parent.single_line?` (a block compares its `{`/`do` and `}`/`end` lines).
    fn single_line(cx: &Cx, ctx: &Ctx, p: P) -> bool {
        let src = cx.src;
        let block = match p {
            P::Call => None,
            P::OwnBlock => ctx.own_block.as_ref().map(|b| (b.opening_loc().start_offset(), b.closing_loc().start_offset())),
            P::Anc(i) => match &ctx.anc[i] {
                Node::BlockNode { .. } => {
                    let b = ctx.anc[i].as_block_node().unwrap();
                    Some((b.opening_loc().start_offset(), b.closing_loc().start_offset()))
                }
                Node::LambdaNode { .. } => {
                    let b = ctx.anc[i].as_lambda_node().unwrap();
                    Some((b.opening_loc().start_offset(), b.closing_loc().start_offset()))
                }
                _ => None,
            },
        };
        let (s, e) = block.unwrap_or_else(|| ctx.range_of(p));
        src.line_of(s) == src.line_of(e)
    }

    fn legitimate_call_with_parentheses(cx: &Cx, ctx: &Ctx, d: &Dispatch) -> bool {
        let parent = ctx.parent();
        let parent_node = parent.and_then(|p| ctx.node_of(p));
        if call_in_literals(ctx, P::Call) {
            return true;
        }
        if parent_node.is_some_and(|p| kind::of(p) == kind::WHEN_NODE) {
            return true;
        }
        if call_with_ambiguous_arguments(ctx, d) {
            return true;
        }
        if call_in_logical_operators(ctx, P::Call) || call_in_optional_arguments(ctx, P::Call) {
            return true;
        }
        // call_in_single_line_inheritance?
        let src = cx.src;
        if let Some(p) = parent_node.filter(|p| kind::of(p) == kind::CLASS_NODE) {
            let (s, e) = span(p);
            if src.line_of(s) == src.line_of(e) {
                return true;
            }
        }
        // allowed_multiline_call_with_parentheses?
        if cx.cfg.bool("AllowParenthesesInMultilineCall").unwrap_or(false)
            && src.line_of(d.range.0) != src.line_of(d.range.1)
        {
            return true;
        }
        if cx.cfg.bool("AllowParenthesesInChaining").unwrap_or(false) && allowed_chained(d) {
            return true;
        }
        // assignment_in_condition?
        if let Some(p) = parent
            && ctx.node_of(p).is_some_and(is_assignment)
            && ctx.parent_of(p).and_then(|gp| ctx.kind_of(gp)).is_some_and(|k| is_conditional(k) || k == kind::WHEN_NODE)
        {
            return true;
        }
        if command_assignment_in_expression(ctx) {
            return true;
        }
        // forwards_anonymous_rest_arguments? (1.92: any argument)
        let forwards = |a: &Node| {
            if a.as_splat_node().is_some_and(|s| s.expression().is_none()) {
                return true;
            }
            let elements: Vec<Node> = match a {
                Node::HashNode { .. } => a.as_hash_node().unwrap().elements().iter().collect(),
                Node::KeywordHashNode { .. } => a.as_keyword_hash_node().unwrap().elements().iter().collect(),
                _ => Vec::new(),
            };
            elements.iter().any(|e| e.as_assoc_splat_node().is_some_and(|s| s.value().is_none()))
        };
        if d.args.iter().any(forwards) {
            return true;
        }
        // trailing_comma? (1.92)
        if let (Some(last), Some(close)) = (d.args.last(), d.close) {
            let from = last.location().end_offset();
            if cx.src.tokens().iter().any(|t| t.kind == sys::PM_TOKEN_COMMA && t.start >= from && t.end <= close.0) {
                return true;
            }
        }
        false
    }

    /// `call_in_literals?(x)`
    fn call_in_literals(ctx: &Ctx, x: P) -> bool {
        ctx.parent_skipping_block(x).and_then(|p| ctx.node_of(p)).is_some_and(|p| {
            matches!(kind::of(p), kind::ASSOC_NODE | kind::ARRAY_NODE | kind::RANGE_NODE) || is_splat(p) || is_ternary(p)
        })
    }

    /// `call_in_logical_operators?(x)`
    fn call_in_logical_operators(ctx: &Ctx, x: P) -> bool {
        let Some(p) = ctx.parent_skipping_block(x) else { return false };
        if command_assignment_before_keyword_operator(ctx, p) {
            return true;
        }
        ctx.node_of(p).is_some_and(|p| {
            logical_operator(p)
                || p.as_call_node().is_some_and(|c| !is_csend(&c) && call_args(&c).iter().any(logical_operator))
        })
    }

    /// `call_in_optional_arguments?(x)`
    fn call_in_optional_arguments(ctx: &Ctx, x: P) -> bool {
        ctx.parent_of(x)
            .and_then(|p| ctx.kind_of(p))
            .is_some_and(|k| matches!(k, kind::OPTIONAL_PARAMETER_NODE | kind::OPTIONAL_KEYWORD_PARAMETER_NODE))
    }

    /// `call_as_argument_or_chain?(x)` (an op-assign's target holds a parser `send`).
    fn call_as_argument_or_chain(ctx: &Ctx, x: P) -> bool {
        let Some(p) = ctx.parent_of(x) else { return false };
        let Some(pn) = ctx.node_of(p) else { return false };
        (is_call_super_yield(pn) || in_write_target(pn, ctx.range_of(x))) && !ctx.assigned_before(p, x)
    }

    /// `command_assignment_before_keyword_operator?(p)` (1.92): an assignment chain that is an
    /// operand of `and` / `or`.
    fn command_assignment_before_keyword_operator(ctx: &Ctx, p: P) -> bool {
        if !ctx.is_assignment(p) {
            return false;
        }
        let mut p = p;
        while let Some(pp) = ctx.parent_of(p).filter(|&pp| ctx.is_assignment(pp)) {
            p = pp;
        }
        ctx.parent_of(p).and_then(|o| ctx.node_of(o)).is_some_and(|o| match o {
            Node::AndNode { .. } => o.as_and_node().unwrap().operator_loc().as_slice() == b"and",
            Node::OrNode { .. } => o.as_or_node().unwrap().operator_loc().as_slice() == b"or",
            _ => false,
        })
    }

    /// `command_assignment_in_expression?` (1.92): the call is the value of an assignment (chain)
    /// that is itself used where a command call needs parentheses.
    fn command_assignment_in_expression(ctx: &Ctx) -> bool {
        let value = match ctx.parent() {
            Some(p) if ctx.is_block(p) || ctx.kind_of(p) == Some(kind::RESCUE_MODIFIER_NODE) => p,
            _ => P::Call,
        };
        let assignment = outermost_assignment(ctx, value);
        if assignment == value {
            return false;
        }
        call_in_literals(ctx, assignment)
            || call_in_logical_operators(ctx, assignment)
            || call_in_optional_arguments(ctx, assignment)
            || call_as_argument_or_chain(ctx, assignment)
            || ctx.parent_of(assignment).and_then(|p| ctx.kind_of(p)).is_some_and(|k| {
                matches!(
                    k,
                    kind::MULTI_WRITE_NODE | kind::RETURN_NODE | kind::NEXT_NODE | kind::BREAK_NODE | kind::DEFINED_NODE
                )
            })
    }

    fn outermost_assignment(ctx: &Ctx, value: P) -> P {
        let Some(p) = ctx.parent_of(value) else { return value };
        if !ctx.is_assignment(p) || ctx.kind_of(p) == Some(kind::MULTI_WRITE_NODE) || !ctx.assigned_before(p, value) {
            return value;
        }
        outermost_assignment(ctx, p)
    }

    /// `allowed_chained_call_with_parentheses?` after the config check: the first descendant (the
    /// receiver, else the first argument) is a parenthesized send, or such a chain.
    fn allowed_chained(d: &Dispatch) -> bool {
        match d.call.as_ref().and_then(|c| c.receiver()) {
            Some(r) => first_send_chain(&r),
            None => d.args.first().is_some_and(first_send_chain),
        }
    }

    fn first_send_chain(n: &Node) -> bool {
        // `previous&.send_type?`: a plain send (a call with a block literal is a parser block).
        let Some(c) =
            n.as_call_node().filter(|c| !is_csend(c) && c.block().is_none_or(|b| b.as_block_node().is_none()))
        else {
            return false;
        };
        if c.closing_loc().is_some_and(|l| l.as_slice() == b")") {
            return true;
        }
        let first = c.receiver().or_else(|| send_args(&c).into_iter().next());
        first.is_some_and(|f| first_send_chain(&f))
    }

    fn call_with_ambiguous_arguments(ctx: &Ctx, d: &Dispatch) -> bool {
        // call_with_braced_block?
        if d.call.is_some() && ctx.own_block.as_ref().is_some_and(|b| b.opening_loc().as_slice() == b"{") {
            return true;
        }
        let parent = ctx.parent();
        // call_in_argument_with_block?
        if let Some(p) = parent
            && ctx.is_block(p)
            && ctx.parent_of(p).and_then(|gp| ctx.node_of(gp)).is_some_and(is_call_super_yield)
        {
            return true;
        }
        let parent_node = parent.and_then(|p| ctx.node_of(p));
        if call_as_argument_or_chain(ctx, P::Call) {
            return true;
        }
        // call_in_match_pattern?
        if parent_node.is_some_and(|p| matches!(kind::of(p), kind::MATCH_PREDICATE_NODE | kind::MATCH_REQUIRED_NODE)) {
            return true;
        }
        let desc = descendants(d);
        // hash_literal_in_arguments?
        let plain_send = |a: &Node| {
            a.as_call_node().is_some_and(|c| !is_csend(&c) && c.block().is_none_or(|b| b.as_block_node().is_none()))
        };
        if d.args.iter().any(|a| a.as_hash_node().is_some() || (plain_send(a) && desc.braced_hash)) {
            return true;
        }
        // ambiguous_range_argument?
        if d.args.first().and_then(|a| a.as_range_node()).is_some_and(|r| r.left().is_none())
            || d.args.last().and_then(|a| a.as_range_node()).is_some_and(|r| r.right().is_none())
        {
            return true;
        }
        desc.ambiguous
    }

    /// `inside_string_interpolation?`: a dstr above the nearest parser `begin` ancestor.
    fn inside_string_interpolation(anc: &[Node]) -> bool {
        let is_begin = |n: &Node| match n {
            Node::EmbeddedStatementsNode { .. } | Node::ParenthesesNode { .. } => true,
            Node::StatementsNode { .. } => n.as_statements_node().unwrap().body().iter().count() > 1,
            _ => false,
        };
        let Some(b) = anc.iter().rposition(is_begin) else { return false };
        anc[..b].iter().any(|a| a.as_interpolated_string_node().is_some())
    }
}

fn require_parentheses(cx: &mut Cx, node: &Node, d: &Dispatch) {
    if allowed_method(cx.cfg, d.name) || matches_allowed_pattern(cx.cfg, d.name) {
        return;
    }
    if OPERATOR_METHODS.contains(&d.name) || d.setter || ignored_macro(cx, node, d) {
        return;
    }
    if d.args.is_empty() || d.parenthesized {
        return;
    }
    let Some(sel) = d.selector else { return };
    let src = cx.src.text.as_bytes();
    // args_begin: `selector.end.resize(1 or 2)`; one parenthesized argument keeps its `)`.
    let args_parenthesized = args_parenthesized(d);
    let begin_end = next_char_end(src, sel.1, if args_parenthesized { 2 } else { 1 });
    let (start, end) = d.range;
    cx.add_fix(start, end, "Use parentheses for method calls with arguments.", |c| {
        c.replace(sel.1, begin_end, "(");
        if !args_parenthesized {
            c.insert_after(end, end, ")");
        }
    });
}

/// `args_parenthesized?`: the one argument is a parenthesized expression whose parentheses can
/// double as the call's (1.92: not `(a and b)`, `(a if b)`, `(not a)`, `(a rescue b)`, `(a; b)`,
/// a multiple assignment or a one-line pattern match).
fn args_parenthesized(d: &Dispatch) -> bool {
    let [arg] = d.args.as_slice() else { return false };
    let Some(p) = arg.as_parentheses_node().filter(|p| p.opening_loc().as_slice() == b"(") else { return false };
    let Some(body) = p.body() else { return false };
    let only = match body.as_statements_node() {
        Some(st) => {
            let mut it = st.body().iter();
            match (it.next(), it.next()) {
                (Some(n), None) => n,
                _ => return false,
            }
        }
        None => body,
    };
    mergeable_parentheses(&only)
}

fn mergeable_parentheses(node: &Node) -> bool {
    match node {
        Node::AndNode { .. } => node.as_and_node().unwrap().operator_loc().as_slice() == b"&&",
        Node::OrNode { .. } => node.as_or_node().unwrap().operator_loc().as_slice() == b"||",
        // modifier_form?: no `end` (a ternary is not a modifier).
        Node::IfNode { .. } => {
            let i = node.as_if_node().unwrap();
            i.if_keyword_loc().is_none() || i.end_keyword_loc().is_some()
        }
        Node::UnlessNode { .. } => node.as_unless_node().unwrap().end_keyword_loc().is_some(),
        Node::WhileNode { .. } => node.as_while_node().unwrap().closing_loc().is_some(),
        Node::UntilNode { .. } => node.as_until_node().unwrap().closing_loc().is_some(),
        // `keyword?` on a send: `not x`.
        Node::CallNode { .. } => !node.as_call_node().unwrap().message_loc().is_some_and(|m| m.as_slice() == b"not"),
        Node::RescueModifierNode { .. }
        | Node::MultiWriteNode { .. }
        | Node::MatchRequiredNode { .. }
        | Node::MatchPredicateNode { .. } => false,
        _ => true,
    }
}

/// Byte offset `n` characters after `pos`.
fn next_char_end(src: &[u8], pos: usize, n: usize) -> usize {
    let mut p = pos;
    for _ in 0..n {
        if p >= src.len() {
            break;
        }
        p += 1;
        while p < src.len() && (src[p] & 0xC0) == 0x80 {
            p += 1;
        }
    }
    p
}

fn ignored_macro(cx: &Cx, node: &Node, d: &Dispatch) -> bool {
    if !cx.cfg.bool("IgnoreMacros").unwrap_or(false) || d.receiver {
        return false;
    }
    let loc = node.location();
    if !in_macro_scope(cx.ancestors, (loc.start_offset(), loc.end_offset())) {
        return false;
    }
    // `cop_config.fetch(key, []).map`: a String raises (a cop error).
    if cx.cfg.array("IncludedMacros").iter().any(|m| m == d.name) {
        return false;
    }
    !cx.cfg.regexes_array("IncludedMacroPatterns").iter().any(|r| r.is_match(d.name))
}
