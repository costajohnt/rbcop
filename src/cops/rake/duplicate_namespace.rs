use crate::helpers::rake::{is_receiverless, namespaces, source_location};
use crate::prelude::*;
use std::collections::HashMap;

pub const NAME: &str = "Rake/DuplicateNamespace";
pub static COP: DuplicateNamespace = DuplicateNamespace;
pub struct DuplicateNamespace;

impl Cop for DuplicateNamespace {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn restrict_on_send(&self) -> &'static [&'static str] {
        &["namespace"]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if !is_receiverless(&call, "namespace") {
            return;
        }
        // A namespace's own block counts: `namespace :a do` is `a`, a block-less `namespace :a` is
        // only the names around it.
        let Some(ns) = namespaces(&call, cx.ancestors, cx.src.target_ruby).into_iter().collect::<Option<Vec<_>>>() else {
            return;
        };
        let full = ns.into_iter().rev().collect::<Vec<_>>().join(":");
        let (s, e) = send_range(&call);
        let current = source_location(cx, s);
        let seen = cx.state::<HashMap<String, String>>();
        match seen.get(&full) {
            Some(previous) => {
                let msg = format!("Namespace `{full}` is defined at both {previous} and {current}.");
                cx.add(s, e, msg);
            }
            None => {
                seen.insert(full, current);
            }
        }
    }
}
