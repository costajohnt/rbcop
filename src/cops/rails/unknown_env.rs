use crate::prelude::*;

pub const NAME: &str = "Rails/UnknownEnv";
pub static COP: UnknownEnv = UnknownEnv;
pub struct UnknownEnv;

/// (send {(const nil? :Rails) (const (cbase) :Rails)} :env)
fn rails_env(n: &Node) -> bool {
    helpers::rails::send_node(n).is_some_and(|c| {
        method_name(&c) == "env" && send_args(&c).is_empty() && c.receiver().is_some_and(|r| helpers::parser_parent::is_top_const(&r, "Rails"))
    })
}

fn message(name: &str, envs: &[String]) -> String {
    let name = name.strip_suffix('?').unwrap_or(name);
    let dict: Vec<&str> = envs.iter().map(String::as_str).collect();
    let similar = helpers::did_you_mean::correct(name, &dict);
    if similar.is_empty() {
        format!("Unknown environment `{name}`.")
    } else {
        format!("Unknown environment `{name}`. Did you mean `{}`?", similar.join(", "))
    }
}

impl Cop for UnknownEnv {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE, kind::CASE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let envs = cx.cfg.strings("Environments");
        if let Some(case) = node.as_case_node() {
            if !case.predicate().is_some_and(|p| rails_env(&p)) {
                return;
            }
            for w in case.conditions().iter() {
                let Some(w) = w.as_when_node() else { continue };
                for c in w.conditions().iter() {
                    let Some(v) = helpers::rails::str_value(&c) else { continue };
                    if !envs.contains(&v) {
                        cx.add(c.location().start_offset(), c.location().end_offset(), message(&v, &envs));
                    }
                }
            }
            return;
        }
        let Some(call) = helpers::rails::send(node) else { return };
        let name = method_name(&call);
        // (send #rails_env? $#unknown_env_predicate?)
        if send_args(&call).is_empty() && call.receiver().is_some_and(|r| rails_env(&r)) && name.ends_with('?') {
            let mut with_local = envs.clone();
            if cx.config.target_rails_version() >= 7.1 {
                with_local.push("local".to_string());
            }
            if !with_local.iter().any(|e| *e == name[..name.len() - 1]) {
                let sel = call.message_loc().unwrap();
                cx.add(sel.start_offset(), sel.end_offset(), message(name, &envs));
            }
        }
        // {(send #rails_env? {:== :=== :!=} $(str #unknown_env_name?)) (send $(str ...) {...} #rails_env?)}
        if matches!(name, "==" | "===" | "!=") {
            let args = send_args(&call);
            let [arg] = args.as_slice() else { return };
            let Some(recv) = call.receiver() else { return };
            let target = if rails_env(&recv) && helpers::rails::str_value(arg).is_some() {
                Some(arg)
            } else if rails_env(arg) && helpers::rails::str_value(&recv).is_some() {
                Some(&recv)
            } else {
                None
            };
            if let Some(t) = target {
                let v = helpers::rails::str_value(t).unwrap();
                if !envs.contains(&v) {
                    cx.add(t.location().start_offset(), t.location().end_offset(), message(&v, &envs));
                }
            }
        }
    }
}
