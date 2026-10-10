use crate::prelude::*;

pub const NAME: &str = "Lint/DeprecatedOpenSSLConstant";
pub static COP: DeprecatedOpenSSLConstant = DeprecatedOpenSSLConstant;
pub struct DeprecatedOpenSSLConstant;

const NO_ARG_ALGORITHM: &[&str] = &["BF", "DES", "IDEA", "RC4"];

fn const_name<'a>(node: &Node<'a>) -> Option<&'a [u8]> {
    match kind::of(node) {
        kind::CONSTANT_READ_NODE => Some(node.as_constant_read_node().unwrap().name().as_slice()),
        kind::CONSTANT_PATH_NODE => node.as_constant_path_node().unwrap().name().map(|n| n.as_slice()),
        _ => None,
    }
}

/// Ruby `String#split('-')`: trailing empty fields dropped.
fn split_dash(s: &str) -> Vec<String> {
    let mut v: Vec<String> = s.split('-').map(str::to_string).collect();
    while v.last().is_some_and(|x| x.is_empty()) {
        v.pop();
    }
    v
}

impl Cop for DeprecatedOpenSSLConstant {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let method = method_name(&call);
        if is_csend(&call) || !matches!(method, "new" | "digest") {
            return;
        }
        let args = send_args(&call);
        let skip_arg = |a: &Node| {
            matches!(
                kind::of(a),
                kind::LOCAL_VARIABLE_READ_NODE
                    | kind::INSTANCE_VARIABLE_READ_NODE
                    | kind::CLASS_VARIABLE_READ_NODE
                    | kind::GLOBAL_VARIABLE_READ_NODE
                    | kind::CALL_NODE
                    | kind::CONSTANT_READ_NODE
                    | kind::CONSTANT_PATH_NODE
            )
        };
        if args.iter().any(skip_arg) {
            return;
        }
        // (const (const (const {nil? cbase} :OpenSSL) {:Cipher :Digest}) _)
        let Some(recv) = call.receiver() else { return };
        if const_name(&recv) == Some(b"Digest") {
            return;
        }
        let Some(algo) = recv.as_constant_path_node() else { return };
        let Some(class) = algo.parent() else { return };
        let Some(class_path) = class.as_constant_path_node() else { return };
        if !matches!(const_name(&class), Some(b"Cipher" | b"Digest")) {
            return;
        }
        let Some(openssl) = class_path.parent() else { return };
        if const_name(&openssl) != Some(b"OpenSSL") {
            return;
        }
        if openssl.as_constant_path_node().is_some_and(|p| p.parent().is_some()) {
            return;
        }
        let (Some(sel), Some(dot)) = (call.message_loc(), call.call_operator_loc()) else { return };
        let openssl_class = node_text(&class);
        let name = text(&algo.name_loc());
        let algorithm_name = if openssl_class == "OpenSSL::Cipher" && !NO_ARG_ALGORITHM.contains(&name) {
            // name.scan(/.{3}/).join('-')
            let chars: Vec<char> = name.chars().collect();
            chars.as_chunks::<3>().0.iter().map(|c| c.iter().collect::<String>()).collect::<Vec<_>>().join("-")
        } else {
            name.to_string()
        };
        let replacement_args = if node_text(&recv) == "OpenSSL::Cipher::Cipher" {
            let Some(first) = args.first() else { return };
            node_text(first).to_string()
        } else if openssl_class == "OpenSSL::Cipher" {
            let parts = split_dash(&algorithm_name.to_lowercase());
            let Some(first) = parts.first() else { return };
            let size_and_mode: Vec<String> = args
                .iter()
                .flat_map(|a| {
                    let s = match a.as_string_node() {
                        Some(s) => String::from_utf8_lossy(s.unescaped()).into_owned(),
                        None => node_text(a).to_string(),
                    };
                    split_dash(&s.replace([':', '\''], ""))
                })
                .map(|s| s.to_lowercase())
                .collect();
            if NO_ARG_ALGORITHM.contains(&first.to_uppercase().as_str()) && args.is_empty() {
                format!("'{first}'")
            } else {
                let mut all: Vec<String> = parts.clone();
                let empty = size_and_mode.is_empty();
                all.extend(size_and_mode);
                if empty {
                    all.push("cbc".to_string());
                }
                format!("'{}'", all.into_iter().take(3).collect::<Vec<_>>().join("-"))
            }
        } else {
            std::iter::once(format!("'{algorithm_name}'"))
                .chain(args.iter().map(|a| node_text(a).to_string()))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let (start, end) = send_range(&call);
        let selector = text(&sel);
        let msg = format!(
            "Use `{openssl_class}.{selector}({replacement_args})` instead of `{}`.",
            cx.src.slice(start, end)
        );
        let (dc, nm) = (algo.delimiter_loc(), algo.name_loc());
        let replacement = format!("{selector}({replacement_args})");
        cx.add_fix(start, end, msg, |c| {
            c.remove(dc.start_offset(), dc.end_offset());
            c.remove(nm.start_offset(), nm.end_offset());
            c.replace(dot.end_offset(), end, &replacement);
        });
    }
}
