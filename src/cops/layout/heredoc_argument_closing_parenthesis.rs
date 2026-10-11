use crate::prelude::*;

pub const NAME: &str = "Layout/HeredocArgumentClosingParenthesis";
pub static COP: HeredocArgumentClosingParenthesis = HeredocArgumentClosingParenthesis;
pub struct HeredocArgumentClosingParenthesis;

const MSG: &str =
    "Put the closing parenthesis for a method call with a HEREDOC parameter on the same line as the HEREDOC opening.";

fn range(n: &Node) -> (usize, usize) {
    let l = n.location();
    (l.start_offset(), l.end_offset())
}

/// `extract_heredoc(node)` is truthy.
fn has_heredoc(node: &Node) -> bool {
    if is_heredoc(node) {
        return true;
    }
    // single_line_send_with_heredoc_receiver?
    if let Some(call) = node.as_call_node().filter(|c| !is_csend(c))
        && let Some(recv) = call.receiver()
        && let Some((_, end, _)) = helpers::heredoc::parts(&recv)
        && end.1 > range(node).1
    {
        return true;
    }
    let elements = match node {
        Node::HashNode { .. } => node.as_hash_node().unwrap().elements(),
        Node::KeywordHashNode { .. } => node.as_keyword_hash_node().unwrap().elements(),
        _ => return false,
    };
    elements.iter().any(|e| e.as_assoc_node().is_some_and(|p| has_heredoc(&p.value())))
}

/// The `loc.end` start offset of a node, where parser gives it one.
fn loc_end(n: &Node) -> Option<usize> {
    let block_close = |b: Option<Node>| b.and_then(|b| b.as_block_node()).map(|b| b.closing_loc().start_offset());
    let off = |l: Option<ruby_prism::Location>| l.map(|l| l.start_offset());
    if is_heredoc(n) {
        return None;
    }
    match n {
        Node::CallNode { .. } => {
            let c = n.as_call_node().unwrap();
            block_close(c.block()).or_else(|| off(c.closing_loc()))
        }
        Node::SuperNode { .. } => {
            let s = n.as_super_node().unwrap();
            block_close(s.block()).or_else(|| off(s.rparen_loc()))
        }
        Node::ForwardingSuperNode { .. } => block_close(n.as_forwarding_super_node().unwrap().block().map(|b| b.as_node())),
        Node::LambdaNode { .. } => Some(n.as_lambda_node().unwrap().closing_loc().start_offset()),
        Node::ArrayNode { .. } => off(n.as_array_node().unwrap().closing_loc()),
        Node::HashNode { .. } => Some(n.as_hash_node().unwrap().closing_loc().start_offset()),
        Node::StringNode { .. } => off(n.as_string_node().unwrap().closing_loc()),
        Node::InterpolatedStringNode { .. } => off(n.as_interpolated_string_node().unwrap().closing_loc()),
        Node::SymbolNode { .. } => off(n.as_symbol_node().unwrap().closing_loc()),
        Node::InterpolatedSymbolNode { .. } => off(n.as_interpolated_symbol_node().unwrap().closing_loc()),
        Node::XStringNode { .. } => Some(n.as_x_string_node().unwrap().closing_loc().start_offset()),
        Node::RegularExpressionNode { .. } => Some(n.as_regular_expression_node().unwrap().closing_loc().start_offset()),
        Node::InterpolatedRegularExpressionNode { .. } => {
            Some(n.as_interpolated_regular_expression_node().unwrap().closing_loc().start_offset())
        }
        Node::ParenthesesNode { .. } => Some(n.as_parentheses_node().unwrap().closing_loc().start_offset()),
        _ => end_keyword(n),
    }
}

/// `loc.end` when it is an `end` keyword.
fn end_keyword(n: &Node) -> Option<usize> {
    let off = |l: Option<ruby_prism::Location>| l.map(|l| l.start_offset());
    let kw = |l: ruby_prism::Location| (l.as_slice() == b"end").then_some(l.start_offset());
    match n {
        Node::DefNode { .. } => off(n.as_def_node().unwrap().end_keyword_loc()),
        Node::ClassNode { .. } => off(Some(n.as_class_node().unwrap().end_keyword_loc())),
        Node::ModuleNode { .. } => off(Some(n.as_module_node().unwrap().end_keyword_loc())),
        Node::SingletonClassNode { .. } => off(Some(n.as_singleton_class_node().unwrap().end_keyword_loc())),
        Node::BlockNode { .. } => kw(n.as_block_node().unwrap().closing_loc()),
        Node::LambdaNode { .. } => kw(n.as_lambda_node().unwrap().closing_loc()),
        Node::IfNode { .. } => off(n.as_if_node().unwrap().end_keyword_loc()),
        Node::UnlessNode { .. } => off(n.as_unless_node().unwrap().end_keyword_loc()),
        Node::WhileNode { .. } => n.as_while_node().unwrap().closing_loc().and_then(kw),
        Node::UntilNode { .. } => n.as_until_node().unwrap().closing_loc().and_then(kw),
        Node::ForNode { .. } => off(Some(n.as_for_node().unwrap().end_keyword_loc())),
        Node::CaseNode { .. } => off(Some(n.as_case_node().unwrap().end_keyword_loc())),
        Node::CaseMatchNode { .. } => off(Some(n.as_case_match_node().unwrap().end_keyword_loc())),
        Node::BeginNode { .. } => {
            let b = n.as_begin_node().unwrap();
            b.begin_keyword_loc().and(off(b.end_keyword_loc()))
        }
        _ => None,
    }
}

