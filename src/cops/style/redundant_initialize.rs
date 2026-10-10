use crate::prelude::*;

pub const NAME: &str = "Style/RedundantInitialize";
pub static COP: RedundantInitialize = RedundantInitialize;
pub struct RedundantInitialize;

impl Cop for RedundantInitialize {
    fn correctable(&self) -> bool {
        true
    }
    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let def = node.as_def_node().unwrap();
        if def.receiver().is_some() || def.name().as_slice() != b"initialize" {
            return;
        }
        let params = def.parameters();
        // `forwards?`: restarg, kwrestarg, forward_args / forward_arg.
        if params.as_ref().is_some_and(|p| {
            parser_rest(p.rest()).is_some_and(|r| r.as_rest_parameter_node().is_some())
                || p.keyword_rest().is_some_and(|k| {
                    k.as_keyword_rest_parameter_node().is_some() || k.as_forwarding_parameter_node().is_some()
                })
        }) {
            return;
        }
        let loc = node.location();
        let (start, end) = (loc.start_offset(), loc.end_offset());
        let end_line = helpers::comments_help::find_end_line(cx, node);
        if helpers::comments_help::allow_comments(cx, cx.src.line_of(start), end_line) {
            return;
        }
        let args = params.as_ref().map(parameter_nodes).unwrap_or_default();
        let msg = match def.body() {
            None if args.is_empty() => "Remove unnecessary empty `initialize` method.",
            None => return,
            Some(body) => {
                let Some(stmts) = body.as_statements_node() else { return };
                let mut it = stmts.body().iter();
                let (Some(stmt), None) = (it.next(), it.next()) else { return };
                // `(def _ (args $arg*) $({super zsuper} ...))`
                let names = args
                    .iter()
                    .map(|a| a.as_required_parameter_node().map(|r| r.name().as_slice()))
                    .collect::<Option<Vec<_>>>();
                let Some(names) = names else { return };
                if let Some(sup) = stmt.as_super_node() {
                    if sup.block().is_some_and(|b| b.as_block_node().is_some()) {
                        return;
                    }
                    // `args.map(&:name) == super_node.arguments.map { |a| a.children[0] }`
                    let mut sargs: Vec<Node> = sup.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default();
                    sargs.extend(sup.block());
                    let first_child = |a: &Node| -> Option<Vec<u8>> {
                        if let Some(l) = a.as_local_variable_read_node() {
                            return Some(l.name().as_slice().to_vec());
                        }
                        if let Some(l) = a.as_local_variable_write_node() {
                            return Some(l.name().as_slice().to_vec());
                        }
                        a.as_symbol_node().filter(|s| s.value_loc().is_some()).map(|s| s.unescaped().to_vec())
                    };
                    if sargs.len() != names.len()
                        || !sargs.iter().zip(&names).all(|(a, n)| first_child(a).is_some_and(|c| c == *n))
                    {
                        return;
                    }
                } else if let Some(z) = stmt.as_forwarding_super_node() {
                    if z.block().is_some() {
                        return;
                    }
                } else {
                    return;
                }
                "Remove unnecessary `initialize` method."
            }
        };
        let (rs, re) = helpers::whole_lines::range_by_whole_lines(cx.src.text, start, end, true);
        cx.add_fix(start, end, msg, |c| c.remove(rs, re));
    }
}
