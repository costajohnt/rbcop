use crate::helpers::const_name::const_name;
use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Lint/MissingSuper";
pub static COP: MissingSuper = MissingSuper;
pub struct MissingSuper;

const CONSTRUCTOR_MSG: &str = "Call `super` to initialize state of the parent class.";
const CALLBACK_MSG: &str = "Call `super` to invoke callback defined in the parent class.";

const CALLBACKS: &[&[u8]] = &[
    b"inherited",
    b"method_added",
    b"method_removed",
    b"method_undefined",
    b"singleton_method_added",
    b"singleton_method_removed",
    b"singleton_method_undefined",
];

struct HasSuper(bool);

impl<'pr> Visit<'pr> for HasSuper {
    fn visit_super_node(&mut self, _: &ruby_prism::SuperNode<'pr>) {
        self.0 = true;
    }
    fn visit_forwarding_super_node(&mut self, _: &ruby_prism::ForwardingSuperNode<'pr>) {
        self.0 = true;
    }
}

impl Cop for MissingSuper {
    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let def = node.as_def_node().unwrap();
        let name = def.name();
        let name = name.as_slice();
        let callback = CALLBACKS.contains(&name)
            && cx.ancestors.iter().any(|a| a.as_class_node().is_some() || a.as_singleton_class_node().is_some() || a.as_module_node().is_some());
        let initialize = def.receiver().is_none() && name == b"initialize";
        if !initialize && !callback {
            return;
        }
        let mut sup = HasSuper(false);
        sup.visit(node);
        if sup.0 {
            return;
        }
        // `STATELESS_CLASSES + cop_config.fetch('AllowedParentClasses', [])`: a String raises
        // (TypeError), a cop error.
        let allowed = |n: &Node| {
            let extra = cx.cfg.array("AllowedParentClasses");
            const_name(n).is_some_and(|c| c == "BasicObject" || c == "Object" || extra.contains(&c))
        };
        let msg = if initialize && stateful_parent(cx.ancestors, &allowed) {
            CONSTRUCTOR_MSG
        } else if callback {
            CALLBACK_MSG
        } else {
            return;
        };
        let l = node.location();
        cx.add(l.start_offset(), l.end_offset(), msg);
    }
}

/// `inside_class_with_stateful_parent?`
fn stateful_parent(ancestors: &[Node], allowed: &dyn Fn(&Node) -> bool) -> bool {
    for (i, a) in ancestors.iter().enumerate().rev() {
        if a.as_lambda_node().is_some() {
            return false;
        }
        if a.as_block_node().is_some() {
            // class_new_block: (any_block (send (const {nil? cbase} :Class) :new $_) ...)
            let Some(call) = i.checked_sub(1).and_then(|p| ancestors[p].as_call_node()) else { return false };
            let is_class = call.receiver().is_some_and(|r| {
                (r.as_constant_read_node().is_some() || r.as_constant_path_node().is_some_and(|p| p.parent().is_none()))
                    && const_name(&r).as_deref() == Some("Class")
            });
            let args = send_args(&call);
            if !is_class || call.name().as_slice() != b"new" || is_csend(&call) || args.len() != 1 {
                return false;
            }
            return !allowed(&args[0]);
        }
    }
    // No block anywhere above: the nearest class.
    let Some(i) = ancestors.iter().rposition(|a| a.as_class_node().is_some()) else { return false };
    let class = ancestors[i].as_class_node().unwrap();
    class.superclass().is_some_and(|s| !allowed(&s)) && !index_verified_stateless_ancestry(&ancestors[..=i])
}

/// `index_verified_stateless_ancestry?`: with the project index, the class' whole ancestry
/// resolves (extends aside) and no inherited ancestor defines `initialize`.
fn index_verified_stateless_ancestry(ancestors: &[Node]) -> bool {
    let Some(index) = crate::project_index::get() else { return false };
    let (class, outer) = ancestors.split_last().unwrap();
    let Some(id) = crate::project_index::identifier(class) else { return false };
    let Some(decl) = crate::project_index::resolve_const_node(index, &id, outer) else { return false };
    let ix = index.read();
    if !ix.is_class(decl) {
        return false;
    }
    let name = ix.name(decl);
    if ix.ancestors(decl).into_iter().any(|a| ix.name(a) != name && ix.member(a, "initialize()").is_some()) {
        return false;
    }
    ix.fully_resolved_ancestry(decl, true)
}
