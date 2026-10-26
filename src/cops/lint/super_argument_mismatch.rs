use crate::prelude::*;
use crate::project_index;

pub const NAME: &str = "Lint/SuperArgumentMismatch";
pub static COP: SuperArgumentMismatch = SuperArgumentMismatch;
pub struct SuperArgumentMismatch;

const CONTEXT_CHANGING_BLOCKS: &[&str] = &[
    "define_method",
    "define_singleton_method",
    "class_eval",
    "class_exec",
    "module_eval",
    "module_exec",
    "instance_eval",
    "instance_exec",
    "refine",
];

const BUILTIN_ROOTS: &[&str] = &["Object", "Kernel", "BasicObject"];

/// `enclosing_instance_method`: the index of the `def` the `super` belongs to.
fn enclosing_instance_method(ancestors: &[Node]) -> Option<usize> {
    for (i, a) in ancestors.iter().enumerate().rev() {
        match a {
            Node::DefNode { .. } => return a.as_def_node().unwrap().receiver().is_none().then_some(i),
            Node::SingletonClassNode { .. } => return None,
            Node::BlockNode { .. } => {
                let name = i.checked_sub(1).and_then(|p| ancestors[p].as_call_node()).map_or("super", |c| method_name(&c));
                if CONTEXT_CHANGING_BLOCKS.contains(&name) {
                    return None;
                }
            }
            _ => {}
        }
    }
    None
}

/// Only with `AllCops/UseProjectIndex`: `super` whose arguments do not fit the signature of the
/// method it reaches in the class' resolved ancestry.
impl Cop for SuperArgumentMismatch {
    fn kinds(&self) -> &'static [usize] {
        &[kind::SUPER_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(index) = project_index::get() else { return };
        let sup = node.as_super_node().unwrap();
        let anc = cx.ancestors;
        let Some(d) = enclosing_instance_method(anc) else { return };
        let def = anc[d].as_def_node().unwrap();
        let def_name = String::from_utf8_lossy(def.name().as_slice()).into_owned();
        // enclosing_class_declaration
        let Some(c) = anc[..d].iter().rposition(|a| {
            matches!(
                a,
                Node::ClassNode { .. }
                    | Node::ModuleNode { .. }
                    | Node::SingletonClassNode { .. }
                    | Node::BlockNode { .. }
                    | Node::LambdaNode { .. }
            )
        }) else {
            return;
        };
        if anc[c].as_class_node().is_none() {
            return;
        }
        let class_line = cx.src.line_of(anc[c].location().start_offset()) as u32;
        let current = cx.src.absolute_path();
        let nesting = project_index::lexical_nesting(&anc[..d], anc[d].location().start_offset());
        let msg = {
            let ix = index.read();
            let Some(decl) = ix.lookup(&nesting.join("::")) else { return };
            if !ix.is_namespace(decl) {
                return;
            }
            let declared_at = ix.definitions(decl).into_iter().any(|def| {
                matches!(ix.location(def), Some((Some(path), line))
                    if line == class_line && project_index::same_file(&path, std::path::Path::new(&current)))
            });
            if !declared_at || !ix.fully_resolved_ancestry(decl, true) {
                return;
            }
            // super_method_shape
            let ancestors = ix.ancestors(decl);
            let name = ix.name(decl);
            let Some(position) = ancestors.iter().position(|&a| ix.name(a) == name) else { return };
            let member = format!("{def_name}()");
            let mut found = None;
            for &a in &ancestors[position + 1..] {
                if BUILTIN_ROOTS.contains(&ix.name(a)) {
                    return;
                }
                let Some(m) = ix.member(a, &member) else { continue };
                found = Some((ix.member_shape(m), a));
                break;
            }
            let Some((Some((min, max, keywords)), parent)) = found else { return };
            let args: Vec<Node> = sup.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default();
            let Some(given) = project_index::positional_argument_count(&args, keywords) else { return };
            if project_index::arity_satisfied(given, min, max) {
                return;
            }
            format!(
                "Wrong number of arguments to `super` (given {given}, expected {} for `{}#{def_name}`).",
                project_index::expected_range(min, max),
                ix.name(parent)
            )
        };
        let kw = sup.keyword_loc();
        cx.add(kw.start_offset(), kw.end_offset(), msg);
    }
}
