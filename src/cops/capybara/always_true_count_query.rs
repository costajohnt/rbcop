use crate::prelude::*;
use helpers::capybara::{self as capy, QueryType};

pub const NAME: &str = "Capybara/AlwaysTrueCountQuery";
pub static COP: AlwaysTrueCountQuery = AlwaysTrueCountQuery;
pub struct AlwaysTrueCountQuery;

impl Cop for AlwaysTrueCountQuery {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        // positive selector queries
        if capy::query_type(name) != Some(QueryType::Selector)
            || !["assert_", "has_", "have_", "must_have_"].iter().any(|p| name.starts_with(p))
            || ["assert_no_", "has_no_", "have_no_"].iter().any(|p| name.starts_with(p))
        {
            return;
        }
        let mut chain: Vec<&Node> = cx.ancestors.iter().collect();
        chain.push(node);
        if !capy::capybara_receiver(call.receiver().as_ref(), &chain) {
            return;
        }
        // positive_usage?: a `have_` matcher must be the argument of `to`
        if name.starts_with("have_") {
            let parent = helpers::parser_parent::parser_parent(cx.ancestors).and_then(|p| p.as_call_node());
            let ok = parent.is_some_and(|p| {
                !is_csend(&p)
                    && method_name(&p) == "to"
                    && send_args(&p).first().is_some_and(|a| a.location().start_offset() == node.location().start_offset())
            });
            if !ok {
                return;
            }
        }
        // always_true_minimum: unbraced options of symbol-keyed pairs only
        let args = send_args(&call);
        let Some(options) = args.last().filter(|o| o.as_keyword_hash_node().is_some()) else { return };
        let elements = helpers::rails::hash_elements(options).unwrap();
        let mut pairs = Vec::new();
        for e in &elements {
            let Some(p) = e.as_assoc_node() else { return };
            let Some(k) = capy::sym(cx, &p.key()) else { return };
            pairs.push((k, p));
        }
        if pairs.iter().any(|(k, p)| matches!(k.as_str(), "count" | "maximum" | "between") && p.value().as_nil_node().is_none()) {
            return;
        }
        let minimum: Vec<_> = pairs.iter().filter(|(k, _)| k == "minimum").collect();
        if let [(_, p)] = &minimum[..]
            && p.value().as_integer_node().is_some()
            && int_literal_value(&p.value()) == Some(0)
        {
            let (s, e) = helpers::rspec::node_range(&p.as_node());
            cx.add(s, e, "`minimum: 0` makes this positive query succeed with no matches.");
        }
    }
}
