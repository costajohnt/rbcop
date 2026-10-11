use crate::prelude::*;

pub const NAME: &str = "Lint/EmptyBlock";
pub static COP: EmptyBlock = EmptyBlock;
pub struct EmptyBlock;

/// `lambda_or_proc?` of a block owned by `owner`: BlockNode#lambda? (`lambda` with any receiver),
/// `proc { }`, `Proc.new { }` / `::Proc.new { }`.
fn lambda_or_proc(owner: &Node) -> bool {
    let Some(call) = owner.as_call_node() else { return false };
    if is_csend(&call) {
        return false;
    }
    match method_name(&call) {
        "lambda" => true,
        "proc" => call.receiver().is_none(),
        "new" => call.receiver().is_some_and(|r| helpers::parser_parent::is_top_const(&r, "Proc")),
        _ => false,
    }
}

/// `comment_disables_cop?`: a disable/todo directive naming this cop (`all`, its department, or
/// its qualified or bare name).
fn comment_disables_cop(comment: &str, cop: &str) -> bool {
    let Some(d) = crate::directives::parse(comment) else { return false };
    if !matches!(d.mode.as_str(), "disable" | "todo" | "disable-next" | "todo-next") {
        return false;
    }
    let (dept, bare) = cop.split_once('/').unwrap_or(("", cop));
    let Some(cops) = d.cops.as_deref() else { return false };
    cops == "all" || cops.split(',').map(str::trim).any(|n| n == cop || n == bare || n == dept)
}

impl Cop for EmptyBlock {
    fn kinds(&self) -> &'static [usize] {
        // A block is handled on entering its owner: the parser `block` wraps its send, so RuboCop
        // reaches the outer block of `foo {}.to bar do end` before the receiver's (and Runner's
        // uniq keeps the first offense at a position).
        &[kind::CALL_NODE, kind::SUPER_NODE, kind::FORWARDING_SUPER_NODE, kind::LAMBDA_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let (body, lambda_or_proc, end) = match node.as_lambda_node() {
            Some(l) => (l.body(), true, node.location().end_offset()),
            None => {
                let Some(b) = helpers::node_types::literal_block(node) else { return };
                (b.body(), lambda_or_proc(node), b.location().end_offset())
            }
        };
        if body.is_some() {
            return;
        }
        if lambda_or_proc && cx.cfg.bool("AllowEmptyLambdas").unwrap_or(false) {
            return;
        }
        let start = node.location().start_offset();
        if cx.cfg.bool("AllowComments").unwrap_or(false) {
            let (first, last) = (cx.src.line_of(start), cx.src.line_of(end));
            let contains = cx.src.comments.iter().any(|c| (first..=last).contains(&cx.src.line_of(c.start)));
            if contains {
                let disables = cx.src.comment_on_line(first).is_some_and(|c| {
                    comment_disables_cop(&cx.src.text[c.start..c.end], cx.cop)
                });
                if !disables {
                    return;
                }
            }
        }
        cx.add(start, end, "Empty block detected.");
    }
}
