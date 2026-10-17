use crate::prelude::*;

pub const NAME: &str = "Rails/EnumSyntax";
pub static COP: EnumSyntax = EnumSyntax;
pub struct EnumSyntax;

const OPTIONS: &[&str] = &["prefix", "suffix", "scopes", "default", "instance_methods"];

/// A pair whose key's source is `_prefix`, `_suffix`, ... (parser key source: a label's colon excluded).
fn option_key(p: &ruby_prism::AssocNode) -> bool {
    let k = p.key();
    let src = key_source(&k);
    src.strip_prefix('_').is_some_and(|o| OPTIONS.contains(&o))
}

fn key_source<'pr>(k: &Node<'pr>) -> &'pr str {
    if k.as_symbol_node().is_some() {
        let (s, e) = helpers::rails::sym_range(k);
        let full = node_text(k);
        &full[..e - s]
    } else {
        node_text(k)
    }
}

fn enum_name_value(key: &Node) -> String {
    helpers::rails::str_or_sym(key).unwrap_or_else(|| node_text(key).to_string())
}

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
            c if (c as u32) < 0x20 || c as u32 == 0x7f => out.push_str(&format!("\\x{:02X}", c as u32)),
            c if c.is_ascii() => out.push(c),
            c if (c as u32) <= 0xFFFF => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push_str(&format!("\\u{{{:X}}}", c as u32)),
        }
    }
    out.push('"');
    out.replace("#{", "\\#{").replace("#$", "\\#$").replace("#@", "\\#@")
}

fn enum_name(k: &Node) -> String {
    if let Some(v) = helpers::rails::str_value(k) {
        return dump(&v);
    }
    if let Some(s) = k.as_symbol_node().filter(|_| helpers::rails::sym_value(k).is_some()) {
        return helpers::ruby_string::symbol_inspect(s.unescaped());
    }
    node_text(k).to_string()
}

impl Cop for EnumSyntax {
    fn correctable(&self) -> bool {
        true
    }

    fn ruby_range(&self) -> (f64, f64) {
        (3.0, f64::MAX)
    }

    fn min_rails(&self) -> f64 {
        7.0
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
        let (s, e) = send_range(&call);
        // check_and_correct_keyword_args: (send nil? :enum (hash $...))
        if let [hash] = args.as_slice()
            && let Some(elems) = helpers::rails::hash_elements(hash)
        {
            // multiple_enum_definitions?: two or more non-option keys.
            let keys: Vec<String> = helpers::rails::hash_pairs(hash).iter().map(|p| key_source(&p.key()).trim_start_matches('_').to_string()).collect();
            let multiple = keys.iter().filter(|k| !OPTIONS.contains(&k.as_str())).count() >= 2;
            let rest: String = {
                let opts: Vec<String> = elems[1..]
                    .iter()
                    .filter_map(|el| el.as_assoc_node())
                    .map(|p| {
                        let ks = key_source(&p.key());
                        let name = ks.strip_prefix('_').unwrap_or(ks);
                        format!("{name}: {}", node_text(&p.value()))
                    })
                    .collect();
                if opts.is_empty() { String::new() } else { format!(", {}", opts.join(", ")) }
            };
            for el in &elems {
                // A kwsplat has no `key`: the cop raises there.
                let Some(p) = el.as_assoc_node() else { break };
                if option_key(&p) {
                    continue;
                }
                let v = p.value();
                let (vs, ve) = (v.location().start_offset(), v.location().end_offset());
                let msg = format!(
                    "Enum defined with keyword arguments in `{}` enum declaration. Use positional arguments instead.",
                    enum_name_value(&p.key())
                );
                let preferred = format!("enum {}, {}{rest}", enum_name(&p.key()), node_text(&v));
                cx.add_fix(vs, ve, msg, |c| {
                    if !multiple {
                        c.replace(s, e, &preferred);
                    }
                });
            }
        }
        // check_enum_options: (send nil? :enum $_ ${array hash} $_)
        if let [key, values, options] = args.as_slice()
            && (values.as_array_node().is_some() || helpers::rails::hash_elements(values).is_some())
        {
            for p in helpers::rails::hash_pairs(options) {
                if !option_key(&p) {
                    continue;
                }
                let k = p.key();
                let (ks, ke) = helpers::rails::sym_range(&k);
                let fixed = key_source(&k).trim_start_matches('_').to_string();
                let fixed = match key_source(&k).strip_prefix('_') {
                    Some(f) => f.to_string(),
                    None => fixed,
                };
                let msg = format!(
                    "Enum defined with deprecated options in `{}` enum declaration. Remove the `_` prefix.",
                    enum_name_value(key)
                );
                cx.add_fix(ks, ke, msg, |c| c.replace(ks, ke, &fixed));
            }
        }
    }
}
