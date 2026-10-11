use crate::prelude::*;

pub const NAME: &str = "Layout/EmptyLinesAroundAccessModifier";
pub static COP: EmptyLinesAroundAccessModifier = EmptyLinesAroundAccessModifier;
pub struct EmptyLinesAroundAccessModifier;

/// RuboCop's instance variables, set by on_class/on_module/on_sclass/on_block in traversal
/// order and never reset (so a nested class seen earlier leaks into a later modifier).
#[derive(Default)]
struct State {
    class_lines: Option<(usize, usize)>,
    block_line: Option<usize>,
}

static COMMENT_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[ \t\r\n\x0c\x0b]*#").unwrap());

fn blank(line: &str) -> bool {
    helpers::ruby_string::strip(line).is_empty()
}

impl Cop for EmptyLinesAroundAccessModifier {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[
            kind::CLASS_NODE,
            kind::MODULE_NODE,
            kind::SINGLETON_CLASS_NODE,
            kind::CALL_NODE,
            kind::SUPER_NODE,
            kind::FORWARDING_SUPER_NODE,
            kind::LAMBDA_NODE,
        ]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let src = cx.src;
        let line = |off: usize| src.line_of(off);
        let loc = node.location();
        let (start, end) = (loc.start_offset(), loc.end_offset());
        // A parser block node is visited before its call; Prism visits the call first, so record
        // the block at the call (its range starts where the call does).
        let block = match kind::of(node) {
            kind::CALL_NODE => node.as_call_node().unwrap().block(),
            kind::SUPER_NODE => node.as_super_node().unwrap().block(),
            kind::FORWARDING_SUPER_NODE => node.as_forwarding_super_node().unwrap().block().map(|b| b.as_node()),
            _ => None,
        };
        if block.is_some_and(|b| b.as_block_node().is_some()) || kind::of(node) == kind::LAMBDA_NODE {
            cx.state::<State>().block_line = Some(line(start));
        }
        match kind::of(node) {
            kind::CLASS_NODE => {
                let first = node.as_class_node().unwrap().superclass().map_or(start, |s| s.location().start_offset());
                cx.state::<State>().class_lines = Some((line(first), line(end)));
            }
            kind::MODULE_NODE => cx.state::<State>().class_lines = Some((line(start), line(end))),
            kind::SINGLETON_CLASS_NODE => {
                let first = node.as_singleton_class_node().unwrap().expression().location().start_offset();
                cx.state::<State>().class_lines = Some((line(first), line(end)));
            }
            kind::CALL_NODE => self.check(cx, node),
            _ => {}
        }
    }
}

