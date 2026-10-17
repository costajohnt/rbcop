use crate::helpers::range_help::{range_with_surrounding_space, Side, Surrounding};
use crate::prelude::*;

pub const NAME: &str = "Style/BlockDelimiters";
pub static COP: BlockDelimiters = BlockDelimiters;
pub struct BlockDelimiters;

/// Ranges passed to `ignore_node`.
#[derive(Default)]
struct Ignored(Vec<(usize, usize)>);

/// A parser block node seen from its owner (call, super, lambda).
struct Block<'pr> {
    /// Parser block range (the whole Prism owner).
    range: (usize, usize),
    opening: (usize, usize),
    closing: (usize, usize),
    body: Option<Node<'pr>>,
    /// Plain block (not numblock / itblock).
    plain: bool,
    method: String,
    /// The owner's send has arguments and no parentheses.
    unparenthesized_args: bool,
}

fn block_of<'pr>(node: &Node<'pr>) -> Option<Block<'pr>> {
    let range = (node.location().start_offset(), node.location().end_offset());
    if let Some(l) = node.as_lambda_node() {
        return Some(Block {
            range,
            opening: (l.opening_loc().start_offset(), l.opening_loc().end_offset()),
            closing: (l.closing_loc().start_offset(), l.closing_loc().end_offset()),
            plain: l.parameters().is_none_or(|p| p.as_block_parameters_node().is_some()),
            body: l.body(),
            method: "lambda".into(),
            unparenthesized_args: false,
        });
    }
    let (block, method, unparenthesized_args) = if let Some(c) = node.as_call_node() {
        let paren = c.opening_loc().is_some_and(|o| o.as_slice() == b"(");
        (c.block()?.as_block_node()?, method_name(&c).to_string(), !send_args(&c).is_empty() && !paren)
    } else if let Some(s) = node.as_super_node() {
        let has_args = s.arguments().is_some();
        (s.block()?.as_block_node()?, "super".into(), has_args && s.lparen_loc().is_none())
    } else {
        (node.as_forwarding_super_node()?.block()?, "super".into(), false)
    };
    Some(Block {
        range,
        opening: (block.opening_loc().start_offset(), block.opening_loc().end_offset()),
        closing: (block.closing_loc().start_offset(), block.closing_loc().end_offset()),
        plain: block.parameters().is_none_or(|p| p.as_block_parameters_node().is_some()),
        body: block.body(),
        method,
        unparenthesized_args,
    })
}

impl Cop for BlockDelimiters {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE, kind::SUPER_NODE, kind::FORWARDING_SUPER_NODE, kind::LAMBDA_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // Parser visits the block before its send.
        if let Some(block) = block_of(node) {
            on_block(cx, &block);
        }
        if let Some(call) = node.as_call_node() {
            on_send(cx, &call);
        }
    }
}

fn on_send(cx: &mut Cx, call: &ruby_prism::CallNode) {
    let args = send_args(call);
    if args.is_empty() || call.opening_loc().is_some_and(|o| o.as_slice() == b"(") {
        return;
    }
    let name = method_name(call);
    if name.ends_with('=') && !matches!(name, "==" | "!=" | "===" | "<=" | ">=") {
        return;
    }
    // single_argument_operator_method?
    if helpers::node_types::is_operator_method(name)
        && args.len() == 1
        && block_of(&args[0]).is_some_and(|b| b.plain)
    {
        return;
    }
    let mut found = Vec::new();
    for arg in &args {
        get_blocks(arg, &mut found);
    }
    cx.state::<Ignored>().0.extend(found);
}

fn get_blocks(node: &Node, out: &mut Vec<(usize, usize)>) {
    if let Some(b) = block_of(node) {
        out.push(b.range);
    } else if let Some(c) = node.as_call_node() {
        if let Some(r) = c.receiver() {
            get_blocks(&r, out);
        }
        for a in send_args(&c) {
            get_blocks(&a, out);
        }
    } else if let Some(h) = node.as_keyword_hash_node() {
        for e in h.elements().iter() {
            get_blocks(&e, out);
        }
    } else if let Some(p) = node.as_assoc_node() {
        get_blocks(&p.key(), out);
        get_blocks(&p.value(), out);
    }
}

