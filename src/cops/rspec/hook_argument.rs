use crate::prelude::*;

pub const NAME: &str = "RSpec/HookArgument";
pub static COP: HookArgument = HookArgument;
pub struct HookArgument;

impl Cop for HookArgument {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (any_block $(send _ #Hooks.all (sym ${:each :example})) ...) or (any_block $(send _ #Hooks.all) ...)
        let Some(call) = helpers::rspec::any_block_call(node) else { return };
        // NodePattern checks the send's arity before its children: a send with two or more
        // arguments matches neither pattern and never asks Hooks.
        let args = send_args(&call);
        if is_csend(&call) || args.len() > 1 {
            return;
        }
        let l = helpers::rspec::lang(cx);
        if !l.hook(method_name(&call)) {
            return;
        }
        let scope = match &args[..] {
            [] => None,
            [a] if helpers::rspec::is_sym(cx, a) => match helpers::rails::sym_value(a).as_deref() {
                Some(s @ ("each" | "example")) => Some(s.to_string()),
                _ => return,
            },
            _ => return,
        };
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("implicit").to_string();
        let implicit = style == "implicit";
        let sel = call.message_loc().unwrap();
        let (_, send_end) = send_range(&call);
        // LocationHelp.arguments_with_whitespace: from the selector's end to the send's end
        let (ws, we) = (sel.end_offset(), send_end);
        let replacement = if implicit { String::new() } else { format!("(:{style})") };
        match scope {
            Some(scope) if scope == style => {}
            Some(scope) => {
                let msg = if implicit {
                    format!("Omit the default `:{scope}` argument for RSpec hooks.")
                } else {
                    format!("Use `:{style}` for RSpec hooks.")
                };
                let (s, e) = send_range(&call);
                cx.add_fix(s, e, msg, |c| c.replace(ws, we, &replacement));
            }
            None if implicit => {}
            None => {
                let msg = format!("Use `:{style}` for RSpec hooks.");
                cx.add_fix(sel.start_offset(), sel.end_offset(), msg, |c| c.replace(ws, we, &replacement));
            }
        }
    }
}