impl EmptyLinesAroundAccessModifier {
    fn check(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if !matches!(name, "public" | "protected" | "private" | "module_function")
            || call.arguments().is_some()
            || call.block().is_some()
        {
            return;
        }
        if !helpers::macro_scope::is_macro(&call, cx.ancestors) {
            return;
        }
        let src = cx.src;
        let loc = node.location();
        let first_line = src.line_of(loc.start_offset());
        let last_line = src.line_of(loc.end_offset());
        // right_sibling: the next statement of a `begin` body
        let anc = cx.ancestors;
        let stmts = anc.last().and_then(|p| p.as_statements_node()).map(|s| s.body().iter().collect::<Vec<_>>());
        let multi = stmts.as_ref().filter(|s| s.len() > 1);
        let index = multi.and_then(|s| s.iter().position(|n| n.location().start_offset() == loc.start_offset()));
        if let (Some(s), Some(i)) = (multi, index)
            && s.get(i + 1).is_some_and(|n| src.line_of(n.location().start_offset()) == first_line)
        {
            return;
        }

        let state = cx.state::<State>();
        let (class_lines, block_line) = (state.class_lines, state.block_line);
        let block_start = |l: usize| block_line.is_some_and(|b| l == b + 1);
        let class_def = |l: usize| class_lines.is_some_and(|(f, _)| l == f + 1);
        let body_end = |l: usize| class_lines.is_some_and(|(_, e)| l + 1 == e);
        let lines = &src.lines;
        let previous_line_empty = {
            // `processed_source[0..(send_line - 2)]`: on line 1 that is `[0..-1]`, every line, so the
            // file's last non-comment line decides (blank only when the file ends in a newline).
            let upto = if first_line == 1 { lines.len() } else { (first_line - 1).min(lines.len()) };
            let prev = lines[..upto].iter().rev().find(|l| !COMMENT_LINE.is_match(l));
            prev.is_none_or(|p| block_start(first_line) || class_def(first_line) || blank(p))
        };
        // next_line_empty?: `nil.blank?` raises in RuboCop past the last line, so no offense there.
        let next_line_empty = if body_end(last_line) {
            true
        } else {
            match lines.get(last_line) {
                Some(l) => blank(l),
                None => return,
            }
        };
        let next_line_empty_and_exists = next_line_empty && last_line + 1 != lines.len();

        let style = cx.cfg.str("EnforcedStyle").unwrap_or("around");
        let modifier = name;
        let (ok, msg) = match style {
            "around" => {
                let msg = if block_start(first_line) || class_def(first_line) {
                    format!("Keep a blank line after `{modifier}`.")
                } else {
                    format!("Keep a blank line before and after `{modifier}`.")
                };
                (previous_line_empty && next_line_empty, msg)
            }
            "only_before" => {
                let special = matches!(name, "private" | "protected");
                let ok = if special && lines.get(last_line).is_some_and(|l| *l == "end") {
                    true
                } else if special && next_line_empty_and_exists {
                    false
                } else {
                    previous_line_empty
                };
                let msg = if next_line_empty {
                    format!("Remove a blank line after `{modifier}`.")
                } else {
                    format!("Keep a blank line before `{modifier}`.")
                };
                (ok, msg)
            }
            _ => return,
        };
        if ok {
            return;
        }

        // [Correctable] only if the corrector edits something.
        let parent_block = |n: Option<&Node>| {
            n.is_some_and(|p| {
                let params = match kind::of(p) {
                    kind::BLOCK_NODE => p.as_block_node().unwrap().parameters(),
                    kind::LAMBDA_NODE => p.as_lambda_node().unwrap().parameters(),
                    _ => return false,
                };
                params.is_none_or(|x| x.as_block_parameters_node().is_some())
            })
        };
        let inside_block = anc.len() >= 2 && parent_block(anc.get(anc.len() - 2));
        let blk = cx.config.for_cop("Layout/EmptyLinesAroundBlockBody");
        let no_empty_lines = blk.truthy("Enabled") && blk.str("EnforcedStyle") == Some("no_empty_lines");
        let strict = inside_block && no_empty_lines;
        // `children.first != node` compares structurally: an identical modifier counts as the node.
        let me = (loc.start_offset(), loc.end_offset());
        let same = |n: &Node| helpers::node_equality::same(src, (n.location().start_offset(), n.location().end_offset()), me);
        let is_first = multi.is_some_and(|s| same(&s[0]));
        let is_last = multi.is_some_and(|s| same(&s[s.len() - 1]));
        let insert_before = !previous_line_empty && (!strict || multi.is_none() || !is_first);
        let insert_after = !strict || (multi.is_some() && !is_last);
        let line = helpers::whole_lines::range_by_whole_lines(src.text, loc.start_offset(), loc.end_offset(), false);
        // next_empty_line_range: source_range(buffer, last_line + 1, 0), one character.
        let next = if last_line < src.line_count() { src.line_start(last_line + 1) } else { src.text.len() };
        let next = (next, next + src.text[next..].chars().next().map_or(0, char::len_utf8));
        let around = style == "around";
        cx.add_fix(loc.start_offset(), loc.end_offset(), msg, |c| {
            if insert_before {
                c.insert_before(line.0, line.1, "\n");
            }
            if insert_after && around && !next_line_empty {
                c.insert_after(line.0, line.1, "\n");
            }
            if insert_after && !around && next_line_empty_and_exists {
                c.remove(next.0, next.1);
            }
        });
    }
}
