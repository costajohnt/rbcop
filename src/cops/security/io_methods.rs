use crate::prelude::*;

pub const NAME: &str = "Security/IoMethods";
pub static COP: IoMethods = IoMethods;
pub struct IoMethods;

impl Cop for IoMethods {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if is_csend(&call) || !matches!(name, "read" | "binread" | "write" | "binwrite" | "foreach" | "readlines") {
            return;
        }
        let Some(receiver) = call.receiver().filter(|r| node_text(r) == "IO") else { return };
        // `argument&.str_type? && argument.value.strip.start_with?('|')` (Ruby strip: NUL, \t-\r, space)
        if let Some(arg) = send_args(&call).into_iter().next()
            && !is_parser_dstr(&arg)
            && let Some(s) = arg.as_string_node()
            && s.unescaped().iter().find(|&&b| !matches!(b, 0 | 9..=13 | b' ')) == Some(&b'|')
        {
            return;
        }
        let (start, end) = send_range(&call);
        let r = receiver.location();
        let (rs, re) = (r.start_offset(), r.end_offset());
        cx.add_fix(start, end, format!("`File.{name}` is safer than `IO.{name}`."), move |c| c.replace(rs, re, "File"));
    }
}
