//! RuboCop's UncommunicativeName mixin (Naming/BlockParameterName, Naming/MethodParameterName).

use crate::config::value::Num;
use crate::cop::Cx;
use crate::node_kind as kind;
use ruby_prism::Node;

fn sym(id: ruby_prism::ConstantId) -> String {
    String::from_utf8_lossy(id.as_slice()).into_owned()
}

/// rubocop-ast `Node#to_s` (s-expression) for a parameter inside an `mlhs`.
fn sexp(node: &Node, indent: usize) -> String {
    let pad = "  ".repeat(indent);
    match kind::of(node) {
        kind::MULTI_TARGET_NODE => {
            let kids: String =
                super::block_args::mlhs_children(node).iter().map(|c| format!("\n{}", sexp(c, indent + 1))).collect();
            format!("{pad}(mlhs{kids})")
        }
        kind::SPLAT_NODE => {
            let name = node.as_splat_node().unwrap().expression().and_then(|e| e.as_required_parameter_node());
            match name {
                Some(n) => format!("{pad}(restarg :{})", sym(n.name())),
                None => format!("{pad}(restarg)"),
            }
        }
        _ => match node.as_required_parameter_node() {
            Some(n) => format!("{pad}(arg :{})", sym(n.name())),
            None => format!("{pad}(arg)"),
        },
    }
}

/// `(arg.children.first.to_s, extra length for */**)` for a parser args child; `None` when the
/// first child is nil (`*`, `**`, `&`, `...`, `**nil`).
fn name_of(arg: &Node) -> Option<(String, usize)> {
    Some(match kind::of(arg) {
        kind::REQUIRED_PARAMETER_NODE => (sym(arg.as_required_parameter_node()?.name()), 0),
        kind::OPTIONAL_PARAMETER_NODE => (sym(arg.as_optional_parameter_node()?.name()), 0),
        kind::REQUIRED_KEYWORD_PARAMETER_NODE => (sym(arg.as_required_keyword_parameter_node()?.name()), 0),
        kind::OPTIONAL_KEYWORD_PARAMETER_NODE => (sym(arg.as_optional_keyword_parameter_node()?.name()), 0),
        kind::REST_PARAMETER_NODE => (sym(arg.as_rest_parameter_node()?.name()?), 1),
        kind::KEYWORD_REST_PARAMETER_NODE => (sym(arg.as_keyword_rest_parameter_node()?.name()?), 2),
        kind::BLOCK_PARAMETER_NODE => (sym(arg.as_block_parameter_node()?.name()?), 0),
        kind::BLOCK_LOCAL_VARIABLE_NODE => (sym(arg.as_block_local_variable_node()?.name()), 0),
        // children.first of an mlhs is a node: its s-expression is the "name"
        kind::MULTI_TARGET_NODE => (sexp(super::block_args::mlhs_children(arg).first()?, 0), 0),
        _ => return None,
    })
}

/// UncommunicativeName#check over the parser `args` children; `name_type` is "block parameter"
/// or "method parameter".
pub fn check(cx: &mut Cx, args: &[Node], name_type: &str) {
    // `cop_config['AllowedNames'].include?(name)`: on a String (`AllowedNames: e, id`) a
    // substring test.
    let list = |key: &str| {
        let value = cx.cfg.get(key).cloned();
        move |name: &str| match &value {
            Some(crate::config::value::Value::Str(s)) => s.contains(name),
            Some(crate::config::value::Value::Seq(items)) => {
                items.iter().any(|i| i.as_string().is_some_and(|s| s == name))
            }
            Some(crate::config::value::Value::Map(m)) => m.contains(name),
            // nil (a key set to `~`) or another scalar: no include?.
            _ => panic!("undefined method 'include?'"),
        }
    };
    let allowed = list("AllowedNames");
    let forbidden = list("ForbiddenNames");
    let allow_nums = cx.cfg.bool("AllowNamesEndingInNumbers").unwrap_or(true);
    for arg in args {
        let Some((full, extra)) = name_of(arg) else { continue };
        if full == "_" {
            continue;
        }
        let name = full.trim_start_matches('_');
        if allowed(name) {
            continue;
        }
        // range: `full.size (+1/+2)` characters from the arg's begin (blockarg: from `&`). An
        // mlhs name is an s-expression that can run past the end of the source; parser counts
        // on (one column per character), and so do the formatters (#312).
        let start = arg.location().start_offset();
        let len = full.chars().count() + extra;
        let rest = &cx.src.text[start..];
        let end = match rest.char_indices().nth(len) {
            Some((i, _)) => start + i,
            None => cx.src.text.len() + (len - rest.chars().count()),
        };
        let nchars = name.chars().count();
        // only the first offense on a range survives
        let first = if forbidden(name) {
            Some(format!("Do not use {name} as a name for a {name_type}."))
        } else if name.chars().any(char::is_uppercase) {
            Some(format!("Only use lowercase characters for {name_type}."))
        } else {
            None
        };
        let found = first.is_some();
        if let Some(msg) = first {
            cx.add(start, end, msg);
        }
        // long_enough? reads MinNameLength for each name, after those offenses (a String one
        // raises there)
        // `name.size >= min_length`, `%<min>s` in the message: a Float one as it is
        let min = cx.cfg.num("MinNameLength").unwrap_or(Num::Int(1));
        if found {
            continue;
        }
        let msg = if (nchars as f64) < min.f64() {
            let mut cap = name_type.to_string();
            cap[..1].make_ascii_uppercase();
            format!("{cap} must be at least {min} characters long.")
        } else if !allow_nums && name.chars().last().is_some_and(|c| c.is_ascii_digit()) {
            format!("Do not end {name_type} with a number.")
        } else {
            continue;
        };
        cx.add(start, end, msg);
    }
}
