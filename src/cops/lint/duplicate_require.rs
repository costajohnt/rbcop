use crate::prelude::*;

pub const NAME: &str = "Lint/DuplicateRequire";
pub static COP: DuplicateRequire = DuplicateRequire;
pub struct DuplicateRequire;

/// `@required[node.parent]`: (parent kind, start, end) -> "method + argument shape" seen so far.
#[derive(Default)]
struct State(Vec<((usize, usize, usize), String)>);

impl Cop for DuplicateRequire {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if !matches!(name, "require" | "require_relative") || is_csend(&call) {
            return;
        }
        // (send {nil? (const _ :Kernel)} %REQUIRE_METHODS _)
        let kernel = |r: &Node| match kind::of(r) {
            kind::CONSTANT_READ_NODE => r.as_constant_read_node().unwrap().name().as_slice() == b"Kernel",
            kind::CONSTANT_PATH_NODE => r.as_constant_path_node().unwrap().name().is_some_and(|n| n.as_slice() == b"Kernel"),
            _ => false,
        };
        if call.receiver().is_some_and(|r| !kernel(&r)) {
            return;
        }
        let args = send_args(&call);
        let [arg] = args.as_slice() else { return };
        // "#{method_name}#{first_argument}": the argument's s-expression, so 'a' and "a" match.
        let arg_key = match arg.as_string_node() {
            Some(s) => format!("str:{}", String::from_utf8_lossy(s.unescaped())),
            None => {
                let l = arg.location();
                helpers::node_equality::shape(cx.src, l.start_offset(), l.end_offset()).join("\0")
            }
        };
        let key = format!("{name}{arg_key}");
        let parent = cx.parent().map_or((0, 0, 0), |p| (kind::of(p), p.location().start_offset(), p.location().end_offset()));
        let state = cx.state::<State>();
        if !state.0.iter().any(|(p, k)| *p == parent && *k == key) {
            state.0.push((parent, key));
            return;
        }
        let (start, end) = send_range(&call);
        let (rs, re) = helpers::whole_lines::range_by_whole_lines(cx.src.text, start, end, true);
        cx.add_fix(start, end, format!("Duplicate `{name}` detected."), |c| c.remove(rs, re));
    }
}
