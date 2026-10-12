use crate::prelude::*;

pub const NAME: &str = "Rails/LinkToBlank";
pub static COP: LinkToBlank = LinkToBlank;
pub struct LinkToBlank;

fn key_is(p: &ruby_prism::AssocNode, name: &str) -> bool {
    let k = p.key();
    helpers::rails::sym_value(&k).as_deref() == Some(name) || helpers::rails::str_value(&k).as_deref() == Some(name)
}

fn blank_target(p: &ruby_prism::AssocNode) -> bool {
    key_is(p, "target") && helpers::rails::str_or_sym(&p.value()).as_deref() == Some("_blank")
}

fn includes_noopener(p: &ruby_prism::AssocNode) -> bool {
    key_is(p, "rel")
        && helpers::rails::str_or_sym(&p.value()).is_some_and(|v| {
            v.split(|c: char| c.is_ascii_whitespace() || c == '\0').any(|w| w == "noopener" || w == "noreferrer")
        })
}

impl Cop for LinkToBlank {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        if !matches!(method_name(&call), "link_to" | "link_to_if" | "link_to_unless") {
            return;
        }
        // node.each_child_node(:hash): the receiver and arguments that are hashes.
        let args = send_args(&call);
        let mut opts: Vec<Node> = Vec::new();
        if let Some(r) = call.receiver().filter(|r| helpers::rails::hash_elements(r).is_some()) {
            opts.push(r);
        }
        opts.extend(send_args(&call).into_iter().filter(|n| helpers::rails::hash_elements(n).is_some()));
        // The first `rel: "..."` (string) pair across all option hashes.
        let rel_node = opts.iter().find_map(|h| {
            helpers::rails::hash_pairs(h)
                .into_iter()
                .find(|p| key_is(p, "rel") && helpers::rails::str_value(&p.value()).is_some())
        });
        let rel = rel_node.map(|p| {
            let v = p.value();
            let (s, e) = (v.location().start_offset() + 1, v.location().end_offset() - 1);
            (s, e, format!("{} noopener", helpers::rails::str_value(&v).unwrap()))
        });
        for h in &opts {
            let pairs = helpers::rails::hash_pairs(h);
            let Some(blank) = pairs.iter().find(|p| blank_target(p)) else { continue };
            if pairs.iter().any(includes_noopener) {
                continue;
            }
            let (s, e) = (blank.location().start_offset(), blank.location().end_offset());
            // add_rel: after the last pair of a hash last argument, else after the last argument.
            let quote = node_text(&blank.value()).chars().next().unwrap_or('\'');
            let close = if quote == ':' { String::new() } else { quote.to_string() };
            let new_rel = format!(", rel: {quote}noopener{close}");
            let loc = |n: &Node| (n.location().start_offset(), n.location().end_offset());
            let last = args.last().map(|l| {
                if helpers::rails::hash_elements(l).is_some() {
                    helpers::rails::hash_pairs(l).last().map(|p| loc(&p.as_node()))
                } else {
                    Some(loc(l))
                }
            });
            let rel = rel.clone();
            if rel.is_none() && !matches!(last, Some(Some(_))) {
                // `pairs.last` is nil: the corrector raises.
                continue;
            }
            cx.add_fix(s, e, "Specify a `:rel` option containing noopener.", |c| match &rel {
                Some((a, b, text)) => c.replace(*a, *b, text),
                None => {
                    let (a, b) = last.flatten().unwrap();
                    c.insert_after(a, b, &new_rel);
                }
            });
        }
    }
}
