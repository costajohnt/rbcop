use crate::prelude::*;

pub const NAME: &str = "Lint/DeprecatedClassMethods";
pub static COP: DeprecatedClassMethods = DeprecatedClassMethods;
pub struct DeprecatedClassMethods;

/// `(const {cbase nil?} :Name)`: the constant's name.
fn top_const<'a>(node: &Node<'a>) -> Option<&'a [u8]> {
    match kind::of(node) {
        kind::CONSTANT_READ_NODE => Some(node.as_constant_read_node().unwrap().name().as_slice()),
        kind::CONSTANT_PATH_NODE => {
            let p = node.as_constant_path_node().unwrap();
            if p.parent().is_some() {
                return None;
            }
            p.name().map(|n| n.as_slice())
        }
        _ => None,
    }
}

impl Cop for DeprecatedClassMethods {
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
        let recv_const = recv.as_ref().and_then(top_const);
        let matched = match name {
            "clone" | "dup" | "freeze" => recv_const == Some(b"ENV") && args.is_empty(),
            "exists?" => matches!(recv_const, Some(b"File" | b"Dir")) && args.len() == 1,
            "gethostbyaddr" | "gethostbyname" => recv_const == Some(b"Socket"),
            "attr" => {
                recv.is_none()
                    && args.len() == 2
                    && matches!(kind::of(&args[1]), kind::TRUE_NODE | kind::FALSE_NODE)
            }
            "iterator?" => recv.is_none() && args.is_empty(),
            _ => false,
        };
        let Some(sel) = call.message_loc().filter(|_| matched) else { return };
        let (start, end) = send_range(&call);
        // receiver short_name: the receiver is one of the matched constants here.
        let socket = recv_const == Some(b"Socket");
        let dir_env_file = matches!(recv_const, Some(b"Dir" | b"ENV" | b"File"));
        let range = if socket || dir_env_file {
            (start, sel.end_offset())
        } else if name == "attr" {
            (start, end)
        } else {
            (sel.start_offset(), sel.end_offset())
        };
        let preferred = |m: &str| match m {
            "clone" | "dup" => Some("to_h"),
            "exists?" => Some("exist?"),
            "gethostbyaddr" => Some("Addrinfo#getnameinfo"),
            "gethostbyname" => Some("Addrinfo.getaddrinfo"),
            "iterator?" => Some("block_given?"),
            _ => None,
        };
        let prefer = if name == "attr" {
            let m = if node_text(&args[1]) == "true" { "attr_accessor" } else { "attr_reader" };
            format!("{m} {}", node_text(&args[0]))
        } else if dir_env_file {
            match preferred(name) {
                Some(p) => format!("{}.{p}", node_text(recv.as_ref().unwrap())),
                None => "ENV".to_string(),
            }
        } else {
            preferred(name).unwrap_or("").to_string()
        };
        let msg = format!("`{}` is deprecated in favor of `{prefer}`.", cx.src.slice(range.0, range.1));
        cx.add_fix(range.0, range.1, msg, |c| {
            if socket {
                return;
            }
            if name == "freeze" {
                c.replace(start, end, "ENV");
            } else {
                c.replace(range.0, range.1, &prefer);
            }
        });
    }
}
