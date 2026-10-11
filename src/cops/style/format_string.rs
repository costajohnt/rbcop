use crate::prelude::*;
use helpers::select_by::plain_send;

pub const NAME: &str = "Style/FormatString";
pub static COP: FormatString = FormatString;
pub struct FormatString;

const AUTOCORRECTABLE_METHODS: &[&str] = &["to_d", "to_f", "to_h", "to_i", "to_r", "to_s", "to_sym"];

fn is_str(node: &Node) -> bool {
    node.as_string_node().is_some() || node.as_interpolated_string_node().is_some()
}

fn display(style: &str) -> &str {
    if style == "percent" { "String#%" } else { style }
}

/// FormatString#format_single_parameter
fn format_single_parameter(arg: &Node) -> String {
    if let Some(s) = arg.as_splat_node() {
        // `*` alone (anonymous splat) has no child: RuboCop would crash; keep the source.
        return s.expression().map_or_else(|| node_text(arg).to_string(), |e| format_single_parameter(&e));
    }
    let source = node_text(arg);
    if arg.as_hash_node().is_some() || arg.as_keyword_hash_node().is_some() {
        return format!("{{ {source} }}");
    }
    let parens = helpers::node_types::is_assignment(arg)
        || matches!(
            arg,
            Node::IfNode { .. } | Node::UnlessNode { .. } | Node::AndNode { .. } | Node::OrNode { .. } | Node::RangeNode { .. }
        )
        || arg.as_call_node().is_some_and(|c| {
            // `parenthesized?` is `loc.end == ')'`: `x[:k]` (Prism opening `[`) is not.
            plain_send(arg).is_some()
                && helpers::node_types::is_operator_method(method_name(&c))
                && c.closing_loc().is_none_or(|l| l.as_slice() != b")")
        });
    if parens { format!("({source})") } else { source.to_string() }
}

impl Cop for FormatString {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) {
            return;
        }
        let name = method_name(&call);
        let args = send_args(&call);
        let recv = call.receiver();
        let detected = match name {
            "format" | "sprintf" if recv.is_none() && args.len() >= 2 => name,
            "%" if recv.as_ref().is_some_and(is_str) => "percent",
            "%" if recv.is_some()
                && args.len() == 1
                && matches!(args[0], Node::ArrayNode { .. } | Node::HashNode { .. } | Node::KeywordHashNode { .. }) =>
            {
                "percent"
            }
            _ => return,
        };
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("format").to_string();
        if detected == style {
            return;
        }
        let Some(sel) = call.message_loc() else { return };
        let msg = format!("Favor `{}` over `{}`.", display(&style), display(detected));
        // variable_argument?: (send {str dstr} :% #autocorrectable?)
        let variable = name == "%"
            && recv.as_ref().is_some_and(is_str)
            && args.len() == 1
            && (args[0].as_local_variable_read_node().is_some()
                || (plain_send(&args[0]).is_some()
                    && !AUTOCORRECTABLE_METHODS.contains(&method_name(&args[0].as_call_node().unwrap()))));
        let (s, e) = send_range(&call);
        let replacement: Option<String> = if variable {
            None
        } else if name == "%" {
            let rhs = &args[0];
            let parts = match rhs {
                Node::ArrayNode { .. } => rhs.as_array_node().unwrap().elements().iter().map(|n| node_text(&n).to_string()).collect(),
                Node::HashNode { .. } => rhs.as_hash_node().unwrap().elements().iter().map(|n| node_text(&n).to_string()).collect(),
                Node::KeywordHashNode { .. } => {
                    rhs.as_keyword_hash_node().unwrap().elements().iter().map(|n| node_text(&n).to_string()).collect()
                }
                _ => vec![node_text(rhs).to_string()],
            };
            Some(format!("{style}({}, {})", node_text(&recv.unwrap()), parts.join(", ")))
        } else if style == "percent" {
            let format = node_text(&args[0]);
            let params = &args[1..];
            let rhs = if params.len() == 1 {
                format_single_parameter(&params[0])
            } else {
                format!("[{}]", params.iter().map(node_text).collect::<Vec<_>>().join(", "))
            };
            Some(format!("{format} % {rhs}"))
        } else {
            None
        };
        let selector_only = name != "%" && style != "percent";
        let (ss, se) = (sel.start_offset(), sel.end_offset());
        cx.add_fix(ss, se, msg, |c| {
            if let Some(r) = &replacement {
                c.replace(s, e, r);
            } else if selector_only && !variable {
                c.replace(ss, se, &style);
            }
        });
    }
}
