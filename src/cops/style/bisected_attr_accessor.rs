use crate::cop::Corrector;
use crate::prelude::*;
use helpers::select_by::plain_send;
use ruby_prism::CallNode;

pub const NAME: &str = "Style/BisectedAttrAccessor";
pub static COP: BisectedAttrAccessor = BisectedAttrAccessor;
pub struct BisectedAttrAccessor;

/// BisectedAttrAccessor::Macro
struct Macro<'pr> {
    call: CallNode<'pr>,
    /// `arguments.to_h { [source, node] }`: unique sources, first-seen order, last node wins.
    attrs: Vec<(&'pr str, (usize, usize))>,
    visibility: &'pr str,
}

impl Macro<'_> {
    fn reader(&self) -> bool {
        matches!(method_name(&self.call), "attr_reader" | "attr")
    }
}

/// `(send nil? {:private :protected :public})`
fn visibility_block<'pr>(n: &Node<'pr>) -> Option<&'pr str> {
    let c = n.as_call_node()?;
    let name = method_name(&c);
    (c.receiver().is_none() && c.arguments().is_none() && c.block().is_none() && matches!(name, "private" | "protected" | "public"))
        .then_some(name)
}

impl Cop for BisectedAttrAccessor {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE, kind::MODULE_NODE, kind::SINGLETON_CLASS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let body = match node {
            Node::ClassNode { .. } => node.as_class_node().unwrap().body(),
            Node::ModuleNode { .. } => node.as_module_node().unwrap().body(),
            _ => node.as_singleton_class_node().unwrap().body(),
        };
        // ponytail: a body with rescue/ensure (parser `rescue`) is skipped.
        let Some(stmts) = body.and_then(|b| b.as_statements_node()) else { return };
        let stmts: Vec<Node> = stmts.body().iter().collect();
        // find_macros: the body itself when it is one send, else the begin's child sends.
        let mut macros: Vec<Macro> = Vec::new();
        for (i, stmt) in stmts.iter().enumerate() {
            let Some(call) = plain_send(stmt) else { continue };
            if !matches!(method_name(&call), "attr_reader" | "attr_writer" | "attr") {
                continue;
            }
            let mut attrs: Vec<(&str, (usize, usize))> = Vec::new();
            for a in send_args(&call) {
                let r = (a.location().start_offset(), a.location().end_offset());
                match attrs.iter_mut().find(|(s, _)| *s == node_text(&a)) {
                    Some(e) => e.1 = r,
                    None => attrs.push((node_text(&a), r)),
                }
            }
            let visibility = stmts[..i].iter().rev().find_map(visibility_block).unwrap_or("public");
            macros.push(Macro { call, attrs, visibility });
        }
        let mut groups: Vec<&str> = Vec::new();
        for m in &macros {
            if !groups.contains(&m.visibility) {
                groups.push(m.visibility);
            }
        }
        let mut rewrites: Vec<(usize, Vec<&str>)> = Vec::new(); // (macro index, bisected names)
        for vis in groups {
            let group: Vec<usize> = (0..macros.len()).filter(|&i| macros[i].visibility == vis).collect();
            let names = |reader: bool| -> Vec<&str> {
                group.iter().filter(|&&i| macros[i].reader() == reader).flat_map(|&i| macros[i].attrs.iter().map(|a| a.0)).collect()
            };
            let writers = names(false);
            let mut bisected: Vec<&str> = Vec::new();
            for n in names(true) {
                if writers.contains(&n) && !bisected.contains(&n) {
                    bisected.push(n);
                }
            }
            if bisected.is_empty() {
                continue;
            }
            for &i in &group {
                let hits: Vec<(&str, (usize, usize))> =
                    bisected.iter().filter_map(|b| macros[i].attrs.iter().find(|a| a.0 == *b).copied()).collect();
                if hits.is_empty() {
                    continue;
                }
                for (name, (s, e)) in &hits {
                    cx.add_with(*s, *e, format!("Combine both accessors into `attr_accessor {name}`."), false);
                }
                rewrites.push((i, hits.iter().map(|h| h.0).collect()));
            }
        }
        if !cx.fixes.autocorrect || rewrites.is_empty() {
            return;
        }
        // after_class: corrections without an offense (`correct(range)`).
        let src = cx.src;
        for (i, bisected) in rewrites {
            let m = &macros[i];
            let (ns, ne) = send_range(&m.call);
            let (rs, re) = helpers::whole_lines::range_by_whole_lines(src.text, ns, ne, true);
            let line_start = src.text[..ns].rfind('\n').map_or(0, |p| p + 1);
            let indent = " ".repeat(src.text[line_start..ns].chars().count());
            let rest: Vec<&str> = m.attrs.iter().map(|a| a.0).filter(|a| !bisected.contains(a)).collect();
            let mut c = Corrector::new(cx.facts.text.clone());
            if m.reader() {
                let accessor = format!("attr_accessor {}\n", bisected.join(", "));
                if rest.is_empty() {
                    c.replace(rs, re, &format!("{indent}{accessor}"));
                } else {
                    c.insert_before(ns, ne, &accessor);
                    c.replace(ns, ne, &format!("{indent}attr_reader {}", rest.join(", ")));
                }
            } else if rest.is_empty() {
                c.remove(rs, re);
            } else {
                c.replace(ns, ne, &format!("attr_writer {}", rest.join(", ")));
            }
            let merged = cx.fixes.corrector.get_or_insert_with(|| Corrector::new(cx.facts.text.clone()));
            // ponytail: a clobbering merge is a cop error in RuboCop; the edit is dropped here too.
            let _ = merged.merge(&c);
        }
    }
}
