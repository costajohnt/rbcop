use crate::helpers::rake::{is_receiverless, namespaces, source_location, task_name};
use crate::prelude::*;
use std::collections::HashMap;

pub const NAME: &str = "Rake/DuplicateTask";
pub static COP: DuplicateTask = DuplicateTask;
pub struct DuplicateTask;

impl Cop for DuplicateTask {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn restrict_on_send(&self) -> &'static [&'static str] {
        &["task"]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if !is_receiverless(&call, "task") {
            return;
        }
        let Some(ns) = namespaces(&call, cx.ancestors, cx.src.target_ruby).into_iter().collect::<Option<Vec<_>>>() else {
            return;
        };
        let Some(name) = task_name(&call, cx.src.target_ruby) else { return };
        let full = ns.into_iter().rev().chain([name]).collect::<Vec<_>>().join(":");
        let (s, e) = send_range(&call);
        let current = source_location(cx, s);
        // ponytail: tasks keyed by full name, first location kept, as the gem's @tasks hash.
        let tasks = cx.state::<HashMap<String, String>>();
        match tasks.get(&full) {
            Some(previous) => {
                let msg = format!("Task `{full}` is defined at both {previous} and {current}.");
                cx.add(s, e, msg);
            }
            None => {
                tasks.insert(full, current);
            }
        }
    }
}
