use crate::prelude::*;
use crate::source::Source;
use helpers::alignment::{
    begins_its_line, blank, cop_enabled, display_column, enabled_cop_str, first_non_space,
    configured_indentation_width,
};

pub const NAME: &str = "Layout/FirstArgumentIndentation";
pub static COP: FirstArgumentIndentation = FirstArgumentIndentation;
pub struct FirstArgumentIndentation;

const OPERATOR_METHODS: &[&str] = &[
    "|", "^", "&", "<=>", "==", "===", "=~", ">", ">=", "<", "<=", "<<", ">>", "+", "-", "*", "/", "%", "**", "~",
    "+@", "-@", "!@", "~@", "[]", "[]=", "!", "!=", "!~", "`",
];

static COMMENT_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^[ \t\r\n\f\v]*#").unwrap());

#[derive(PartialEq, Clone, Copy)]
enum Style {
    Consistent,
    RelativeToReceiver,
    SpecialForInner,
    SpecialInParens,
}

/// The parser-gem parent of a send/super (Prism ancestors, nearest last): None when parser wraps
/// the call in a block, or there is no parent; otherwise the nearest non-wrapper ancestor.
fn parser_parent<'a, 'pr>(node: &Node<'pr>, ancestors: &'a [Node<'pr>]) -> Option<&'a Node<'pr>> {
    let own_block = match kind::of(node) {
        kind::CALL_NODE => node.as_call_node().unwrap().block(),
        kind::SUPER_NODE => node.as_super_node().unwrap().block(),
        _ => None,
    };
    if own_block.is_some_and(|b| kind::of(&b) == kind::BLOCK_NODE) {
        return None;
    }
    helpers::parser_parent::parser_parent(ancestors)
}

fn span(n: &Node) -> (usize, usize) {
    (n.location().start_offset(), n.location().end_offset())
}

struct Ctx<'a, 'b, 'pr> {
    src: &'a Source<'a>,
    style: Style,
    comment_lines: Vec<usize>,
    node: &'b Node<'pr>,
    parent: Option<&'b Node<'pr>>,
    first: &'b Node<'pr>,
}

impl Ctx<'_, '_, '_> {
    fn special_inner_call_indentation(&self) -> bool {
        match self.style {
            Style::Consistent => return false,
            Style::RelativeToReceiver => return true,
            _ => {}
        }
        // eligible_method_call?: (send _ !:[]= ...)
        let Some(parent) = self.parent.and_then(|p| p.as_call_node()) else { return false };
        if is_csend(&parent) || method_name(&parent) == "[]=" {
            return false;
        }
        let parenthesized = parent.opening_loc().is_some_and(|l| l.as_slice() == b"(");
        if !parenthesized && self.style == Style::SpecialInParens {
            return false;
        }
        span(self.node).0 > parent.location().start_offset()
    }

    /// base_range(send, first_arg) start: the send, or a splat/kwsplat around it.
    fn base_start(&self) -> usize {
        match self.parent {
            Some(p) if matches!(kind::of(p), kind::SPLAT_NODE | kind::ASSOC_SPLAT_NODE) => span(p).0,
            _ => span(self.node).0,
        }
    }

    fn previous_code_line(&self, mut line_number: usize) -> &str {
        let mut line = "";
        while blank(line) || self.comment_lines.contains(&line_number) {
            if line_number <= 1 {
                return "";
            }
            line_number -= 1;
            line = self.src.lines.get(line_number - 1).copied().unwrap_or("");
        }
        line
    }

    fn indent_of(line: &str) -> i64 {
        line[..first_non_space(line).unwrap_or(0)].chars().count() as i64
    }

    fn base_indentation(&self) -> i64 {
        let first_line = self.src.line_of(span(self.first).0);
        if self.special_inner_call_indentation() {
            let (start, end) = (self.base_start(), span(self.first).0);
            let source = self.src.slice(start, end).trim();
            if source.contains('\n') {
                let line = self.src.line_of(start) + source.matches('\n').count() + 1;
                Self::indent_of(self.previous_code_line(line))
            } else {
                display_column(self.src, start) as i64
            }
        } else {
            Self::indent_of(self.previous_code_line(first_line))
        }
    }
}

impl Cop for FirstArgumentIndentation {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE, kind::SUPER_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let args = if let Some(call) = node.as_call_node() {
            let name = method_name(&call);
            let dot = call.call_operator_loc().is_some_and(|l| l.as_slice() == b".");
            if (OPERATOR_METHODS.contains(&name) && !dot) || call.is_attribute_write() {
                return;
            }
            send_args(&call)
        } else {
            let s = node.as_super_node().unwrap();
            let mut v: Vec<Node> = s.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default();
            v.extend(s.block().filter(|b| kind::of(b) == kind::BLOCK_ARGUMENT_NODE));
            v
        };
        let Some(first) = args.first() else { return };
        let src = cx.src;
        if src.line_of(span(node).0) == src.line_of(span(first).0) {
            return;
        }
        let style = match cx.cfg.str("EnforcedStyle") {
            Some("consistent") => Style::Consistent,
            Some("consistent_relative_to_receiver") => Style::RelativeToReceiver,
            Some("special_for_inner_method_call") => Style::SpecialForInner,
            _ => Style::SpecialInParens,
        };
        let comment_lines =
            src.comments.iter().filter(|c| begins_its_line(src, c.start)).map(|c| src.line_of(c.start)).collect();
        let c = Ctx { src, style, comment_lines, node, parent: parser_parent(node, cx.ancestors), first };
        let width = configured_indentation_width(cx);

