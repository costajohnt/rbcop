use crate::prelude::*;

pub const NAME: &str = "Lint/RaiseException";
pub static COP: RaiseException = RaiseException;
pub struct RaiseException;

/// `(const ${cbase nil?} :Exception)`: Some(has_cbase).
fn exception_const(node: &Node) -> Option<bool> {
    if let Some(c) = node.as_constant_read_node() {
        return (c.name().as_slice() == b"Exception").then_some(false);
    }
    let p = node.as_constant_path_node()?;
    (p.parent().is_none() && p.name()?.as_slice() == b"Exception").then_some(true)
}

impl Cop for RaiseException {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if call.receiver().is_some() || !matches!(method_name(&call), "raise" | "fail") {
            return;
        }
        let args = send_args(&call);
        let Some(first) = args.first() else { return };
        // (send nil? {:raise :fail} $(const ${cbase nil?} :Exception) ...)
        // (send nil? {:raise :fail} (send $(const ${cbase nil?} :Exception) :new ...))
        let hit = exception_const(first).map(|cbase| (first.location(), cbase)).or_else(|| {
            let new = first.as_call_node().filter(|n| {
                args.len() == 1
                    && !is_csend(n)
                    && method_name(n) == "new"
                    && !n.block().is_some_and(|b| b.as_block_node().is_some())
            })?;
            let recv = new.receiver()?;
            let cbase = exception_const(&recv)?;
            Some((recv.location(), cbase))
        });
        let Some((loc, cbase)) = hit else { return };
        if !cbase {
            // implicit_namespace?: some enclosing `module` is an allowed namespace.
            // `(cop_config['AllowedImplicitNamespaces'] || []).include?`: a substring test on a String.
            let implicit = cx.ancestors.iter().any(|a| {
                a.as_module_node()
                    .is_some_and(|m| cx.cfg.includes_or_empty("AllowedImplicitNamespaces", node_text(&m.constant_path())))
            });
            if implicit {
                return;
            }
        }
        let replacement = if cbase { "::StandardError" } else { "StandardError" };
        cx.add_fix(loc.start_offset(), loc.end_offset(), "Use `StandardError` over `Exception`.", |c| {
            c.replace(loc.start_offset(), loc.end_offset(), replacement);
        });
    }
}
