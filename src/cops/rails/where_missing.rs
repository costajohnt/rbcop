use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Rails/WhereMissing";
pub static COP: WhereMissing = WhereMissing;
pub struct WhereMissing;

/// (pair (sym _) (hash (pair (sym :id) (nil)))): the key's value.
fn missing_relationship(p: &ruby_prism::AssocNode) -> Option<String> {
    let k = helpers::rails::sym_value(&p.key())?;
    let v = p.value();
    let pairs = helpers::rails::hash_elements(&v)?;
    let [only] = pairs.as_slice() else { return None };
    let inner = only.as_assoc_node()?;
    (v.as_hash_node().is_some()
        && helpers::rails::sym_value(&inner.key()).as_deref() == Some("id")
        && inner.value().as_nil_node().is_some())
    .then_some(k)
}

/// A `where` found below the root, with whether its root_receiver is the root.
struct Found<'pr> {
    call: ruby_prism::CallNode<'pr>,
    key: String,
    key_range: (usize, usize),
    same_root: bool,
}

/// def_node_search :where_node_and_argument over the root, pre-order, tracking whether the path
/// from each node up to the root is made of plain sends other than `or`/`and` (root_receiver).
struct Search<'pr> {
    ok: Vec<bool>,
    cur_ok: bool,
    found: Vec<Found<'pr>>,
}

impl<'pr> Search<'pr> {
    fn check(&mut self, c: &ruby_prism::CallNode<'pr>, same_root: bool) {
        // $(send ... :where (hash <(pair $(sym _) (hash (pair (sym :id) (nil))))...>))
        if is_csend(c) || method_name(c) != "where" || c.block().is_some_and(|b| b.as_block_node().is_some()) {
            return;
        }
        let args = send_args(c);
        let [h] = args.as_slice() else { return };
        if h.as_hash_node().is_none() && h.as_keyword_hash_node().is_none() {
            return;
        }
        for p in helpers::rails::hash_pairs(h) {
            if let Some(key) = missing_relationship(&p) {
                let k = p.key();
                self.found.push(Found {
                    call: c.as_node().as_call_node().unwrap(),
                    key,
                    key_range: helpers::rails::sym_range(&k),
                    same_root,
                });
                break;
            }
        }
    }
}

impl<'pr> Visit<'pr> for Search<'pr> {
    fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
        let here = *self.ok.last().unwrap_or(&false);
        self.cur_ok = here;
        // A plain send other than or/and passes the path on to its receiver and arguments
        // (its block's contents are cut off in visit_call_node).
        let pass = here && node.as_call_node().is_some_and(|c| !is_csend(&c) && !matches!(method_name(&c), "or" | "and"));
        self.ok.push(pass);
    }
    fn visit_branch_node_leave(&mut self) {
        self.ok.pop();
    }
    fn visit_call_node(&mut self, c: &ruby_prism::CallNode<'pr>) {
        let here = self.cur_ok;
        self.check(c, here);
        if let Some(r) = c.receiver() {
            self.visit(&r);
        }
        if let Some(a) = c.arguments() {
            for x in a.arguments().iter() {
                self.visit(&x);
            }
        }
        if let Some(b) = c.block() {
            self.ok.push(false);
            self.visit(&b);
            self.ok.pop();
        }
    }
}

impl Cop for WhereMissing {
    fn correctable(&self) -> bool {
        true
    }

    fn min_rails(&self) -> f64 {
        6.1
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        let name = method_name(&call);
        if !matches!(name, "left_joins" | "left_outer_joins") {
            return;
        }
        let args = send_args(&call);
        let Some(assoc) = args.first().and_then(helpers::rails::sym_value) else { return };
        // root_receiver: climb parent sends (not or/and).
        let anc = cx.ancestors;
        let mut root_idx: Option<usize> = None;
        let mut cur = anc.len();
        loop {
            let Some(p) = helpers::parser_parent::parser_parent_index(anc, cur) else { break };
            let ok = helpers::rails::send(&anc[p]).is_some_and(|c| !matches!(method_name(&c), "or" | "and"));
            if !ok {
                break;
            }
            root_idx = Some(p);
            cur = p;
        }
        let root: Node = match root_idx {
            Some(i) => anc[i].as_call_node().unwrap().as_node(),
            None => call.as_node().as_call_node().unwrap().as_node(),
        };
        let mut search = Search { ok: vec![true], cur_ok: true, found: Vec::new() };
        search.visit(&root);
        let Some(found) = search.found.into_iter().find(|f| {
            f.same_root && (f.key == assoc || f.key == format!("{assoc}s"))
        }) else {
            return;
        };
        let where_node = found.call;
        let sel = call.message_loc().unwrap();
        let (s, e) = (sel.start_offset(), send_range(&call).1);
        let msg = format!(
            "Use `where.missing(:{assoc})` instead of `{name}(:{assoc}).where({}: {{ id: nil }})`.",
            found.key
        );
        let src = cx.src;
        let wargs = send_args(&where_node);
        let hash = &wargs[0];
        let elems = helpers::rails::hash_elements(hash).unwrap();
        let mut edits: Vec<(usize, usize)> = Vec::new();
        if elems.len() > 1 {
            // replace_where_method: every missing-relationship pair goes, with its separator.
            for (i, el) in elems.iter().enumerate() {
                let Some(p) = el.as_assoc_node() else { continue };
                if missing_relationship(&p).is_none() {
                    continue;
                }
                let (ps, pe) = (el.location().start_offset(), el.location().end_offset());
                match elems.get(i + 1) {
                    Some(next) => edits.push((ps, next.location().start_offset())),
                    None => edits.push((elems[i - 1].location().end_offset(), pe)),
                }
            }
        } else {
            // remove_where_method
            let Some(close) = where_node.closing_loc() else { return };
            let ws = where_node.message_loc().unwrap().start_offset();
            let mut range = (ws, close.end_offset());
            let multiline = src.line_of(node.location().start_offset()) != src.line_of(send_range(&call).1);
            if multiline && src.line_of(sel.start_offset()) != src.line_of(ws) {
                range = helpers::whole_lines::range_by_whole_lines(src.text, range.0, range.1, true);
            } else if where_node.receiver().is_some() {
                let d = where_node.call_operator_loc().unwrap();
                edits.push((d.start_offset(), d.end_offset()));
            } else if let Some(d) = call.call_operator_loc() {
                edits.push((d.start_offset(), d.end_offset()));
            }
            edits.push(range);
        }
        let (ss, se) = (sel.start_offset(), sel.end_offset());
        let _ = found.key_range;
        cx.add_fix(s, e, msg, |c| {
            c.replace(ss, se, "where.missing");
            for &(a, b) in &edits {
                c.remove(a, b);
            }
        });
    }
}
