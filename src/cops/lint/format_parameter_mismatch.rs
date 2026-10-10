use crate::prelude::*;
use helpers::format_string as fs;
use ruby_prism::CallNode;

pub const NAME: &str = "Lint/FormatParameterMismatch";
pub static COP: FormatParameterMismatch = FormatParameterMismatch;
pub struct FormatParameterMismatch;

/// Parser `str` / `dstr`.
fn string_type(n: &Node) -> bool {
    n.as_string_node().is_some() || n.as_interpolated_string_node().is_some()
}

fn is_const(n: &Node) -> bool {
    n.as_constant_read_node().is_some() || n.as_constant_path_node().is_some()
}

/// `format_method?(name, node)`.
fn format_method(call: &CallNode, args: &[Node], name: &str) -> bool {
    if let Some(r) = call.receiver().filter(is_const) {
        let last = match r.as_constant_read_node() {
            Some(c) => c.name().as_slice().to_vec(),
            None => r.as_constant_path_node().unwrap().name().map(|n| n.as_slice().to_vec()).unwrap_or_default(),
        };
        if last != b"Kernel" {
            return false;
        }
    }
    method_name(call) == name && args.len() > 1 && string_type(&args[0])
}

fn heredoc_first_arg(args: &[Node]) -> bool {
    args.first().is_some_and(|a| node_text(a).starts_with("<<"))
}

/// `expected_fields_count`; Err where RuboCop crashes.
fn expected_fields(n: &Node) -> Result<u64, ()> {
    let src = node_text(n);
    if fs::named_interpolation(src) {
        return Ok(1);
    }
    if let Some(m) = fs::max_digit_dollar_num(src)?
        && m != 0
    {
        return Ok(m);
    }
    Ok(fs::format_sequences(src).iter().filter(|q| !q.percent).map(|q| q.arity() as u64).sum())
}

impl Cop for FormatParameterMismatch {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if !matches!(name, "format" | "sprintf" | "%") || is_csend(&call) {
            return;
        }
        let args = send_args(&call);
        let recv = call.receiver();
        // called_on_string?
        let on_string = match &recv {
            None => args.first().is_some_and(string_type),
            Some(r) if is_const(r) => args.first().is_some_and(string_type),
            Some(r) => string_type(r),
        };
        if !on_string {
            return;
        }
        let format = format_method(&call, &args, "format") || format_method(&call, &args, "sprintf");
        let percent = name == "%" && {
            let recv_str = recv.as_ref().is_some_and(string_type);
            let Some(first) = args.first() else { return }; // RuboCop crashes on `nil.array_type?` / `nil.source`
            let p = recv_str || first.as_array_node().is_some();
            !(p && recv_str && heredoc_first_arg(&args)) && p
        };
        if !format && !percent {
            return;
        }
        let Some(sel) = call.message_loc() else { return };
        let (s, e) = (sel.start_offset(), sel.end_offset());
        let fmt_src = if format { node_text(&args[0]) } else { node_text(recv.as_ref().unwrap()) };
        if !fs::is_valid(fmt_src) {
            cx.add(
                s,
                e,
                "Format string is invalid because formatting sequence types (numbered, named or unnumbered) are mixed.",
            );
            return;
        }
        // offending_node?
        if !percent && args.iter().skip(1).any(|a| a.as_splat_node().is_some()) {
            return;
        }
        let counts = if format && !heredoc_first_arg(&args) {
            (args.len() as u64 - 1, expected_fields(&args[0]))
        } else if percent && args[0].as_array_node().is_some() {
            (args[0].as_array_node().unwrap().elements().iter().count() as u64, expected_fields(recv.as_ref().unwrap()))
        } else {
            return;
        };
        let (passed, Ok(expected)) = counts else { return };
        if expected == 0 && (is_parser_dstr(&args[0]) || args[0].as_array_node().is_some()) {
            return;
        }
        if expected != passed {
            let method = if name == "%" { "String#%" } else { name };
            cx.add(
                s,
                e,
                format!("Number of arguments ({passed}) to `{method}` doesn't match the number of fields ({expected})."),
            );
        }
    }
}
