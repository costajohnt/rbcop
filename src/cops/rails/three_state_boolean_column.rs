use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Rails/ThreeStateBooleanColumn";
pub static COP: ThreeStateBooleanColumn = ThreeStateBooleanColumn;
pub struct ThreeStateBooleanColumn;

fn boolean_type(n: &Node) -> bool {
    helpers::rails::sym_value(n).as_deref() == Some("boolean") || helpers::rails::str_value(n).as_deref() == Some("boolean")
}

impl Cop for ThreeStateBooleanColumn {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        let name = method_name(&call);
        let args = send_args(&call);
        let has_recv = call.receiver().is_some();
        let (column, options) = match (name, args.as_slice()) {
            ("add_column", [_, col, t, rest @ ..]) if !has_recv && boolean_type(t) && rest.len() <= 1 => (col, rest.first()),
            ("column", [col, t, rest @ ..]) if has_recv && boolean_type(t) && rest.len() <= 1 => (col, rest.first()),
            ("boolean", [col, rest @ ..]) if has_recv && rest.len() <= 1 => (col, rest.first()),
            _ => return,
        };
        // (hash <(pair (sym :default) !nil?) (pair (sym :null) false) ...>)
        let required = options.is_some_and(|o| {
            let Some(elems) = helpers::rails::hash_elements(o) else { return false };
            let pairs: Vec<ruby_prism::AssocNode> = elems.iter().filter_map(|e| e.as_assoc_node()).collect();
            let default = pairs.iter().any(|p| {
                helpers::rails::sym_value(&p.key()).as_deref() == Some("default") && p.value().as_nil_node().is_none()
            });
            let null = pairs.iter().any(|p| {
                helpers::rails::sym_value(&p.key()).as_deref() == Some("null") && p.value().as_false_node().is_some()
            });
            default && null
        });
        if required {
            return;
        }
        let anc = cx.ancestors;
        let def = anc.iter().rev().find(|a| a.as_def_node().is_some());
        let table_range: Option<(usize, usize)> = if name == "add_column" {
            Some((args[0].location().start_offset(), args[0].location().end_offset()))
        } else {
            // each_ancestor(:block).find { create_table / change_table }&.send_node&.first_argument
            anc.iter()
                .enumerate()
                .rev()
                .filter(|(i, a)| a.as_block_node().is_some() && *i > 0)
                .find_map(|(i, _)| {
                    let c = helpers::rails::block_call(&anc[i - 1])?;
                    matches!(method_name(&c), "create_table" | "change_table").then_some(c)
                })
                .and_then(|c| send_args(&c).first().map(|a| (a.location().start_offset(), a.location().end_offset())))
        };
        if let Some(def) = def {
            let Some(table) = table_range else { return };
            let col = (column.location().start_offset(), column.location().end_offset());
            // (send nil? :change_column_null %1 %2 false) anywhere in the def.
            struct F<'s, 'a> {
                src: &'s crate::source::Source<'a>,
                table: (usize, usize),
                col: (usize, usize),
                found: bool,
            }
            impl<'pr> Visit<'pr> for F<'_, '_> {
                fn visit_call_node(&mut self, c: &ruby_prism::CallNode<'pr>) {
                    let a = send_args(c);
                    if !is_csend(c)
                        && helpers::rails::is_command(c, "change_column_null")
                        && a.len() == 3
                        && a[2].as_false_node().is_some()
                        && helpers::node_equality::same(self.src, (a[0].location().start_offset(), a[0].location().end_offset()), self.table)
                        && helpers::node_equality::same(self.src, (a[1].location().start_offset(), a[1].location().end_offset()), self.col)
                    {
                        self.found = true;
                    }
                    ruby_prism::visit_call_node(self, c);
                }
            }
            let mut f = F { src: cx.src, table, col, found: false };
            f.visit(def);
            if f.found {
                return;
            }
        }
        let (s, e) = send_range(&call);
        cx.add(s, e, "Boolean columns should always have a default value and a `NOT NULL` constraint.");
    }
}
