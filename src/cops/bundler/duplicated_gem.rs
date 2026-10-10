use crate::prelude::*;

pub const NAME: &str = "Bundler/DuplicatedGem";
pub static COP: DuplicatedGem = DuplicatedGem;
pub struct DuplicatedGem;

struct Gem {
    name: Vec<u8>,
    start: usize,
    end: usize,
    /// `nodes[0].each_ancestor.find { !begin_type? }` when it is an if/when: the root conditional.
    root: Option<usize>,
    /// Root conditionals (start offsets) whose `branches` hold this node directly or as a child.
    within: Vec<usize>,
    /// Stand-in for parser's structural node equality (`branch.child_nodes.include?(node)` uses `==`).
    shape: String,
}

#[derive(Default)]
struct State(Vec<Gem>);

/// Start offsets of the if/unless/case whose `branches` include the branch that is the
/// StatementsNode `ancestors[i]`, plus every if up an elsif chain (IfNode#branches recurses).
fn branch_owners(ancestors: &[Node], i: usize) -> Vec<usize> {
    let mut out = Vec::new();
    let Some(mut j) = i.checked_sub(1) else { return out };
    if kind::of(&ancestors[j]) == kind::ELSE_NODE {
        let Some(k) = j.checked_sub(1) else { return out };
        j = k;
    }
    match kind::of(&ancestors[j]) {
        kind::WHEN_NODE if j > 0 => out.push(ancestors[j - 1].location().start_offset()),
        kind::IF_NODE | kind::UNLESS_NODE | kind::CASE_NODE => {
            out.push(ancestors[j].location().start_offset());
            while kind::of(&ancestors[j]) == kind::IF_NODE && j > 0 && kind::of(&ancestors[j - 1]) == kind::IF_NODE {
                j -= 1;
                out.push(ancestors[j].location().start_offset());
            }
        }
        _ => {}
    }
    out
}

impl Cop for DuplicatedGem {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if call.receiver().is_some() || method_name(&call) != "gem" {
            return;
        }
        let Some(first) = call_args(&call).into_iter().next() else { return };
        let Some(s) = first.as_string_node() else { return };
        let anc = cx.ancestors;
        let n = anc.len();
        // Parser parent skipping `begin`s: Prism's StatementsNode / ElseNode wrappers.
        let mut p = n;
        while p > 0 && matches!(kind::of(&anc[p - 1]), kind::STATEMENTS_NODE | kind::ELSE_NODE) {
            p -= 1;
        }
        // With a `do`/`{}` block, parser's `block` (not if/when) is the send's parent, and the
        // branch statement is that `block`, not this `send`.
        let has_block = call.block().is_some_and(|b| b.as_block_node().is_some());
        let root = match p.checked_sub(1).map(|i| kind::of(&anc[i])) {
            _ if has_block => None,
            Some(kind::IF_NODE | kind::UNLESS_NODE) => Some(anc[p - 1].location().start_offset()),
            Some(kind::WHEN_NODE) if p >= 2 => Some(anc[p - 2].location().start_offset()),
            _ => None,
        };
        // within_conditional?: the node is a branch (a lone statement), a statement of a
        // multi-statement branch, or the body of a block call that is a lone branch statement.
        // ponytail: other `child_nodes` shapes (arguments, parenthesized branches) are not modelled.
        let mut within = Vec::new();
        if !has_block && n >= 1 && kind::of(&anc[n - 1]) == kind::STATEMENTS_NODE {
            within = branch_owners(anc, n - 1);
            let lone = anc[n - 1].as_statements_node().unwrap().body().iter().count() == 1;
            if within.is_empty()
                && lone
                && n >= 4
                && kind::of(&anc[n - 2]) == kind::BLOCK_NODE
                && kind::of(&anc[n - 3]) == kind::CALL_NODE
                && kind::of(&anc[n - 4]) == kind::STATEMENTS_NODE
                && anc[n - 4].as_statements_node().unwrap().body().iter().count() == 1
            {
                within = branch_owners(anc, n - 4);
            }
        }
        let (start, end) = send_range(&call);
        // ponytail: strings compare by value, other arguments by source text.
        let shape: Vec<String> = send_args(&call)
            .iter()
            .map(|a| match a.as_string_node() {
                Some(s) => format!("str:{}", String::from_utf8_lossy(s.unescaped())),
                None => node_text(a).to_string(),
            })
            .collect();
        let gem = Gem { name: s.unescaped().to_vec(), start, end, root, within, shape: shape.join("\0") };
        cx.state::<State>().0.push(gem);
    }

    fn on_file(&self, cx: &mut Cx) {
        let gems = std::mem::take(&mut cx.state::<State>().0);
        let mut groups: Vec<Vec<&Gem>> = Vec::new();
        for g in &gems {
            match groups.iter_mut().find(|grp| grp[0].name == g.name) {
                Some(grp) => grp.push(g),
                None => groups.push(vec![g]),
            }
        }
        for grp in groups.iter().filter(|g| g.len() > 1) {
            let within =
                |g: &Gem, r: usize| grp.iter().any(|other| other.shape == g.shape && other.within.contains(&r));
            if grp[0].root.is_some_and(|r| grp.iter().all(|g| within(g, r))) {
                continue;
            }
            let first_line = cx.src.line_of(grp[0].start);
            for g in &grp[1..] {
                let (s, e) = helpers::range_help::first_line_to_last_column(cx.src, g.start, g.end);
                let msg = format!(
                    "Gem `{}` requirements already given on line {first_line} of the Gemfile.",
                    String::from_utf8_lossy(&g.name)
                );
                cx.add(s, e, msg);
            }
        }
    }
}
