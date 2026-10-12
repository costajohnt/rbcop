use crate::helpers::variable_force::{self as vf, Tree};
use crate::prelude::*;

pub const NAME: &str = "Style/InfiniteLoop";
pub static COP: InfiniteLoop = InfiniteLoop;
pub struct InfiniteLoop;

/// Every variable VariableForce saw in the file (`after_leaving_scope` runs before the walk):
/// assignment ranges and reference start offsets.
#[derive(Default)]
struct Vars(Vec<VarUses>);

/// (assignment ranges, reference starts) of one variable.
type VarUses = (Vec<(usize, usize)>, Vec<usize>);

fn truthy_literal(n: &Node) -> bool {
    matches!(
        crate::helpers::literal_node::folded_kind(n),
        kind::STRING_NODE
            | kind::INTERPOLATED_STRING_NODE
            | kind::X_STRING_NODE
            | kind::INTERPOLATED_X_STRING_NODE
            | kind::SOURCE_FILE_NODE
            | kind::INTEGER_NODE
            | kind::SOURCE_LINE_NODE
            | kind::FLOAT_NODE
            | kind::SYMBOL_NODE
            | kind::INTERPOLATED_SYMBOL_NODE
            | kind::ARRAY_NODE
            | kind::HASH_NODE
            | kind::REGULAR_EXPRESSION_NODE
            | kind::INTERPOLATED_REGULAR_EXPRESSION_NODE
            | kind::TRUE_NODE
            | kind::RANGE_NODE
            | kind::IMAGINARY_NODE
            | kind::RATIONAL_NODE
    )
}

impl Cop for InfiniteLoop {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::PROGRAM_NODE, kind::WHILE_NODE, kind::UNTIL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if let Some(p) = node.as_program_node() {
            let tree = Tree::build(&p, cx.src);
            let mut all = Vec::new();
            vf::investigate(&tree, |tree, scope, vars| {
                for &v in &scope.variables {
                    let var = &vars[v];
                    let assigns = var.assignments.iter().map(|a| tree.range(a.node)).collect();
                    let refs = var.references.iter().map(|r| tree.range(r.node).0).collect();
                    all.push((assigns, refs));
                }
            });
            cx.state::<Vars>().0 = all;
            return;
        }
        let (keyword, pred, stmts, post, closing, do_kw) = match node.as_while_node() {
            Some(w) => (w.keyword_loc(), w.predicate(), w.statements(), w.is_begin_modifier(), w.closing_loc(), w.do_keyword_loc()),
            None => {
                let u = node.as_until_node().unwrap();
                (u.keyword_loc(), u.predicate(), u.statements(), u.is_begin_modifier(), u.closing_loc(), u.do_keyword_loc())
            }
        };
        let infinite = if node.as_while_node().is_some() {
            truthy_literal(&pred)
        } else {
            matches!(kind::of(&pred), kind::FALSE_NODE | kind::NIL_NODE)
        };
        if !infinite {
            return;
        }
        let (start, end) = (node.location().start_offset(), node.location().end_offset());
        let blocked = cx.state::<Vars>().0.iter().any(|(assigns, refs)| {
            assigns.iter().any(|&(s, e)| s >= start && e <= end)
                && !assigns.iter().any(|&(_, e)| e < start)
                && refs.iter().any(|&r| r > end)
        });
        if blocked {
            return;
        }
        let src = cx.src;
        let edits: Vec<(usize, usize, String)> = if post {
            // replace_begin_end_with_modifier
            let Some(b) = stmts.and_then(|s| s.body().iter().next()).and_then(|b| b.as_begin_node()) else { return };
            let (Some(bk), Some(ek)) = (b.begin_keyword_loc(), b.end_keyword_loc()) else { return };
            vec![(bk.start_offset(), bk.end_offset(), "loop do".into()), (ek.end_offset(), end, String::new())]
        } else if closing.is_none() {
            // modifier form
            let body = stmts.map(|s| (s.location().start_offset(), s.location().end_offset())).unwrap_or((start, start));
            let body_src = src.slice(body.0, body.1);
            let text = if src.line_of(start) == src.line_of(end) {
                format!("loop {{ {body_src} }}")
            } else {
                let line = src.lines.get(src.line_of(body.0) - 1).copied().unwrap_or("");
                let lead: String = line.chars().take_while(|c| c.is_ascii_whitespace() || *c == '\x0b').collect();
                let width = helpers::alignment::configured_indentation_width(cx) as usize;
                let ind = " ".repeat(src.column(start) + width);
                let body_ind: Vec<String> = body_src.split('\n').map(|l| format!("{ind}{l}")).collect();
                ["loop do".to_string(), body_ind.join("\n"), "end".to_string()].join(&format!("\n{lead}"))
            };
            vec![(start, end, text)]
        } else {
            let stop = do_kw.map_or(pred.location().end_offset(), |d| d.end_offset());
            vec![(keyword.start_offset(), stop, "loop do".into())]
        };
        cx.add_fix(keyword.start_offset(), keyword.end_offset(), "Use `Kernel#loop` for infinite loops.", |c| {
            for (s, e, t) in &edits {
                c.replace(*s, *e, t);
            }
        });
    }
}
