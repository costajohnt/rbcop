use crate::prelude::*;

pub const NAME: &str = "RSpec/BeEmpty";
pub static COP: BeEmpty = BeEmpty;
pub struct BeEmpty;

impl Cop for BeEmpty {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || call.receiver().is_some() || helpers::rspec::block_of(&call).is_some() {
            return;
        }
        // ${(send nil? :match_array (array)) (send nil? :contain_exactly)}
        let args = send_args(&call);
        match method_name(&call) {
            "match_array" => match &args[..] {
                [a] if a.as_array_node().is_some_and(|a| a.elements().iter().next().is_none()) => {}
                _ => return,
            },
            "contain_exactly" if args.is_empty() => {}
            _ => return,
        }
        // node.parent: (send (send nil? :expect _) #Runners.all $node _?)
        let Some(parent) = helpers::parser_parent::parser_parent(cx.ancestors).and_then(|p| p.as_call_node()) else {
            return;
        };
        if is_csend(&parent) || !helpers::rspec::is_runner(method_name(&parent)) {
            return;
        }
        let pargs = send_args(&parent);
        if pargs.is_empty() || pargs.len() > 2 || pargs[0].location().start_offset() != node.location().start_offset() {
            return;
        }
        let Some(expect) = parent.receiver().and_then(|r| helpers::rspec::nil_send(&r, "expect")) else { return };
        if send_args(&expect).len() != 1 {
            return;
        }
        let (s, e) = send_range(&call);
        cx.add_fix(s, e, "Use `be_empty` matchers for checking an empty array.", |c| c.replace(s, e, "be_empty"));
    }
}
