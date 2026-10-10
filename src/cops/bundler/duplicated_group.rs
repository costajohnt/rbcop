use crate::prelude::*;

pub const NAME: &str = "Bundler/DuplicatedGroup";
pub static COP: DuplicatedGroup = DuplicatedGroup;
pub struct DuplicatedGroup;

struct Group {
    key: String,
    name: String,
    start: usize,
    end: usize,
}

#[derive(Default)]
struct State(Vec<Group>);

/// `argument.respond_to?(:value) ? argument.value.to_s : argument.source` (BasicLiteralNode / DstrNode).
/// ponytail: numeric literals use their source rather than Ruby's normalised `to_s`.
fn value_or_source(node: &Node) -> String {
    if let Some(s) = node.as_string_node() {
        return String::from_utf8_lossy(s.unescaped()).into_owned();
    }
    if let Some(s) = node.as_symbol_node() {
        return String::from_utf8_lossy(s.unescaped()).into_owned();
    }
    if let Some(d) = node.as_interpolated_string_node() {
        return d
            .parts()
            .iter()
            .map(|p| match p.as_embedded_variable_node() {
                Some(v) => node_text(&v.variable()).to_string(),
                None => value_or_source(&p),
            })
            .collect();
    }
    node_text(node).to_string()
}

impl Cop for DuplicatedGroup {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if call.receiver().is_some() || method_name(&call) != "group" {
            return;
        }
        // find_source_key: nearest enclosing `source`/`git`/`platforms`/`path` block (parser `block`,
        // not numblock/itblock; in parser the block also encloses the call's own arguments).
        let source_key = cx.ancestors.iter().rev().find_map(|a| {
            let c = a.as_call_node()?;
            let params = c.block()?.as_block_node()?.parameters();
            if params.is_some_and(|p| p.as_block_parameters_node().is_none()) {
                return None;
            }
            let name = method_name(&c);
            matches!(name, "source" | "git" | "platforms" | "path")
                .then(|| format!("{name}{}", call_args(&c).first().map_or("", |a| node_text(a))))
        });
        let args = send_args(&call);
        let mut attrs: Vec<String> = args
            .iter()
            .map(|a| {
                let pairs = match (a.as_hash_node(), a.as_keyword_hash_node()) {
                    (Some(h), _) => Some(h.elements()),
                    (_, Some(h)) => Some(h.elements()),
                    _ => None,
                };
                match pairs {
                    Some(elements) => {
                        let mut p: Vec<&str> =
                            elements.iter().filter(|e| e.as_assoc_node().is_some()).map(|e| node_text(&e)).collect();
                        p.sort();
                        p.join(", ")
                    }
                    None => value_or_source(a),
                }
            })
            .collect();
        attrs.sort();
        let key = format!("{}{}", source_key.unwrap_or_default(), attrs.concat());
        let name = args.iter().map(|a| node_text(a)).collect::<Vec<_>>().join(", ");
        let (start, end) = send_range(&call);
        cx.state::<State>().0.push(Group { key, name, start, end });
    }

    fn on_file(&self, cx: &mut Cx) {
        let groups = std::mem::take(&mut cx.state::<State>().0);
        for (i, g) in groups.iter().enumerate() {
            let Some(first) = groups[..i].iter().find(|f| f.key == g.key) else { continue };
            let first_line = cx.src.line_of(first.start);
            let (s, e) = helpers::range_help::first_line_to_last_column(cx.src, g.start, g.end);
            let msg = format!("Gem group `{}` already defined on line {first_line} of the Gemfile.", g.name);
            cx.add(s, e, msg);
        }
    }
}
