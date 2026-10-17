use crate::prelude::*;

pub const NAME: &str = "Rails/EnumHash";
pub static COP: EnumHash = EnumHash;
pub struct EnumHash;

/// Ruby's String#dump.
fn dump(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\x0c' => out.push_str("\\f"),
            '\x0b' => out.push_str("\\v"),
            '\x08' => out.push_str("\\b"),
            '\x07' => out.push_str("\\a"),
            '\x1b' => out.push_str("\\e"),
            '#' => out.push('#'),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => out.push_str(&format!("\\x{:02X}", c as u32)),
            c if c.is_ascii() => out.push(c),
            c if (c as u32) <= 0xFFFF => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push_str(&format!("\\u{{{:X}}}", c as u32)),
        }
    }
    out.push('"');
    // `#{`, `#$`, `#@` are escaped by dump too.
    out.replace("#{", "\\#{").replace("#$", "\\#$").replace("#@", "\\#@")
}

fn build_hash(array: &ruby_prism::ArrayNode) -> String {
    let items: Vec<String> = array
        .elements()
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let src = if let Some(v) = helpers::rails::str_value(&e) {
                dump(&v)
            } else if let Some(s) = e.as_symbol_node().filter(|_| helpers::rails::sym_value(&e).is_some()) {
                helpers::ruby_string::symbol_inspect(s.unescaped())
            } else {
                node_text(&e).to_string()
            };
            format!("{src} => {i}")
        })
        .collect();
    format!("{{{}}}", items.join(", "))
}

fn enum_name(key: &Node) -> String {
    helpers::rails::str_or_sym(key).unwrap_or_else(|| node_text(key).to_string())
}

fn offense(cx: &mut Cx, key: &Node, array: &Node) {
    let a = array.as_array_node().unwrap();
    let (s, e) = (array.location().start_offset(), array.location().end_offset());
    let replacement = build_hash(&a);
    let msg = format!("Enum defined as an array found in `{}` enum declaration. Use hash syntax instead.", enum_name(key));
    cx.add_fix(s, e, msg, |c| c.replace(s, e, &replacement));
}

impl Cop for EnumHash {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        if !helpers::rails::is_command(&call, "enum") {
            return;
        }
        let args = send_args(&call);
        // target_rails_version >= 7.0 && (send nil? :enum $_ ${array} ...)
        if cx.config.target_rails_version() >= 7.0 && args.len() >= 2 && args[1].as_array_node().is_some() {
            offense(cx, &args[0], &args[1]);
        }
        // (send nil? :enum (hash $...)): each (pair $_ $array)
        if let [hash] = args.as_slice()
            && helpers::rails::hash_elements(hash).is_some()
        {
            for p in helpers::rails::hash_pairs(hash) {
                if p.value().as_array_node().is_some() {
                    offense(cx, &p.key(), &p.value());
                }
            }
        }
    }
}