fn on_block(cx: &mut Cx, b: &Block) {
    let ignored = &cx.state::<Ignored>().0;
    if ignored.iter().any(|&(s, e)| s <= b.range.0 && e >= b.range.1) {
        return;
    }
    let src = cx.src;
    let braces = src.slice(b.closing.0, b.closing.1) == "}";
    let multiline = src.line_of(b.opening.0) != src.line_of(b.closing.0);
    let style = cx.cfg.str("EnforcedStyle").unwrap_or("line_count_based");
    // `cop_config.fetch('BracesRequiredMethods', []).include?`: a substring test on a String.
    let braces_required = cx.cfg.includes("BracesRequiredMethods", &b.method);

    // proper_block_style?
    if require_do_end(b, braces, multiline) {
        return;
    }
    if helpers::allowed::allowed_method(cx.cfg, &b.method) || helpers::allowed::matches_allowed_pattern(cx.cfg, &b.method) {
        return;
    }
    let ancestors = cx.ancestors;
    let chained = parent_idx(ancestors).is_some_and(|i| receiver_is(&ancestors[i], b.range));
    let proper = if braces_required {
        braces
    } else {
        match style {
            "semantic" => {
                if braces {
                    // `cop_config['FunctionalMethods'].to_set`: a String raises (a cop error).
                    cx.cfg.array_bare("FunctionalMethods").contains(&b.method)
                        || return_value_used(ancestors)
                        || return_value_of_scope(src, ancestors, b.range)
                        || (cx.cfg.bool("AllowBracesOnProceduralOneLiners") == Some(true) && !multiline)
                } else {
                    cx.cfg.array_bare("ProceduralMethods").contains(&b.method) || !return_value_used(ancestors)
                }
            }
            "braces_for_chaining" => {
                if multiline {
                    if chained { braces } else { !braces }
                } else {
                    braces
                }
            }
            "always_braces" => braces,
            _ => multiline ^ braces,
        }
    };
    if proper {
        return;
    }

    let msg = if braces_required {
        format!("Brace delimiters `{{...}}` required for '{}' method.", b.method)
    } else {
        match style {
            "semantic" if braces => "Prefer `do...end` over `{...}` for procedural blocks.",
            "semantic" => "Prefer `{...}` over `do...end` for functional blocks.",
            "braces_for_chaining" if multiline && chained => "Prefer `{...}` over `do...end` for multi-line chained blocks.",
            "braces_for_chaining" if multiline => "Prefer `do...end` for multi-line blocks without chaining.",
            "braces_for_chaining" => "Prefer `{...}` over `do...end` for single-line blocks.",
            "always_braces" => "Prefer `{...}` over `do...end` for blocks.",
            _ if multiline => "Avoid using `{...}` for multi-line blocks.",
            _ => "Prefer `{...}` over `do...end` for single-line blocks.",
        }
        .to_string()
    };

    // correction_would_break_code?
    let breaks = !braces && b.unparenthesized_args;
    let (open, close) = (b.opening, b.closing);
    let text = src.text;
    let ws = |i: usize| text.as_bytes().get(i).is_some_and(|&c| helpers::parser_tokens::is_ruby_space(c));
    let ws_before = |i: usize| if i == 0 { ws(text.len().wrapping_sub(1)) } else { ws(i - 1) };
    // Comment after the closing brace, moved before the block.
    let comment_move = (braces && !breaks)
        .then(|| src.comment_on_line(src.line_of(close.0)).map(|c| (c.start, c.end)))
        .flatten()
        .map(|(cs, ce)| {
            let anchor = if chained { end_of_chain(ancestors, b.range) } else { close };
            // parser's Range#join takes the min begin and max end, so a chain ending after the
            // comment (`}.to( # c\n  x\n)`) gives the range from the comment to the chain end.
            let join = |a: usize, b: usize| (a.min(b), a.max(b));
            let is_ws = helpers::ruby_string::is_space;
            // source_range_before_comment
            let (pre_start, joined_end) = join(anchor.1, cs);
            let pre_end = pre_start + text[pre_start..joined_end].trim_end_matches(is_ws).len();
            let removal = range_with_surrounding_space(text, cs, ce, Surrounding { side: Side::Right, ..Default::default() });
            // remove_trailing_whitespace
            let trailing = join(pre_end, cs);
            let gap = &text[trailing.0..trailing.1];
            let trailing_ws = (!gap.is_empty() && gap.chars().all(is_ws)).then_some(trailing);
            (removal, (pre_start, pre_end), trailing_ws, format!("{}\n", &text[cs..ce]))
        });
    let begin_required = !braces
        && multiline
        && b.body.as_ref().and_then(|n| n.as_begin_node()).is_some_and(|n| {
            n.begin_keyword_loc().is_none() && (n.rescue_clause().is_some() || n.ensure_clause().is_some())
        });
    let body_range = b.body.as_ref().map(body_range);
    let block_range = b.range;

    let mut ran = false;
    cx.add_fix(open.0, open.1, msg, |c| {
        ran = true;
        if breaks {
            return;
        }
        if braces {
            if !ws_before(open.0) {
                c.insert_before(open.0, open.1, " ");
            }
            if !ws_before(close.0) {
                c.insert_before(close.0, close.1, " ");
            }
            if !ws(open.0 + 1) {
                c.insert_after(open.0, open.1, " ");
            }
            c.replace(open.0, open.1, "do");
            if let Some((removal, pre, trailing_ws, comment)) = &comment_move {
                c.remove(removal.0, removal.1);
                if let Some((s, e)) = trailing_ws {
                    c.remove(*s, *e);
                }
                c.insert_after(pre.0, pre.1, "\n");
                c.insert_before(block_range.0, block_range.1, comment);
            }
            c.replace(close.0, close.1, "end");
        } else {
            if !ws(open.0 + 2) {
                c.insert_after(open.0, open.1, " ");
            }
            c.replace(open.0, open.1, "{");
            c.replace(close.0, close.1, "}");
            if begin_required && let Some((s, e)) = body_range {
                c.wrap(s, e, "begin\n", "\nend");
            }
        }
    });
    if ran {
        cx.state::<Ignored>().0.push(b.range);
    }
}

