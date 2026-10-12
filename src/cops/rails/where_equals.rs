use crate::prelude::*;
use helpers::where_sql::{S, W, find_pair, good_method, template};

pub const NAME: &str = "Rails/WhereEquals";
pub static COP: WhereEquals = WhereEquals;
pub struct WhereEquals;

static EQ_ANON: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!(r"\A((?:{W}|\.)+){S}+={S}+\?\z")).unwrap());
static IN_ANON: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"(?i)\A((?:{W}|\.)+){S}+IN{S}+\(\?\)\z")).unwrap());
static EQ_NAMED: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!(r"\A((?:{W}|\.)+){S}+={S}+:({W}+)\z")).unwrap());
static IN_NAMED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"(?i)\A((?:{W}|\.)+){S}+IN{S}+\(:({W}+)\)\z")).unwrap());
static IS_NULL: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!(r"(?i)\A((?:{W}|\.)+){S}+IS{S}+NULL\z")).unwrap());

impl Cop for WhereEquals {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if !matches!(name, "where" | "not") {
            return;
        }
        // where_not?: a `not` needs a `where` send receiver.
        if name == "not"
            && !call.receiver().and_then(|r| helpers::rails::send_node(&r)).is_some_and(|r| method_name(&r) == "where")
        {
            return;
        }
        let Some((t, values)) = template(&call, false) else { return };
        let value_node = values.first();
        let (column, value) = if let Some(c) = EQ_ANON.captures(&t).or_else(|| IN_ANON.captures(&t)) {
            let Some(v) = value_node else { return };
            (c[1].to_string(), node_text(v).to_string())
        } else if let Some(c) = EQ_NAMED.captures(&t).or_else(|| IN_NAMED.captures(&t)) {
            let Some(h) = value_node.filter(|v| helpers::rails::hash_elements(v).is_some()) else { return };
            // `pair.value.source` raises when no key matches.
            let Ok(Some(pair)) = find_pair(h, &c[2]) else { return };
            (c[1].to_string(), helpers::rails::pair_value_source(&pair).to_string())
        } else if let Some(c) = IS_NULL.captures(&t) {
            (c[1].to_string(), "nil".to_string())
        } else {
            return;
        };
        if column.matches('.').count() > 1 {
            return;
        }
        let good = good_method(name, &column, &value);
        let s = call.message_loc().unwrap().start_offset();
        let (_, e) = send_range(&call);
        cx.add_fix(s, e, format!("Use `{good}` instead of manually constructing SQL."), |c| c.replace(s, e, &good));
    }
}
