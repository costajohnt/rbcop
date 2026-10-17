use crate::prelude::*;

pub const NAME: &str = "Rails/DynamicFindBy";
pub static COP: DynamicFindBy = DynamicFindBy;
pub struct DynamicFindBy;

impl Cop for DynamicFindBy {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        // METHOD_PATTERN /^find_by_(.+?)(!)?$/
        let Some(rest) = name.strip_prefix("find_by_") else { return };
        let (cols, bang) = match rest.strip_suffix('!') {
            Some(c) if !c.is_empty() => (c, true),
            _ => (rest, false),
        };
        if cols.is_empty() {
            return;
        }
        if call.receiver().is_none() && !helpers::rails::inherit_active_record_base(cx.ancestors) {
            return;
        }
        let listed = |key: &str| cx.cfg.get(key).is_some_and(|v| !matches!(v, Value::Null | Value::Bool(false)));
        // `cop_config[key].include?`: a substring test on a String.
        if listed("AllowedMethods") && cx.cfg.includes("AllowedMethods", name) {
            return;
        }
        if listed("AllowedReceivers")
            && call.receiver().is_some_and(|r| cx.cfg.includes("AllowedReceivers", node_text(&r)))
        {
            return;
        }
        if listed("Whitelist") && cx.cfg.includes("Whitelist", name) {
            return;
        }
        let keywords: Vec<String> = cols.split("_and_").map(|k| format!("{k}: ")).collect();
        let args = send_args(&call);
        if keywords.len() != args.len()
            || args.iter().any(|a| helpers::rails::hash_elements(a).is_some() || a.as_splat_node().is_some())
        {
            return;
        }
        let static_name = if bang { "find_by!" } else { "find_by" };
        let sel = call.message_loc().unwrap();
        let (ss, se) = (sel.start_offset(), sel.end_offset());
        let inserts: Vec<(usize, usize)> = args.iter().map(|a| (a.location().start_offset(), a.location().end_offset())).collect();
        let (s, e) = send_range(&call);
        let msg = format!("Use `{static_name}` instead of dynamic `{name}`.");
        cx.add_fix(s, e, msg, |c| {
            c.replace(ss, se, static_name);
            for (k, (a, b)) in keywords.iter().zip(inserts) {
                c.insert_before(a, b, k);
            }
        });
    }
}
