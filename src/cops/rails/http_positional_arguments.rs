use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Rails/HttpPositionalArguments";
pub static COP: HttpPositionalArguments = HttpPositionalArguments;
pub struct HttpPositionalArguments;

const KEYWORD_ARGS: &[&str] = &["method", "params", "session", "body", "flash", "xhr", "as", "headers", "env", "to"];

/// use_rack_test_methods?: the file has `include Rack::Test::Methods` somewhere.
#[derive(Default)]
struct RackTest(Option<bool>);

fn rack_test(program: &Node) -> bool {
    struct F(bool);
    impl<'pr> Visit<'pr> for F {
        fn visit_call_node(&mut self, c: &ruby_prism::CallNode<'pr>) {
            if !is_csend(c) && helpers::rails::is_command(c, "include") {
                let a = send_args(c);
                if let [m] = a.as_slice()
                    && helpers::rails::is_const(m, "Rack::Test::Methods")
                {
                    self.0 = true;
                }
            }
            ruby_prism::visit_call_node(self, c);
        }
    }
    let mut f = F(false);
    f.visit(program);
    f.0
}

/// convert_hash_data
fn convert(data: &Node, ty: &str) -> String {
    if let Some(elems) = helpers::rails::hash_elements(data) {
        if elems.is_empty() {
            return String::new();
        }
        let pairs: Vec<&str> = elems.iter().filter(|e| e.as_assoc_node().is_some()).map(|e| node_text(e)).collect();
        return format!(", {ty}: {{ {} }}", pairs.join(", "));
    }
    format!(", {ty}: {}", node_text(data))
}

impl Cop for HttpPositionalArguments {
    fn correctable(&self) -> bool {
        true
    }

    fn min_rails(&self) -> f64 {
        5.0
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        let verb = method_name(&call);
        if !matches!(verb, "get" | "post" | "put" | "patch" | "delete" | "head") {
            return;
        }
        let anc = cx.ancestors;
        // in_routing_block?: inside a `draw` / `routes` block.
        let routing = (1..anc.len()).any(|i| {
            anc[i].as_block_node().is_some_and(|b| {
                !b.parameters().is_some_and(|p| p.as_numbered_parameters_node().is_some() || p.as_it_parameters_node().is_some())
            }) && anc[i - 1].as_call_node().is_some_and(|c| matches!(method_name(&c), "draw" | "routes"))
        });
        if routing {
            return;
        }
        let rack = match cx.state::<RackTest>().0 {
            Some(v) => v,
            None => {
                let v = anc.first().is_some_and(rack_test);
                cx.state::<RackTest>().0 = Some(v);
                v
            }
        };
        if rack {
            return;
        }
        // (send nil? VERB !nil? $_ ...)
        if call.receiver().is_some() {
            return;
        }
        let args = send_args(&call);
        if args.len() < 2 || args[0].as_nil_node().is_some() {
            return;
        }
        let data = &args[1];
        // needs_conversion?
        if data.as_forwarding_arguments_node().is_some() {
            return;
        }
        if let Some(elems) = helpers::rails::hash_elements(data) {
            if let [only] = elems.as_slice()
                && let Some(s) = only.as_assoc_splat_node()
            {
                // (hash (forwarded-kwrestarg)) / (hash (kwsplat _))
                let _ = s;
                return;
            }
            let pairs = helpers::rails::hash_pairs(data);
            let special = pairs.iter().any(|p| {
                let k = helpers::rails::sym_value(&p.key());
                k.as_deref().is_some_and(|k| KEYWORD_ARGS.contains(&k)) || (k.as_deref() == Some("format") && pairs.len() == 1)
            });
            if special {
                return;
            }
        }
        let rest = &args[1..];
        let (hs, he) = (rest[0].location().start_offset(), rest.last().unwrap().location().end_offset());
        let params = convert(&rest[0], "params");
        let session = if rest.len() > 1 { convert(rest.last().unwrap(), "session") } else { String::new() };
        let action = node_text(&args[0]);
        let replacement = if call.closing_loc().is_some_and(|c| c.as_slice() == b")") {
            format!("{verb}({action}{params}{session})")
        } else {
            format!("{verb} {action}{params}{session}")
        };
        let (s, e) = send_range(&call);
        cx.add_fix(hs, he, format!("Use keyword arguments instead of positional arguments for http call: `{verb}`."), |c| {
            c.replace(s, e, &replacement)
        });
    }
}
