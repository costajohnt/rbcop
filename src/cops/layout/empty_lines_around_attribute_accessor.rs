use crate::prelude::*;
use crate::source::Source;

pub const NAME: &str = "Layout/EmptyLinesAroundAttributeAccessor";
pub static COP: EmptyLinesAroundAttributeAccessor = EmptyLinesAroundAttributeAccessor;
pub struct EmptyLinesAroundAttributeAccessor;

/// rubocop-ast `attribute_accessor?`: receiverless attr_* send with at least one argument.
fn attribute_accessor(call: &ruby_prism::CallNode) -> bool {
    call.receiver().is_none()
        && !is_csend(call)
        && matches!(method_name(call), "attr_reader" | "attr_writer" | "attr_accessor" | "attr")
        && !send_args(call).is_empty()
}

/// What follows the node in its parser-gem parent (`right_sibling`).
enum Sibling<'pr> {
    None,
    Node(Node<'pr>),
    /// A node that is neither an alias nor a send (rescue body, multi-statement ensure, ...),
    /// by its start offset.
    Other(usize),
}

impl Cop for EmptyLinesAroundAttributeAccessor {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if !attribute_accessor(&call) {
            return;
        }
        let (start, end) = send_range(&call);
        if next_line_empty_or_enable_directive(cx.src, end) {
            return;
        }
        // require_empty_line?: never when the next node starts on the accessor's last line (1.92).
        let same_line = |at: usize| cx.src.line_of(at) == cx.src.line_of(end);
        let require = match self.right_sibling(cx, node) {
            Some(Sibling::None) | None => false,
            Some(Sibling::Other(at)) => !same_line(at),
            Some(Sibling::Node(n)) if same_line(n.location().start_offset()) => false,
            Some(Sibling::Node(n)) => {
                let alias = matches!(kind::of(&n), kind::ALIAS_METHOD_NODE | kind::ALIAS_GLOBAL_VARIABLE_NODE)
                    && cx.cfg.bool("AllowAliasSyntax").unwrap_or(true);
                let send = n.as_call_node().filter(|c| !is_csend(c)).is_some_and(|c| {
                    attribute_accessor(&c) || helpers::allowed::allowed_method(cx.cfg, method_name(&c))
                });
                !alias && !send
            }
        };
        if require {
            let at = empty_line_insertion(cx.src, start, end);
            cx.add_fix(start, end, "Add an empty line after attribute accessor.", |c| c.insert_after(at.0, at.1, "\n"));
        }
    }
}

/// An `enable` / `enable-next` directive comment on `line`.
fn enable_directive(src: &Source, line: usize) -> bool {
    src.comment_on_line(line).is_some_and(|c| {
        crate::directives::parse(src.slice(c.start, c.end)).is_some_and(|d| d.mode == "enable" || d.mode == "enable-next")
    })
}

/// `next_line_empty_or_enable_directive_comment?` for a node ending at `end` (shared with
/// Layout/EmptyLinesAfterModuleInclusion).
pub fn next_line_empty_or_enable_directive(src: &Source, end: usize) -> bool {
    let last_line = src.line_of(end);
    let blank = |line: usize| {
        src.lines.get(line).is_none_or(|l| l.trim_start_matches([' ', '\t', '\n', '\x0b', '\x0c', '\r', '\0']).is_empty())
    };
    // processed_source[line] is the line after `line` (1-based).
    blank(last_line) || (enable_directive(src, last_line + 1) && blank(last_line + 1))
}

/// Where the autocorrection inserts the empty line: after the node's whole lines, or after an
/// enable directive on the next line.
pub fn empty_line_insertion(src: &Source, start: usize, end: usize) -> (usize, usize) {
    let next_line = src.line_of(end) + 1;
    match src.comment_on_line(next_line) {
        Some(c) if enable_directive(src, next_line) => (c.start, c.end),
        _ => helpers::whole_lines::range_by_whole_lines(src.text, start, end, false),
    }
}

impl EmptyLinesAroundAttributeAccessor {
    /// `next_line_node`: nil when the parser parent is an `if`, else the right sibling.
    /// None when the node has no parser parent (RuboCop crashes there).
    fn right_sibling<'pr>(&self, cx: &Cx<'_, 'pr>, node: &Node) -> Option<Sibling<'pr>> {
        let n = cx.ancestors.len();
        let start = node.location().start_offset();
        let parent = cx.ancestors.last()?;
        let after = |list: Vec<Node<'pr>>| {
            let i = list.iter().position(|s| s.location().start_offset() == start);
            let mut list = list;
            i.filter(|&i| i + 1 < list.len()).map_or(Sibling::None, |i| Sibling::Node(list.swap_remove(i + 1)))
        };
        if let Some(args) = parent.as_arguments_node() {
            let mut list: Vec<Node> = args.arguments().iter().collect();
            if let Some(b) = cx.ancestors.get(n.wrapping_sub(2)).and_then(|c| c.as_call_node()).and_then(|c| c.block())
                && kind::of(&b) == kind::BLOCK_ARGUMENT_NODE
            {
                list.push(b);
            }
            return Some(after(list));
        }
        if let Some(arr) = parent.as_array_node() {
            return Some(after(arr.elements().iter().collect()));
        }
        let Some(stmts) = parent.as_statements_node() else { return Some(Sibling::None) };
        let body: Vec<Node> = stmts.body().iter().collect();
        if body.len() > 1 {
            return Some(after(body));
        }
        // A single statement: its parser parent is the Prism grandparent.
        let grand = cx.ancestors.get(n.wrapping_sub(2))?;
        if kind::of(grand) == kind::PROGRAM_NODE {
            return None;
        }
        let Some(begin) = grand.as_begin_node() else { return Some(Sibling::None) };
        if let Some(r) = begin.rescue_clause() {
            return Some(Sibling::Other(r.location().start_offset()));
        }
        let Some(ensure) = begin.ensure_clause() else { return Some(Sibling::None) };
        let mut stmts: Vec<Node> = ensure.statements().map(|s| s.body().iter().collect()).unwrap_or_default();
        Some(match stmts.len() {
            0 => Sibling::None,
            1 => Sibling::Node(stmts.remove(0)),
            _ => Sibling::Other(stmts[0].location().start_offset()),
        })
    }
}
