use crate::prelude::*;

pub const NAME: &str = "Rails/DuplicateAssociation";
pub static COP: DuplicateAssociation = DuplicateAssociation;
pub struct DuplicateAssociation;

impl Cop for DuplicateAssociation {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let class = node.as_class_node().unwrap();
        if !class.superclass().is_some_and(|s| helpers::rails::is_active_record(&s)) {
            return;
        }
        // (send nil? {:belongs_to :has_one :has_many :has_and_belongs_to_many} ({sym str} $_) $...)
        let assocs: Vec<(ruby_prism::CallNode, String)> = helpers::rails::class_send_nodes(class.body())
            .into_iter()
            .filter_map(|c| {
                if c.receiver().is_some()
                    || !matches!(method_name(&c), "belongs_to" | "has_one" | "has_many" | "has_and_belongs_to_many")
                {
                    return None;
                }
                let name = send_args(&c).first().and_then(helpers::rails::str_or_sym)?;
                Some((c, name))
            })
            .collect();
        let src = cx.src;
        let mut groups: Vec<(String, Vec<usize>, bool)> = Vec::new();
        for (i, (_, name)) in assocs.iter().enumerate() {
            match groups.iter_mut().find(|g| g.0 == *name && !g.2) {
                Some(g) => g.1.push(i),
                None => groups.push((name.clone(), vec![i], false)),
            }
        }
        let mut class_groups: Vec<(String, Vec<usize>, bool)> = Vec::new();
        for (i, (c, _)) in assocs.iter().enumerate() {
            if method_name(c) == "belongs_to" {
                continue;
            }
            let args = send_args(c);
            let [_, h] = args.as_slice() else { continue };
            // (hash (pair (sym :class_name) $_))
            let Some(elems) = helpers::rails::hash_elements(h) else { continue };
            let [only] = elems.as_slice() else { continue };
            let Some(p) = only.as_assoc_node() else { continue };
            if helpers::rails::sym_value(&p.key()).as_deref() != Some("class_name") {
                continue;
            }
            let key = helpers::rails::pair_value_source(&p).to_string();
            match class_groups.iter_mut().find(|g| g.0 == key) {
                Some(g) => g.1.push(i),
                None => class_groups.push((key, vec![i], true)),
            }
        }
        for (name, idx, is_class) in groups.into_iter().chain(class_groups) {
            if idx.len() < 2 {
                continue;
            }
            let msg = if is_class {
                format!("Association `class_name: {name}` is defined multiple times. Don't repeat associations.")
            } else {
                format!("Association `{name}` is defined multiple times. Don't repeat associations.")
            };
            let last = &assocs[*idx.last().unwrap()].0;
            let (ls, le) = send_range(last);
            let last_src = src.slice(ls, le).to_string();
            for (k, &i) in idx.iter().enumerate() {
                let (s, e) = send_range(&assocs[i].0);
                let whole = helpers::whole_lines::range_by_whole_lines(src.text, s, e, true);
                let last_src = last_src.clone();
                cx.add_fix(s, e, msg.clone(), |c| {
                    if k == 0 {
                        c.replace(s, e, &last_src);
                    } else {
                        c.remove(whole.0, whole.1);
                    }
                });
            }
        }
    }
}
