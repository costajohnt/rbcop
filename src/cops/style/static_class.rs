use crate::prelude::*;
use helpers::visibility_help::node_visibility;

pub const NAME: &str = "Style/StaticClass";
pub static COP: StaticClass = StaticClass;
pub struct StaticClass;

/// `class_elements`: the statements of a body (none when empty).
fn elements<'pr>(body: Option<Node<'pr>>) -> Vec<Node<'pr>> {
    match body {
        None => Vec::new(),
        Some(b) => match b.as_statements_node() {
            Some(s) => s.body().iter().collect(),
            None => vec![b],
        },
    }
}

/// `equals_asgn?`: lvasgn ivasgn cvasgn gvasgn casgn masgn
fn equals_asgn(node: &Node) -> bool {
    matches!(
        kind::of(node),
        kind::LOCAL_VARIABLE_WRITE_NODE
            | kind::INSTANCE_VARIABLE_WRITE_NODE
            | kind::CLASS_VARIABLE_WRITE_NODE
            | kind::GLOBAL_VARIABLE_WRITE_NODE
            | kind::CONSTANT_WRITE_NODE
            | kind::CONSTANT_PATH_WRITE_NODE
            | kind::MULTI_WRITE_NODE
    )
}

/// `node_visibility` of `elems[i]` within a class-like body.
fn visibility(elems: &[Node], i: usize, parent: &Node) -> &'static str {
    if elems.len() > 1 { node_visibility(&elems[i], Some(parent), elems, i) } else { node_visibility(&elems[i], Some(parent), &[], 0) }
}

impl Cop for StaticClass {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let class = node.as_class_node().unwrap();
        if class.superclass().is_some() {
            return;
        }
        let elems = elements(class.body());
        if elems.is_empty() {
            return;
        }
        let convertible = (0..elems.len()).all(|i| {
            let n = &elems[i];
            (n.as_def_node().is_some_and(|d| d.receiver().is_some()) && visibility(&elems, i, node) == "public")
                || sclass_convertible(n)
                || equals_asgn(n)
                || n.as_call_node().is_some_and(|c| {
                    !is_csend(&c) && method_name(&c) == "extend" && c.block().is_none_or(|b| b.as_block_node().is_none())
                })
        });
        if !convertible || subclassed_in_project(cx, &class) {
            return;
        }
        let kw = class.class_keyword_loc();
        let name = class.constant_path().location();
        let (name_start, name_end) = (name.start_offset(), name.end_offset());
        let mut removals = Vec::new();
        for n in &elems {
            if let Some(d) = n.as_def_node()
                && let Some(r) = d.receiver()
            {
                removals.push((r.location().start_offset(), d.name_loc().start_offset()));
            } else if let Some(s) = n.as_singleton_class_node() {
                removals.push((s.class_keyword_loc().start_offset(), s.expression().location().end_offset()));
                let e = s.end_keyword_loc();
                removals.push((e.start_offset(), e.end_offset()));
            }
        }
        let loc = node.location();
        cx.add_fix(loc.start_offset(), loc.end_offset(), "Prefer modules to classes with only class methods.", |c| {
            c.replace(kw.start_offset(), kw.end_offset(), "module");
            c.insert_after(name_start, name_end, "\nmodule_function\n");
            for &(s, e) in &removals {
                c.remove(s, e);
            }
        });
    }
}

fn sclass_convertible(node: &Node) -> bool {
    let Some(s) = node.as_singleton_class_node() else { return false };
    let elems = elements(s.body());
    (0..elems.len()).all(|i| {
        visibility(&elems, i, node) == "public"
            && (elems[i].as_def_node().is_some_and(|d| d.receiver().is_none()) || equals_asgn(&elems[i]))
    })
}

/// `subclassed_in_project?`: with the project index, a class elsewhere inherits from this one.
fn subclassed_in_project(cx: &Cx, class: &ruby_prism::ClassNode) -> bool {
    let Some(index) = crate::project_index::get() else { return false };
    let Some(decl) = crate::project_index::resolve_const_node(index, &class.constant_path(), cx.ancestors) else {
        return false;
    };
    let ix = index.read();
    let name = ix.name(decl);
    ix.is_class(decl) && ix.descendants(decl).into_iter().any(|d| ix.name(d) != name)
}
