use crate::prelude::*;
use helpers::select_by::plain_send;

pub const NAME: &str = "Style/EvalWithLocation";
pub static COP: EvalWithLocation = EvalWithLocation;
pub struct EvalWithLocation;

/// `(const {nil? cbase} :Kernel)`
fn kernel(node: &Node) -> bool {
    crate::helpers::code_length::global_const(node, &["Kernel"])
}

fn int_is(node: &Node, want: i64) -> bool {
    node.as_integer_node().is_some_and(|i| {
        let v: Result<i32, _> = i.value().try_into();
        v.is_ok_and(|v| i64::from(v) == want)
    })
}

/// `{(send #special_line_keyword? sign (int diff)) (send (int diff) sign #special_line_keyword?)}`
fn line_with_offset(node: &Node, sign: &str, diff: i64) -> bool {
    let Some(call) = plain_send(node) else { return false };
    let args = send_args(&call);
    let (Some(recv), [arg]) = (call.receiver(), args.as_slice()) else { return false };
    method_name(&call) == sign
        && ((recv.as_source_line_node().is_some() && int_is(arg, diff))
            || (int_is(&recv, diff) && arg.as_source_line_node().is_some()))
}

/// `string_first_line`: a heredoc's first body line, else the literal's first line.
fn string_first_line(cx: &Cx, code: &Node) -> usize {
    let start = if let Some(s) = code.as_string_node().filter(|_| is_heredoc(code)) {
        s.content_loc().start_offset()
    } else if let Some(s) = code.as_interpolated_string_node().filter(|_| is_heredoc(code)) {
        match s.parts().iter().next() {
            Some(p) => p.location().start_offset(),
            None => s.closing_loc().map_or(code.location().start_offset(), |c| c.start_offset()),
        }
    } else {
        code.location().start_offset()
    };
    cx.src.line_of(start)
}

fn expected_line(sign: &str, diff: i64) -> String {
    if diff == 0 { "__LINE__".to_string() } else { format!("__LINE__ {sign} {}", diff.abs()) }
}

impl Cop for EvalWithLocation {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if is_csend(&call) || !matches!(name, "eval" | "class_eval" | "module_eval" | "instance_eval") {
            return;
        }
        let eval = name == "eval";
        if eval && !call.receiver().is_none_or(|r| kernel(&r)) {
            return;
        }
        let args = send_args(&call);
        let Some(code) = args.first() else { return };
        if !matches!(kind::of(code), kind::STRING_NODE | kind::INTERPOLATED_STRING_NODE | kind::SOURCE_FILE_NODE) {
            return;
        }
        let base = if eval { 2 } else { 1 };
        let (file, line) = (args.get(base), args.get(base + 1));
        let last = args.last().unwrap();
        let code_line = string_first_line(cx, code) as i64;
        let (ns, ne) = send_range(&call);
        let node_msg =
            if eval { "Pass a binding, `__FILE__`, and `__LINE__` to `eval`.".to_string() } else { format!("Pass `__FILE__` and `__LINE__` to `{name}`.") };
        if let Some(file) = file {
            check_file(cx, name, file);
        }
        if line.is_some() {
            // check_line
            if matches!(
                kind::of(last),
                kind::LOCAL_VARIABLE_READ_NODE
                    | kind::IT_LOCAL_VARIABLE_READ_NODE
                    | kind::INSTANCE_VARIABLE_READ_NODE
                    | kind::CLASS_VARIABLE_READ_NODE
                    | kind::GLOBAL_VARIABLE_READ_NODE
            ) || plain_send(last).is_some_and(|c| method_name(&c) != "+")
            {
                return;
            }
            let diff = code_line - cx.src.line_of(last.location().start_offset()) as i64;
            let sign = if diff > 0 { "+" } else { "-" };
            if diff == 0 {
                if last.as_source_line_node().is_some() {
                    return;
                }
            } else if line_with_offset(last, sign, diff.abs()) {
                return;
            }
            let expected = expected_line(sign, diff);
            let (s, e) = (last.location().start_offset(), last.location().end_offset());
            let msg = format!("Incorrect line number for `{name}`; use `{expected}` instead of `{}`.", node_text(last));
            cx.add_fix(s, e, msg, |c| c.replace(s, e, &expected));
            return;
        }
        let diff = code_line - cx.src.line_of(last.location().start_offset()) as i64;
        let line_str = expected_line(if diff > 0 { "+" } else { "-" }, diff);
        let at = last.location().end_offset();
        if file.is_some() {
            cx.add_fix(ns, ne, node_msg, |c| c.insert_after(at, at, &format!(", {line_str}")));
        } else if eval && args.len() < 2 {
            cx.add_with(ns, ne, node_msg, false);
        } else {
            cx.add_fix(ns, ne, node_msg, |c| c.insert_after(at, at, &format!(", __FILE__, {line_str}")));
        }
    }
}

fn check_file(cx: &mut Cx, name: &str, file: &Node) {
    if file.as_source_file_node().is_some() {
        return;
    }
    let (s, e) = (file.location().start_offset(), file.location().end_offset());
    let msg = format!("Incorrect file for `{name}`; use `__FILE__` instead of `{}`.", node_text(file));
    cx.add_fix(s, e, msg, |c| c.replace(s, e, "__FILE__"));
}