/// require_do_end?: a single-line do-end block whose body needs `ensure`/`rescue`.
fn require_do_end(b: &Block, braces: bool, multiline: bool) -> bool {
    if braces || multiline {
        return false;
    }
    let Some(body) = b.body.as_ref().and_then(|n| n.as_begin_node()).filter(|n| n.begin_keyword_loc().is_none())
    else {
        return false;
    };
    if body.ensure_clause().is_some() {
        return true;
    }
    let Some(rescue) = body.rescue_clause() else { return false };
    // !modifier_rescue?
    let modifier = body.statements().is_some()
        && body.else_clause().is_none()
        && rescue.subsequent().is_none()
        && rescue.exceptions().is_empty()
        && rescue.reference().is_none();
    !modifier
}

/// Index of the parser parent in `ancestors`.
fn parent_idx(ancestors: &[Node]) -> Option<usize> {
    let p = helpers::parser_parent::parser_parent(ancestors)?;
    ancestors.iter().position(|a| std::ptr::eq(a, p))
}

/// `node` is a send/csend whose receiver spans `range`.
fn receiver_is(node: &Node, range: (usize, usize)) -> bool {
    node.as_call_node().and_then(|c| c.receiver()).is_some_and(|r| {
        let l = r.location();
        (l.start_offset(), l.end_offset()) == range
    })
}

/// Range of `end_of_chain(block.parent)`: the outermost call of the method chain.
fn end_of_chain(ancestors: &[Node], mut range: (usize, usize)) -> (usize, usize) {
    let mut upto = ancestors.len();
    while let Some(i) = parent_idx(&ancestors[..upto]).filter(|&i| receiver_is(&ancestors[i], range)) {
        let l = ancestors[i].location();
        range = (l.start_offset(), l.end_offset());
        upto = i;
    }
    range
}

/// Parser `begin` (parentheses, a statement list outside `begin..end`, interpolation).
fn is_begin(ancestors: &[Node], i: usize) -> bool {
    match kind::of(&ancestors[i]) {
        kind::PARENTHESES_NODE | kind::EMBEDDED_STATEMENTS_NODE => true,
        kind::STATEMENTS_NODE => !(i > 0 && frame(&ancestors[i - 1]) == Frame::KwBegin),
        _ => false,
    }
}

/// return_value_used?
fn return_value_used(ancestors: &[Node]) -> bool {
    let Some(i) = parent_idx(ancestors) else { return false };
    if is_begin(ancestors, i) {
        return return_value_used(&ancestors[..i]);
    }
    let p = &ancestors[i];
    is_assignment(p) || p.as_call_node().is_some()
}

/// return_value_of_scope?
fn return_value_of_scope(src: &crate::source::Source, ancestors: &[Node], range: (usize, usize)) -> bool {
    let Some(i) = parent_idx(ancestors) else { return false };
    let p = &ancestors[i];
    match kind::of(p) {
        kind::IF_NODE
        | kind::UNLESS_NODE
        | kind::WHILE_NODE
        | kind::UNTIL_NODE
        | kind::CASE_NODE
        | kind::CASE_MATCH_NODE
        | kind::AND_NODE
        | kind::OR_NODE
        | kind::ARRAY_NODE
        | kind::RANGE_NODE => true,
        kind::RESCUE_MODIFIER_NODE => false,
        // ponytail: `parent.children.last == node` approximated as "only closers follow in the parent",
        // plus rubocop-ast's structural `==` against a statements list's last statement.
        _ => {
            if let Some(last) = p.as_statements_node().and_then(|s| s.body().iter().last()) {
                let l = (last.location().start_offset(), last.location().end_offset());
                if l != range && helpers::node_equality::same(src, l, range) {
                    return true;
                }
            }
            let rest = src.slice(range.1.min(p.location().end_offset()), p.location().end_offset());
            rest.split(|c: char| c.is_whitespace() || matches!(c, ')' | ']' | '}' | ';'))
                .all(|w| w.is_empty() || w == "end")
        }
    }
}
