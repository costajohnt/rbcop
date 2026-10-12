use crate::prelude::*;

pub const NAME: &str = "Rails/RootPublicPath";
pub static COP: RootPublicPath = RootPublicPath;
pub struct RootPublicPath;

impl Cop for RootPublicPath {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send (send $(const {nil? cbase} :Rails) :root) :join (str $#public_path?) $...)
        let Some(call) = helpers::rails::send(node) else { return };
        if method_name(&call) != "join" {
            return;
        }
        let Some(root) = call.receiver().and_then(|r| helpers::rails::send_node(&r)) else { return };
        if method_name(&root) != "root" || !send_args(&root).is_empty() {
            return;
        }
        let Some(rails) = root.receiver().filter(|r| helpers::rails::is_const(r, "Rails")) else { return };
        let args = send_args(&call);
        let Some(path) = args.first().and_then(helpers::rails::str_value) else { return };
        let first = match path.strip_prefix("public") {
            Some(r) if r.is_empty() => String::new(),
            Some(r) if r.starts_with('/') => r[1..].to_string(),
            _ => return,
        };
        let mut parts: Vec<String> = args[1..].iter().map(|a| node_text(a).to_string()).collect();
        if !first.is_empty() {
            parts.insert(0, format!("'{first}'"));
        }
        let mut replacement = format!("{}.public_path", node_text(&rails));
        if !parts.is_empty() {
            replacement += &format!(".join({})", parts.join(", "));
        }
        let (s, e) = send_range(&call);
        cx.add_fix(s, e, "Use `Rails.public_path`.", |c| c.replace(s, e, &replacement));
    }
}
