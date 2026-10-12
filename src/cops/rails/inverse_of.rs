use crate::prelude::*;

pub const NAME: &str = "Rails/InverseOf";
pub static COP: InverseOf = InverseOf;
pub struct InverseOf;

/// An option: a hash's pair (key name, value is nil literal) or a kwsplat.
enum Opt {
    Pair(Option<String>, bool),
    Splat,
}

fn options_of(arg: &Node, out: &mut Vec<Opt>) {
    let Some(elems) = helpers::rails::hash_elements(arg) else { return };
    for e in elems {
        if let Some(p) = e.as_assoc_node() {
            out.push(Opt::Pair(helpers::rails::sym_value(&p.key()), p.value().as_nil_node().is_some()));
        } else {
            out.push(Opt::Splat);
        }
    }
}

/// A parser `block`-type argument (a lambda literal or a call with a non-numbered block).
fn block_type(n: &Node) -> bool {
    let numbered = |p: Option<Node>| p.is_some_and(|p| p.as_numbered_parameters_node().is_some() || p.as_it_parameters_node().is_some());
    if let Some(l) = n.as_lambda_node() {
        return !numbered(l.parameters());
    }
    n.as_call_node().and_then(|c| c.block()).and_then(|b| b.as_block_node()).is_some_and(|b| !numbered(b.parameters()))
}

/// The first block parameter's name (`arg.children[0]`), or None without parameters.
fn first_param_name(b: &ruby_prism::BlockNode) -> Option<Option<String>> {
    let p = b.parameters()?;
    let bp = p.as_block_parameters_node()?;
    let ps = bp.parameters()?;
    let first = parameter_nodes(&ps).into_iter().next()?;
    let name = if let Some(r) = first.as_required_parameter_node() {
        Some(String::from_utf8_lossy(r.name().as_slice()).into_owned())
    } else if let Some(o) = first.as_optional_parameter_node() {
        Some(String::from_utf8_lossy(o.name().as_slice()).into_owned())
    } else {
        None
    };
    Some(name)
}

impl Cop for InverseOf {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send $_ {:has_many :has_one :belongs_to} _ $...)
        let Some(call) = helpers::rails::send(node) else { return };
        if !matches!(method_name(&call), "has_many" | "has_one" | "belongs_to") {
            return;
        }
        let args = send_args(&call);
        if args.is_empty() {
            return;
        }
        let arguments = &args[1..];
        let recv = call.receiver();
        // with_options_arguments: enclosing `with_options` blocks for the same receiver.
        let anc = cx.ancestors;
        let mut opts: Vec<Opt> = Vec::new();
        for a in arguments {
            options_of(a, &mut opts);
        }
        for i in (1..anc.len()).rev() {
            let Some(b) = anc[i].as_block_node() else { continue };
            if b.parameters().is_some_and(|p| p.as_numbered_parameters_node().is_some() || p.as_it_parameters_node().is_some()) {
                continue;
            }
            let Some(owner) = anc[i - 1].as_call_node() else { continue };
            if !helpers::rails::is_command(&owner, "with_options") {
                continue;
            }
            // same_context_in_with_options?
            let arg = first_param_name(&b);
            let same = match (&arg, &recv) {
                (None, None) => true,
                (Some(name), Some(r)) => {
                    let recv_name = r.as_local_variable_read_node().map(|l| String::from_utf8_lossy(l.name().as_slice()).into_owned());
                    name.is_some() && *name == recv_name
                }
                _ => false,
            };
            if !same {
                continue;
            }
            for a in send_args(&owner) {
                options_of(&a, &mut opts);
            }
        }
        let has = |k: &str| opts.iter().any(|o| matches!(o, Opt::Pair(Some(n), false) if n == k));
        if has("through") || has("polymorphic") {
            return;
        }
        let ignore_scopes = cx.cfg.get("IgnoreScopes") == Some(&Value::Bool(true));
        let scope = !ignore_scopes && arguments.iter().any(block_type);
        let mut required = has("conditions") || has("foreign_key");
        if cx.config.target_rails_version() < 5.2 {
            required |= has("as");
        }
        if !scope && !required {
            return;
        }
        if has("inverse_of") {
            return;
        }
        let nil_inverse = opts.iter().any(|o| matches!(o, Opt::Pair(Some(n), true) if n == "inverse_of"));
        if opts.iter().any(|o| matches!(o, Opt::Splat)) && !nil_inverse {
            return;
        }
        let msg = if nil_inverse {
            "You specified `inverse_of: nil`, you probably meant to use `inverse_of: false`."
        } else {
            "Specify an `:inverse_of` option."
        };
        let sel = call.message_loc().unwrap();
        cx.add(sel.start_offset(), sel.end_offset(), msg);
    }
}
