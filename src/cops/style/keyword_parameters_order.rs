use crate::prelude::*;
use helpers::range_help::{range_with_surrounding_space, Side, Surrounding};

pub const NAME: &str = "Style/KeywordParametersOrder";
pub static COP: KeywordParametersOrder = KeywordParametersOrder;
pub struct KeywordParametersOrder;

fn range(n: &Node) -> (usize, usize) {
    (n.location().start_offset(), n.location().end_offset())
}

impl Cop for KeywordParametersOrder {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::OPTIONAL_KEYWORD_PARAMETER_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(params) = cx.parent().and_then(|p| p.as_parameters_node()) else { return };
        let me = range(node);
        let keywords: Vec<Node> = params.keywords().iter().collect();
        let kwargs: Vec<(usize, usize)> = keywords
            .iter()
            .filter(|k| k.as_required_keyword_parameter_node().is_some() && range(k).0 > me.0)
            .map(range)
            .collect();
        if kwargs.is_empty() {
            return;
        }
        let first_kwopt = keywords.iter().find(|k| k.as_optional_keyword_parameter_node().is_some()).map(range);

        // defining_node = each_ancestor(:any_def, :block).first, with its parser `args` node
        let mut all: Vec<(usize, usize)> = parameter_nodes(&params).iter().map(range).collect();
        let mut parens = false;
        let mut is_block = false;
        let mut last_is_kwarg = parameter_nodes(&params).last().is_some_and(|n| n.as_required_keyword_parameter_node().is_some());
        for a in cx.ancestors.iter().rev() {
            if let Some(d) = a.as_def_node() {
                parens = d.rparen_loc().is_some();
                break;
            }
            let bp = if let Some(b) = a.as_block_node() {
                b.parameters()
            } else if let Some(l) = a.as_lambda_node() {
                l.parameters()
            } else {
                continue;
            };
            is_block = true;
            if let Some(bp) = bp.and_then(|p| p.as_block_parameters_node()) {
                parens = bp.closing_loc().is_some_and(|c| c.as_slice() == b")");
                let locals: Vec<(usize, usize)> = bp.locals().iter().map(|l| range(&l)).collect();
                if !locals.is_empty() {
                    last_is_kwarg = false;
                }
                all.extend(locals);
            }
            break;
        }
        let (args_start, args_end) = (all.iter().map(|r| r.0).min().unwrap_or(me.0), all.iter().map(|r| r.1).max().unwrap_or(me.1));
        let src = cx.src;
        let has_comment = helpers::comments_help::contains_comments(cx, src.line_of(args_start), src.line_of(args_end) + 1);
        let correct = !has_comment && first_kwopt == Some(me);
        let insert = format!("{}, ", kwargs.iter().map(|&(a, b)| &src.text[a..b]).collect::<Vec<_>>().join(", "));
        let last_kwopt = keywords.iter().rev().find(|k| k.as_optional_keyword_parameter_node().is_some()).map(range);
        let newline = !parens && last_is_kwarg && !is_block;
        let removals: Vec<(usize, usize)> = kwargs
            .iter()
            .map(|&(a, b)| {
                // 1.92: all whitespace (newlines included) on the left, then spaces on the right.
                let left = Surrounding { side: Side::Left, whitespace: true, ..Default::default() };
                let (s, _) = range_with_surrounding_space(src.text, a, b, left);
                let right = Surrounding { side: Side::Right, ..Default::default() };
                let (s, e) = range_with_surrounding_space(src.text, s, b, right);
                let bytes = src.text.as_bytes();
                let mut s = s;
                while s > 0 && bytes[s - 1] == b',' {
                    s -= 1;
                }
                (s, e)
            })
            .collect();
        cx.add_fix(me.0, me.1, "Place optional keyword parameters at the end of the parameters list.", |c| {
            if !correct {
                return;
            }
            c.insert_before(me.0, me.1, &insert);
            if newline
                && let Some((a, b)) = last_kwopt {
                    c.insert_after(a, b, "\n");
                }
            for (s, e) in removals {
                c.remove(s, e);
            }
        });
    }
}