        if enabled_cop_str(cx, "Layout/ArgumentAlignment", "EnforcedStyle").as_deref() == Some("with_fixed_indentation")
            && (!cop_enabled(cx, "Layout/FirstMethodArgumentLineBreak") || conflicts_with_fixed(&c, node, &args))
        {
            return;
        }

        let indent = c.base_indentation() + width;
        let (start, end) = span(first);
        if !begins_its_line(src, start) {
            return;
        }
        let delta = indent - display_column(src, start) as i64;
        if delta == 0 {
            return;
        }
        if cx.out.iter().any(|o| o.cop == NAME && start >= o.start && end <= o.end) {
            cx.add_with(start, end, "Bad indentation of the first argument.", false);
            return;
        }
        let text = src.slice(c.base_start(), start).trim();
        let base = if !text.contains('\n') && c.special_inner_call_indentation() {
            format!("`{text}`")
        } else if text.lines().last().is_some_and(|l| COMMENT_LINE.is_match(l)) {
            "the start of the previous line (not counting the comment)".to_string()
        } else {
            "the start of the previous line".to_string()
        };
        let (cs, ce, target) = correct_target(&c, cx.ancestors, delta);
        let corr_node = match target {
            None => first,
            Some(usize::MAX) => node,
            Some(i) => &cx.ancestors[i],
        };
        let set = helpers::alignment_corrector::Settings::of(cx);
        cx.add_fix(start, end, format!("Indent the first argument one step more than {base}."), |c| {
            helpers::alignment_corrector::correct(c, src, set, cs, ce, Some(corr_node), delta, false)
        });
    }
}

/// Range AlignmentCorrector would shift (and the ancestor index of its node, None for the first
/// argument): the first argument, or for special_for_inner_method_call_in_parentheses sometimes
/// the whole receiver chain.
fn correct_target(c: &Ctx, ancestors: &[Node], delta: i64) -> (usize, usize, Option<usize>) {
    let first = span(c.first);
    if c.style != Style::SpecialInParens {
        return (first.0, first.1, None);
    }
    // find_top_level_send: climb while the parent call has us as its `.` receiver.
    let mut top_span = span(c.node);
    let mut top: Option<usize> = None;
    let mut i = ancestors.len();
    while i > 0 {
        let a = &ancestors[i - 1];
        let Some(p) = a.as_call_node() else { break };
        let is_recv = p.receiver().is_some_and(|r| span(&r) == top_span);
        if !is_recv || is_csend(&p) || p.call_operator_loc().is_none() {
            break;
        }
        top_span = span(a);
        top = Some(i - 1);
        i -= 1;
        // A call with a block is parser's `send` inside a `block`: the climb stops there.
        if own_block(a) {
            break;
        }
    }
    let top_node = top.map_or(c.node, |t| &ancestors[t]);
    // inner_call?: top's parent is a parenthesized send
    let top_parent =
        if own_block(top_node) { None } else { ancestors[..i].iter().rev().find(|a| frame(a) != Frame::Transparent) };
    let inner = top_parent
        .and_then(|p| p.as_call_node())
        .is_some_and(|p| !is_csend(&p) && p.opening_loc().is_some_and(|l| l.as_slice() == b"("));
    if !inner || display_column(c.src, span(c.node).0) as i64 >= delta.abs() {
        return (first.0, first.1, None);
    }
    let top_end_begins = top_node
        .as_call_node()
        .and_then(|t| t.closing_loc())
        .is_some_and(|l| begins_its_line(c.src, l.start_offset()));
    if top.is_some() || top_end_begins {
        let (s, e) = match top_node.as_call_node() {
            Some(t) => send_range(&t),
            None => span(top_node),
        };
        return (s, e, Some(top.unwrap_or(usize::MAX)));
    }
    (first.0, first.1, None)
}

fn own_block(n: &Node) -> bool {
    n.as_call_node().and_then(|c| c.block()).is_some_and(|b| kind::of(&b) == kind::BLOCK_NODE)
}

fn conflicts_with_fixed(c: &Ctx, node: &Node, args: &[Node]) -> bool {
    if !c.special_inner_call_indentation() {
        return false;
    }
    let Some(call) = node.as_call_node() else { return false };
    if method_name(&call) == "[]=" {
        return false;
    }
    let pairs = |n: &Node| {
        n.as_hash_node()
            .map(|h| h.elements().iter().filter(|e| kind::of(e) == kind::ASSOC_NODE).count())
            .or_else(|| {
                n.as_keyword_hash_node().map(|h| h.elements().iter().filter(|e| kind::of(e) == kind::ASSOC_NODE).count())
            })
    };
    if !(args.len() >= 2 || pairs(&args[0]).is_some_and(|n| n >= 2)) {
        return false;
    }
    let line_off = call.message_loc().or(call.opening_loc()).map_or(span(node).0, |l| l.start_offset());
    let line = c.src.lines.get(c.src.line_of(line_off) - 1).copied().unwrap_or("");
    c.base_indentation() != line[..first_non_space(line).unwrap_or(0)].chars().count() as i64
}
