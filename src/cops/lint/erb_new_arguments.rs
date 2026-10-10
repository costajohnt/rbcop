use crate::prelude::*;

pub const NAME: &str = "Lint/ErbNewArguments";
pub static COP: ErbNewArguments = ErbNewArguments;
pub struct ErbNewArguments;

fn is_hash(n: &Node) -> bool {
    n.as_hash_node().is_some() || n.as_keyword_hash_node().is_some()
}

/// Source of a pair key as parser sees it (a label `a:` is the symbol `a`).
fn key_source<'pr>(key: &Node<'pr>) -> &'pr str {
    if let Some(s) = key.as_symbol_node()
        && s.opening_loc().is_none()
        && let Some(v) = s.value_loc()
    {
        return text(&v);
    }
    node_text(key)
}

impl Cop for ErbNewArguments {
    fn correctable(&self) -> bool {
        true
    }

    fn ruby_range(&self) -> (f64, f64) {
        (2.6, f64::MAX)
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "new" || is_csend(&call) {
            return;
        }
        let erb = call.receiver().is_some_and(|r| {
            r.as_constant_read_node().is_some_and(|c| c.name().as_slice() == b"ERB")
                || r.as_constant_path_node()
                    .is_some_and(|c| c.parent().is_none() && c.name().is_some_and(|n| n.as_slice() == b"ERB"))
        });
        if !erb {
            return;
        }
        let args = send_args(&call);
        if args.is_empty() || args.len() == 1 || (args.len() == 2 && is_hash(&args[1])) {
            return;
        }
        // autocorrect(corrector, node)
        let mut kwargs: [Option<String>; 2] = [None, None];
        if let Some(h) = args.last().filter(|a| is_hash(a)) {
            let elements: Vec<Node> = match h.as_hash_node() {
                Some(h) => h.elements().iter().collect(),
                None => h.as_keyword_hash_node().unwrap().elements().iter().collect(),
            };
            for pair in elements.iter().filter_map(|e| e.as_assoc_node()) {
                match key_source(&pair.key()) {
                    "trim_mode" => kwargs[0] = Some(format!("trim_mode: {}", node_text(&pair.value()))),
                    "eoutvar" => kwargs[1] = Some(format!("eoutvar: {}", node_text(&pair.value()))),
                    _ => {}
                }
            }
        }
        if let Some(a) = args.get(2).filter(|a| !is_hash(a)) {
            kwargs[0] = Some(format!("trim_mode: {}", node_text(a)));
        }
        if let Some(a) = args.get(3).filter(|a| !is_hash(a)) {
            kwargs[1] = Some(format!("eoutvar: {}", node_text(a)));
        }
        let mut good = vec![node_text(&args[0]).to_string()];
        good.extend(kwargs.into_iter().flatten());
        let good = good.join(", ");
        let range = (args[0].location().start_offset(), args.last().unwrap().location().end_offset());

        for (i, arg) in args.iter().enumerate().skip(1).take(3) {
            if is_hash(arg) {
                continue;
            }
            let msg = match i {
                1 => "Passing safe_level with the 2nd argument of `ERB.new` is deprecated. Do not use it, and specify other arguments as keyword arguments.".to_string(),
                2 => format!("Passing trim_mode with the 3rd argument of `ERB.new` is deprecated. Use keyword argument like `ERB.new(str, trim_mode: {})` instead.", node_text(arg)),
                _ => format!("Passing eoutvar with the 4th argument of `ERB.new` is deprecated. Use keyword argument like `ERB.new(str, eoutvar: {})` instead.", node_text(arg)),
            };
            let l = arg.location();
            let good = good.clone();
            cx.add_fix(l.start_offset(), l.end_offset(), msg, |c| c.replace(range.0, range.1, &good));
        }
    }
}
