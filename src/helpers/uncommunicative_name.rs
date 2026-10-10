//! RuboCop's UncommunicativeName mixin (Naming/BlockParameterName, Naming/MethodParameterName).

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
    let allowed = cx.cfg.strings("AllowedNames");
    let forbidden = cx.cfg.strings("ForbiddenNames");
    let allow_nums = cx.cfg.bool("AllowNamesEndingInNumbers").unwrap_or(true);
    let min = cx.cfg.int("MinNameLength").unwrap_or(1).max(0) as usize;
    for arg in args {
        let Some((full, extra)) = name_of(arg) else { continue };
        if full == "_" {
            continue;
        }
        let name = full.trim_start_matches('_');
        if allowed.iter().any(|a| a == name) {
            continue;
        }
        // range: `full.size (+1/+2)` characters from the arg's begin (blockarg: from `&`)
        let start = arg.location().start_offset();
        let len = full.chars().count() + extra;
        let Some(end) = cx.src.text[start..].char_indices().map(|(i, _)| start + i).chain([cx.src.text.len()]).nth(len)
        else {
            continue;
        };
        let nchars = name.chars().count();
        // only the first offense on a range survives
        let msg = if forbidden.iter().any(|f| f == name) {
            format!("Do not use {name} as a name for a {name_type}.")
        } else if name.chars().any(char::is_uppercase) {
            format!("Only use lowercase characters for {name_type}.")
        } else if nchars < min {
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
