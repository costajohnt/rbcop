use crate::helpers::alignment::{self, Item};
use crate::helpers::macro_scope;
use crate::prelude::*;

pub const NAME: &str = "Layout/IndentationConsistency";
pub static COP: IndentationConsistency = IndentationConsistency;
pub struct IndentationConsistency;

const MSG: &str = "Inconsistent indentation detected.";

impl Cop for IndentationConsistency {
    fn correctable(&self) -> bool {
        true
    }

    // Parser `begin`/`kwbegin` nodes with two or more children are the Prism StatementsNodes with
    // two or more statements; single-child ones cannot be inconsistent.
    fn kinds(&self) -> &'static [usize] {
        &[kind::STATEMENTS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let stmts = node.as_statements_node().unwrap();
        if stmts.body().iter().count() < 2 {
            return;
        }
        let mut ancestors: Vec<&Node> = cx.ancestors.iter().collect();
        ancestors.push(node);
        let bare: Vec<bool> = stmts
            .body()
            .iter()
            .map(|c| {
                c.as_call_node().is_some_and(|call| {
                    !is_csend(&call)
                        && call.arguments().is_none()
                        && call.block().is_none()
                        && matches!(method_name(&call), "public" | "protected" | "private" | "module_function")
                        && macro_scope::is_macro(&call, &ancestors)
                })
            })
            .collect();
        if cx.cfg.str("EnforcedStyle") == Some("indented_internal_methods") {
            let mut groups: Vec<Vec<Item>> = vec![Vec::new()];
            for (child, &is_bare) in stmts.body().iter().zip(&bare) {
                if is_bare {
                    groups.push(Vec::new());
                } else {
                    groups.last_mut().unwrap().push(Item::of(child));
                }
            }
            for group in &groups {
                alignment::check_alignment(cx, group, None, MSG);
            }
            return;
        }
        let base = if bare[0] {
            let first = stmts.body().iter().next().unwrap().location().start_offset();
            let modifier_indent = alignment::display_column(cx.src, first) as i64;
            match self.parent_start(cx) {
                None => Some(modifier_indent),
                Some(p) => (modifier_indent > alignment::display_column(cx.src, p) as i64).then_some(modifier_indent),
            }
        } else {
            None
        };
        let items: Vec<Item> =
            stmts.body().iter().zip(&bare).filter(|(_, b)| !**b).map(|(c, _)| Item::of(c)).collect();
        alignment::check_alignment(cx, &items, base, MSG);
    }
}

impl IndentationConsistency {
    /// Start of the parser parent of the `begin` these statements form (None at the top level).
    fn parent_start(&self, cx: &Cx) -> Option<usize> {
        let n = cx.ancestors.len();
        let mut i = n.checked_sub(1)?;
        // `(a; b)` and `begin a; b end` are themselves the begin node.
        let p = &cx.ancestors[i];
        if kind::of(p) == kind::PARENTHESES_NODE
            || p.as_begin_node().is_some_and(|b| {
                b.begin_keyword_loc().is_some() && b.rescue_clause().is_none() && b.ensure_clause().is_none()
            })
        {
            i = i.checked_sub(1)?;
            while i > 0
                && (kind::of(&cx.ancestors[i]) == kind::ARGUMENTS_NODE
                    || cx.ancestors[i].as_statements_node().is_some_and(|s| s.body().iter().count() == 1))
            {
                i -= 1;
            }
        }
        let p = &cx.ancestors[i];
        match kind::of(p) {
            kind::PROGRAM_NODE => None,
            kind::BLOCK_NODE if i > 0 => Some(cx.ancestors[i - 1].location().start_offset()),
            kind::ELSE_NODE if i > 0 => Some(cx.ancestors[i - 1].location().start_offset()),
            _ => Some(p.location().start_offset()),
        }
    }
}
