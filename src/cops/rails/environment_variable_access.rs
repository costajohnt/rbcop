use crate::prelude::*;

pub const NAME: &str = "Rails/EnvironmentVariableAccess";
pub static COP: EnvironmentVariableAccess = EnvironmentVariableAccess;
pub struct EnvironmentVariableAccess;

impl Cop for EnvironmentVariableAccess {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CONSTANT_READ_NODE, kind::CONSTANT_PATH_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // ^(send (const {cbase nil?} :ENV) ...): the parser parent is a send on ENV.
        let anc = cx.ancestors;
        let Some(i) = helpers::rails::parent_index(anc) else { return };
        let p = &anc[i];
        let env = |r: Option<Node>| r.is_some_and(|r| helpers::parser_parent::is_top_const(&r, "ENV"));
        let method: Option<String> = match p {
            Node::CallNode { .. } => {
                let c = p.as_call_node().unwrap();
                (!is_csend(&c) && env(c.receiver())).then(|| method_name(&c).to_string())
            }
            // op-assign / mlhs forms hold a parser send on their receiver.
            Node::IndexOperatorWriteNode { .. } => {
                let n = p.as_index_operator_write_node().unwrap();
                (!n.is_safe_navigation() && env(n.receiver())).then(|| "[]".into())
            }
            Node::IndexOrWriteNode { .. } => {
                let n = p.as_index_or_write_node().unwrap();
                (!n.is_safe_navigation() && env(n.receiver())).then(|| "[]".into())
            }
            Node::IndexAndWriteNode { .. } => {
                let n = p.as_index_and_write_node().unwrap();
                (!n.is_safe_navigation() && env(n.receiver())).then(|| "[]".into())
            }
            Node::IndexTargetNode { .. } => {
                let n = p.as_index_target_node().unwrap();
                env(Some(n.receiver())).then(|| "[]=".into())
            }
            Node::CallOperatorWriteNode { .. } => {
                let n = p.as_call_operator_write_node().unwrap();
                (!n.is_safe_navigation() && env(n.receiver())).then(|| String::from_utf8_lossy(n.read_name().as_slice()).into_owned())
            }
            Node::CallOrWriteNode { .. } => {
                let n = p.as_call_or_write_node().unwrap();
                (!n.is_safe_navigation() && env(n.receiver())).then(|| String::from_utf8_lossy(n.read_name().as_slice()).into_owned())
            }
            Node::CallAndWriteNode { .. } => {
                let n = p.as_call_and_write_node().unwrap();
                (!n.is_safe_navigation() && env(n.receiver())).then(|| String::from_utf8_lossy(n.read_name().as_slice()).into_owned())
            }
            Node::CallTargetNode { .. } => {
                let n = p.as_call_target_node().unwrap();
                (!n.is_safe_navigation() && env(Some(n.receiver()))).then(|| String::from_utf8_lossy(n.name().as_slice()).into_owned())
            }
            _ => None,
        };
        let Some(method) = method else { return };
        let (s, e) = (node.location().start_offset(), node.location().end_offset());
        if method != "[]=" && cx.cfg.get("AllowReads") != Some(&Value::Bool(true)) {
            cx.add(s, e, "Do not read from `ENV` directly post initialization.");
        }
        if method == "[]=" && cx.cfg.get("AllowWrites") != Some(&Value::Bool(true)) {
            cx.add(s, e, "Do not write to `ENV` directly post initialization.");
        }
    }
}