/// What the corrector needs of the outermost send.
#[derive(Clone, Copy)]
struct Outer {
    range: (usize, usize),
    close: (usize, usize),
    last_arg: (usize, usize),
}

impl Cop for HeredocArgumentClosingParenthesis {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let args = send_args(&call);
        let Some(heredoc_arg) = args.iter().find(|a| has_heredoc(a)) else { return };
        let src = cx.src;
        let heredoc_last_line = src.line_of(range(heredoc_arg).1);

        // outermost_send_on_same_line: climb parser parents from the argument.
        let missing_parens = |c: &ruby_prism::CallNode, child: (usize, usize)| {
            c.opening_loc().is_some()
                && send_args(c).iter().any(|a| range(a) == child)
                && c.closing_loc().is_some_and(|e| src.line_of(e.start_offset()) != heredoc_last_line)
        };
        let mut outer: Option<Node> = None;
        if missing_parens(&call, range(heredoc_arg)) {
            outer = Some(call.as_node());
        } else {
            let mut prev = range(node);
            let anc = cx.ancestors;
            for i in (0..anc.len()).rev() {
                let a = &anc[i];
                if frame(a) == Frame::Transparent || a.as_statements_node().is_some() {
                    continue;
                }
                if let Some(c) = a.as_call_node() {
                    // A call owning the block we come from is inside parser's block node.
                    if anc.get(i + 1).is_some_and(|b| b.as_block_node().is_some()) {
                        prev = (a.location().start_offset(), anc[i + 1].location().end_offset());
                        continue;
                    }
                    if missing_parens(&c, prev) {
                        outer = Some(c.as_node());
                        break;
                    }
                }
                if a.as_block_node().is_some() {
                    continue;
                }
                prev = range(a);
            }
        }
        let Some(outer) = outer else { return };
        let oc = outer.as_call_node().unwrap();

        if cx.ancestors.iter().any(|a| end_keyword(a).is_some()) {
            return;
        }
        let close = oc.closing_loc().unwrap();
        // subsequent_closing_parentheses_in_same_line?
        let outer_args = send_args(&oc);
        if let Some(e) = outer_args.last().and_then(loc_end)
            && src.line_of(e) == src.line_of(close.start_offset())
            && src.column(close.start_offset()) == src.column(e) + 1
        {
            return;
        }
        // exist_argument_between_heredoc_end_and_closing_parentheses?
        let Some(node_close) = call.closing_loc() else { return };
        if let Some(heredoc_end) = args.iter().filter_map(|a| helpers::heredoc::parts(a).map(|p| p.1.1)).max()
            && heredoc_end < node_close.start_offset()
            && !helpers::ruby_string::strip(src.slice(heredoc_end, node_close.start_offset())).is_empty()
        {
            return;
        }

        let o = Outer {
            range: range(&outer),
            close: (close.start_offset(), close.end_offset()),
            last_arg: range(outer_args.last().unwrap()),
        };
        let text = src.text;
        let line_of_close = src.lines.get(src.line_of(o.close.0) - 1).copied().unwrap_or("");
        cx.add_fix(o.close.0, o.close.1, MSG, |c| autocorrect(c, text, o, line_of_close));
    }
}

static SAFE_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^ *\) {0,20},? *$").unwrap());

fn autocorrect(c: &mut Corrector, text: &str, o: Outer, line_of_close: &str) {
    let bytes = text.as_bytes();
    let end_pos = o.range.1;
    // remove_incorrect_closing_paren
    let begin = if SAFE_LINE.is_match(line_of_close) {
        let source = &text[o.range.0..end_pos];
        match source.rfind('\n') {
            Some(i) => end_pos - (source.len() - i - 1) - 1,
            None => return,
        }
    } else {
        end_pos - 1
    };
    let end = if bytes.get(end_pos) == Some(&b',') { end_pos + 1 } else { end_pos };
    c.remove(begin, end);
    c.insert_after(o.last_arg.0, o.last_arg.1, ")");
    // internal trailing comma
    let after = &text[o.last_arg.1..o.close.0];
    if let (Some(comma), Some(nl)) = (after.find(','), after.find('\n'))
        && comma <= nl
    {
        c.remove(o.last_arg.1, o.last_arg.1 + comma + 1);
    }
    // external trailing comma
    let spaces = bytes[end_pos..].iter().take(20).take_while(|&&b| b == b' ').count();
    if bytes.get(end_pos + spaces) == Some(&b',') {
        c.remove(end_pos, end_pos + spaces + 1);
        c.insert_after(o.last_arg.0, o.last_arg.1, ",");
    }
}
