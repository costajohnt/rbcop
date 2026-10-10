use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Style/SingleLineBlockParams";
pub static COP: SingleLineBlockParams = SingleLineBlockParams;
pub struct SingleLineBlockParams;

/// `each_descendant(:lvar)` ranges and names.
#[derive(Default)]
struct Lvars(Vec<(usize, usize, String)>);

impl<'pr> Visit<'pr> for Lvars {
    fn visit_local_variable_read_node(&mut self, node: &ruby_prism::LocalVariableReadNode<'pr>) {
        let l = node.location();
        self.0.push((l.start_offset(), l.end_offset(), node_text(&node.as_node()).to_string()));
    }
    // `id:` value omission: parser's lvar is `id` (Prism's spans the colon too).
    fn visit_implicit_node(&mut self, node: &ruby_prism::ImplicitNode<'pr>) {
        if let Some(lvar) = node.value().as_local_variable_read_node() {
            let name = String::from_utf8_lossy(lvar.name().as_slice()).into_owned();
            let start = lvar.location().start_offset();
            self.0.push((start, start + name.len(), name));
        }
    }
}

/// The configured argument names for `method` (`Methods: [{reduce: [acc, elem]}, ...]`).
fn target_args(cx: &Cx, method: &str) -> Option<Vec<String>> {
    let Some(Value::Seq(methods)) = cx.cfg.get("Methods") else { return None };
    methods.iter().filter_map(Value::as_map).find_map(|m| {
        let (k, v) = m.0.first()?;
        (k == method).then(|| match v {
            Value::Seq(items) => items.iter().filter_map(Value::as_string).collect(),
            _ => Vec::new(),
        })
    })
}

impl Cop for SingleLineBlockParams {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return };
        let src = cx.src;
        if src.line_of(block.opening_loc().start_offset()) != src.line_of(block.closing_loc().start_offset()) {
            return;
        }
        if call.receiver().is_none() {
            return;
        }
        let method = method_name(&call);
        let Some(target) = target_args(cx, method) else { return };
        // eligible_arguments?: `|a, b|` with plain arguments only.
        let Some(bp) = block.parameters().and_then(|p| p.as_block_parameters_node()) else { return };
        if bp.locals().iter().next().is_some() {
            return;
        }
        let Some(params) = bp.parameters() else { return };
        if parameter_nodes(&params).iter().any(|p| p.as_required_parameter_node().is_none()) {
            return;
        }
        let names: Vec<&str> = params
            .requireds()
            .iter()
            .chain(params.posts().iter())
            .map(|p| std::str::from_utf8(p.as_required_parameter_node().map_or(&[][..], |r| r.name().as_slice())).unwrap_or(""))
            .collect();
        if names.is_empty() {
            return;
        }
        // args_match?
        let stripped: Vec<&str> = names.iter().map(|n| n.trim_start_matches('_')).collect();
        let expected: Vec<&str> = target.iter().take(stripped.len()).map(String::as_str).collect();
        if stripped == expected {
            return;
        }
        // build_preferred_arguments_map (a Hash: duplicate keys keep their first position).
        // Values are nil past the configured names (`"_#{nil}"` is `"_"`).
        let mut map: Vec<(String, Option<String>)> = Vec::new();
        for (i, current) in names.iter().enumerate() {
            let pref = target.get(i).cloned();
            let pref = if current.starts_with('_') { Some(format!("_{}", pref.unwrap_or_default())) } else { pref };
            match map.iter_mut().find(|(k, _)| k == current) {
                Some(entry) => entry.1 = pref,
                None => map.push((current.to_string(), pref)),
            }
        }
        let joined = map.iter().map(|(_, v)| v.as_deref().unwrap_or("")).collect::<Vec<_>>().join(", ");
        let msg = format!("Name `{method}` block params `|{joined}|`.");
        let loc = bp.location();
        let (start, end) = (loc.start_offset(), loc.end_offset());
        let mut lvars = Lvars::default();
        lvars.visit(node);
        cx.add_fix(start, end, msg, |c| {
            c.replace(start, end, &format!("|{joined}|"));
            for (s, e, name) in &lvars.0 {
                if let Some((_, Some(v))) = map.iter().find(|(k, _)| k == name) {
                    c.replace(*s, *e, v);
                }
            }
        });
    }
}
