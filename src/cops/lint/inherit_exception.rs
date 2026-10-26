use crate::prelude::*;

pub const NAME: &str = "Lint/InheritException";
pub static COP: InheritException = InheritException;
pub struct InheritException;

/// `const_name == 'Exception'` (a `::Exception` counts).
fn exception_const(node: &Node) -> bool {
    match kind::of(node) {
        kind::CONSTANT_READ_NODE => node.as_constant_read_node().unwrap().name().as_slice() == b"Exception",
        kind::CONSTANT_PATH_NODE => {
            let p = node.as_constant_path_node().unwrap();
            p.parent().is_none() && p.name().is_some_and(|n| n.as_slice() == b"Exception")
        }
        _ => false,
    }
}

/// `(const {cbase nil?} _)`
fn simple_const(node: &Node) -> bool {
    node.as_constant_read_node().is_some() || node.as_constant_path_node().is_some_and(|p| p.parent().is_none())
}

fn defines_exception(node: &Node) -> bool {
    match kind::of(node) {
        kind::CLASS_NODE => exception_const(&node.as_class_node().unwrap().constant_path()),
        kind::MODULE_NODE => exception_const(&node.as_module_node().unwrap().constant_path()),
        _ => false,
    }
}

impl Cop for InheritException {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE, kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let prefer = if cx.cfg.str("EnforcedStyle") == Some("runtime_error") { "RuntimeError" } else { "StandardError" };
        let target = if let Some(class) = node.as_class_node() {
            let Some(parent) = class.superclass() else { return };
            if !exception_const(&parent) {
                if let Some(via) = inherits_exception_via(cx, &parent) {
                    let l = parent.location();
                    let msg = format!("Inherit from `{prefer}` instead of `Exception` (inherited via `{via}`).");
                    cx.add_with(l.start_offset(), l.end_offset(), msg, false);
                }
                return;
            }
            // inherit_exception_class_with_omitted_namespace?
            let cbase = parent.as_constant_path_node().is_some();
            if !cbase && let Some(stmts) = cx.ancestors.last().and_then(|a| a.as_statements_node()) {
                let start = node.location().start_offset();
                if stmts.body().iter().take_while(|s| s.location().start_offset() != start).any(|s| defines_exception(&s)) {
                    return;
                }
            }
            parent
        } else {
            let call = node.as_call_node().unwrap();
            if call.name().as_slice() != b"new" || is_csend(&call) {
                return;
            }
            let Some(recv) = call.receiver() else { return };
            let is_class = match kind::of(&recv) {
                kind::CONSTANT_READ_NODE => recv.as_constant_read_node().unwrap().name().as_slice() == b"Class",
                kind::CONSTANT_PATH_NODE => {
                    let p = recv.as_constant_path_node().unwrap();
                    p.parent().is_none() && p.name().is_some_and(|n| n.as_slice() == b"Class")
                }
                _ => false,
            };
            let args = send_args(&call);
            if !is_class || args.len() != 1 || !simple_const(&args[0]) || !exception_const(&args[0]) {
                return;
            }
            args.into_iter().next().unwrap()
        };
        let l = target.location();
        let (s, e) = (l.start_offset(), l.end_offset());
        cx.add_fix(s, e, format!("Inherit from `{prefer}` instead of `Exception`."), |c| c.replace(s, e, prefer));
    }
}

/// `inherits_exception_via`: with the project index, the first ancestor of the parent class whose
/// own superclass is an unresolved `Exception`.
fn inherits_exception_via(cx: &Cx, parent: &Node) -> Option<String> {
    use crate::project_index::ConstRef;
    let index = crate::project_index::get()?;
    if parent.as_constant_read_node().is_none() && parent.as_constant_path_node().is_none() {
        return None;
    }
    let decl = crate::project_index::resolve_const_node(index, parent, cx.ancestors)?;
    let ix = index.read();
    if !ix.is_class(decl) {
        return None;
    }
    let found = ix.ancestors(decl).into_iter().find(|&a| {
        ix.is_class(a)
            && ix.definitions(a).into_iter().any(|d| {
                matches!(ix.superclass(d), Some(ConstRef::Unresolved(n)) if n.strip_prefix("::").unwrap_or(&n) == "Exception")
            })
    })?;
    Some(ix.name(found).to_string())
}
