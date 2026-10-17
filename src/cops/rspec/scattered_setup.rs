use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "RSpec/ScatteredSetup";
pub static COP: ScatteredSetup = ScatteredSetup;
pub struct ScatteredSetup;

/// Ranges of hooks that are the lone statement of a `def self.x` or of a `def` directly in
/// `class << self` (Hook#inside_class_method?).
fn class_method_bodies(node: &Node) -> Vec<(usize, usize)> {
    struct V(Vec<(usize, usize)>);
    fn lone(def: &ruby_prism::DefNode, out: &mut Vec<(usize, usize)>) {
        if let Some(b) = helpers::rspec::parser_body(def.body())
            && b.as_statements_node().is_none()
        {
            out.push(helpers::rspec::node_range(&b));
        }
    }
    impl<'pr> Visit<'pr> for V {
        fn visit_singleton_class_node(&mut self, n: &ruby_prism::SingletonClassNode<'pr>) {
            // a def directly in `class << self` (alone, or among other statements)
            for s in helpers::rspec::body_children(n.body()) {
                if let Some(d) = s.as_def_node() {
                    lone(&d, &mut self.0);
                }
            }
            ruby_prism::visit_singleton_class_node(self, n);
        }
        fn visit_def_node(&mut self, n: &ruby_prism::DefNode<'pr>) {
            if n.receiver().is_some() {
                lone(n, &mut self.0);
            }
            ruby_prism::visit_def_node(self, n);
        }
    }
    let mut v = V(Vec::new());
    v.visit(node);
    v.0
}

impl Cop for ScatteredSetup {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let l = helpers::rspec::lang(cx);
        if helpers::rspec::plain_block_call(node).is_none() || !helpers::rspec::example_group(&l, node) {
            return;
        }
        let src = cx.src;
        let class_methods = class_method_bodies(node);
        let mut items = Vec::new();
        for hook in helpers::rspec::find_all_in_scope(&l, node, helpers::rspec::hook) {
            let range = helpers::rspec::node_range(&hook);
            if class_methods.contains(&range) {
                continue;
            }
            let call = hook.as_call_node().unwrap();
            let name = method_name(&call);
            let args = send_args(&call);
            // knowable_scope?: no argument, a symbol or a hash
            if let Some(first) = args.first()
                && !(first.as_symbol_node().is_some() || first.as_hash_node().is_some() || first.as_keyword_hash_node().is_some())
            {
                continue;
            }
            if name == "around" {
                continue;
            }
            let scope = helpers::rspec::hook_scope(&call).unwrap_or("nil");
            // Hook#metadata: the arguments after a valid scope, merged into one hash
            let mut meta: Option<Vec<(String, String)>> = None;
            if helpers::rspec::plain_block_call(&hook).is_some() {
                let skip = usize::from(args.first().is_some_and(|a| {
                    helpers::rspec::is_sym(cx, a)
                        && helpers::rails::sym_value(a).is_some_and(|v| helpers::rspec::is_hook_scope(&v))
                }));
                for m in &args[skip..] {
                    let map = meta.get_or_insert_with(Vec::new);
                    let mut put = |k: String, v: String| match map.iter_mut().find(|(mk, _)| *mk == k) {
                        Some(e) => e.1 = v,
                        None => map.push((k, v)),
                    };
                    if m.as_symbol_node().is_some() {
                        put(helpers::rspec::skey(src, Some(m)), "true".into());
                    } else if let Some(pairs) = helpers::rails::hash_elements(m) {
                        for p in pairs.iter().filter_map(|p| p.as_assoc_node()) {
                            let v = p.value();
                            let val = if v.as_true_node().is_some() { "true".into() } else { helpers::rspec::skey(src, Some(&v)) };
                            put(helpers::rspec::skey(src, Some(&p.key())), val);
                        }
                    } else {
                        // `meta.pairs` raises: the group is not checked
                        return;
                    }
                }
            }
            let meta = meta.map_or("nil".to_string(), |mut m| {
                m.sort();
                format!("{m:?}")
            });
            items.push((format!("{name}|{scope}|{meta}"), hook));
        }
        for group in helpers::rspec::repeated_groups(items) {
            let lines: Vec<usize> = group.iter().map(|h| src.line_of(h.location().start_offset())).collect();
            let first = &group[0];
            let first_call = first.as_call_node().unwrap();
            let first_body = helpers::rspec::block_body(&first_call).map(|b| body_range(&b));
            let hook_name = method_name(&first_call).to_string();
            for occ in &group {
                let me = src.line_of(occ.location().start_offset());
                let others: Vec<usize> = lines.iter().copied().filter(|&l| l != me).collect();
                let lines_msg = if others.len() == 1 {
                    format!("line {}", others[0])
                } else {
                    format!("lines {}", others.iter().map(|l| l.to_string()).collect::<Vec<_>>().join(", "))
                };
                let msg = format!(
                    "Do not define multiple `{hook_name}` hooks in the same example group (also defined on {lines_msg})."
                );
                // autocorrect: move this body into the first hook (not for one equal to the first)
                let same = helpers::rspec::skey(src, Some(first)) == helpers::rspec::skey(src, Some(occ));
                let fix = match first_body {
                    Some((fbs, fbe)) if !same => {
                        let occ_call = occ.as_call_node().unwrap();
                        let body = helpers::rspec::block_body(&occ_call).map(|b| {
                            let (bs, _) = body_range(&b);
                            let end = helpers::rspec::final_end_location(occ).map_or(bs, |r| r.0);
                            src.slice(bs, end.max(bs)).to_string()
                        });
                        let (os, oe) = helpers::rspec::node_range(occ);
                        Some(((fbs, fbe), format!("\n{}", body.unwrap_or_default()), helpers::whole_lines::range_by_whole_lines(src.text, os, oe, true)))
                    }
                    _ => None,
                };
                let (s, e) = helpers::rspec::node_range(occ);
                cx.add_fix(s, e, msg, |c| {
                    if let Some(((fs, fe), text, (rs, re))) = &fix {
                        c.insert_after(*fs, *fe, text);
                        c.remove(*rs, *re);
                    }
                });
            }
        }
    }
}
