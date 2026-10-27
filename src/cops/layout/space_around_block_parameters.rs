use crate::helpers::range_help::{Side, Surrounding, range_with_surrounding_space};
use crate::prelude::*;

pub const NAME: &str = "Layout/SpaceAroundBlockParameters";
pub static COP: SpaceAroundBlockParameters = SpaceAroundBlockParameters;
pub struct SpaceAroundBlockParameters;

/// `check_space`: report when the two positions touch (no space between them). With `node` the
/// fix inserts a space before the range (`insert_before(node)`), otherwise at `a` (past a
/// trailing comma).
fn check_space(cx: &mut Cx, a: usize, b: usize, range: (usize, usize), msg: &str, node: bool) {
    if a == b {
        let (s, e) = range;
        cx.add_fix(s, e, format!("Space {msg} missing."), |c| {
            if node { c.insert_before(s, e, " ") } else { c.insert_after(a, a, " ") }
        });
    }
}

fn check_no_space(cx: &mut Cx, a: usize, b: usize, msg: &str) {
    if a >= b || cx.src.slice(a, b).contains('\n') {
        return;
    }
    cx.add_fix(a, b, format!("{msg} block parameter detected."), |c| c.remove(a, b));
}

fn check_arg(cx: &mut Cx, arg: &Node) {
    if kind::of(arg) == kind::MULTI_TARGET_NODE {
        for a in helpers::block_args::mlhs_children(arg) {
            check_arg(cx, &a);
        }
    }
    let start = arg.location().start_offset();
    let left = Surrounding { side: Side::Left, ..Default::default() };
    let (ws, _) = range_with_surrounding_space(cx.src.text, start, start, left);
    check_no_space(cx, ws, start.saturating_sub(1), "Extra space before");
}

impl Cop for SpaceAroundBlockParameters {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::BLOCK_NODE, kind::LAMBDA_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let (params, body) = match node.as_block_node() {
            Some(b) => (b.parameters(), b.body()),
            None => {
                let l = node.as_lambda_node().unwrap();
                (l.parameters(), l.body())
            }
        };
        let Some(params) = params.and_then(|p| p.as_block_parameters_node()) else { return };
        let (Some(open), Some(close)) = (params.opening_loc(), params.closing_loc()) else { return };
        let args = helpers::block_args::children(&params);
        let (Some(first), Some(last)) = (args.first(), args.last()) else { return };
        let style = cx.cfg.str("EnforcedStyleInsidePipes").unwrap_or("no_space").to_string();

        // checked_by_space_inside_parens?: `config.cop_enabled?` is `!!for_cop(name)['Enabled']`.
        let conflicting =
            open.as_slice() == b"(" && cx.config.for_cop("Layout/SpaceInsideParens").truthy("Enabled");

        let (first_s, first_e) = (first.location().start_offset(), first.location().end_offset());
        let last_s = last.location().start_offset();
        let last_e = last.location().end_offset();
        // last_end_pos_inside_pipes: past a trailing comma, if any, before the closing pipe.
        let params_end = params.location().end_offset();
        let last_end = match cx.src.slice(last_e, params_end).find(',') {
            Some(i) => last_e + i + 1,
            None => last_e,
        };
        let (open_e, close_s) = (open.end_offset(), close.start_offset());

        if !conflicting {
            match style.as_str() {
                "no_space" => {
                    check_no_space(cx, open_e, first_s, "Space before first");
                    check_no_space(cx, last_end, close_s, "Space after last");
                }
                "space" => {
                    check_space(cx, open_e, first_s, (first_s, first_e), "before first block parameter", true);
                    check_no_space(cx, open_e, first_s.saturating_sub(1), "Extra space before first");
                    check_space(cx, last_end, close_s, (last_s, last_e), "after last block parameter", false);
                    check_no_space(cx, last_end + 1, close_s, "Extra space after last");
                }
                _ => {}
            }
        }
        if let Some(body) = body {
            check_space(cx, close.end_offset(), body_range(&body).0, (close_s, close.end_offset()), "after closing `|`", false);
        }
        for arg in &args {
            check_arg(cx, arg);
        }
    }
}
