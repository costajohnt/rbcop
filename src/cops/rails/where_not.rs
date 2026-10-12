use crate::prelude::*;
use helpers::where_sql::{S, W, find_pair, good_method, template};

pub const NAME: &str = "Rails/WhereNot";
pub static COP: WhereNot = WhereNot;
pub struct WhereNot;

static NOT_EQ_ANON: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"\A((?:{W}|\.)+){S}+(?:!=|<>){S}+\?\z")).unwrap());
static NOT_IN_ANON: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"(?i)\A((?:{W}|\.)+){S}+NOT{S}+IN{S}+\(\?\)\z")).unwrap());
static NOT_EQ_NAMED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"\A((?:{W}|\.)+){S}+(?:!=|<>){S}+:({W}+)\z")).unwrap());
static NOT_IN_NAMED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"(?i)\A((?:{W}|\.)+){S}+NOT{S}+IN{S}+\(:({W}+)\)\z")).unwrap());
static IS_NOT_NULL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"(?i)\A((?:{W}|\.)+){S}+IS{S}+NOT{S}+NULL\z")).unwrap());

impl Cop for WhereNot {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "where" {
            return;
        }
        let Some((t, values)) = template(&call, false) else { return };
        let value_node = values.first();
        let (column, value) = if let Some(c) = NOT_EQ_ANON.captures(&t).or_else(|| NOT_IN_ANON.captures(&t)) {
            let Some(v) = value_node else { return };
            (c[1].to_string(), node_text(v).to_string())
        } else if let Some(c) = NOT_EQ_NAMED.captures(&t).or_else(|| NOT_IN_NAMED.captures(&t)) {
            let Some(h) = value_node.filter(|v| helpers::rails::hash_elements(v).is_some()) else { return };
            let Some(Some(pair)) = find_pair(h, &c[2]) else { return };
            (c[1].to_string(), helpers::rails::pair_value_source(&pair).to_string())
        } else if let Some(c) = IS_NOT_NULL.captures(&t) {
            (c[1].to_string(), "nil".to_string())
        } else {
            return;
        };
        if column.matches('.').count() > 1 {
            return;
        }
        let dot = call.call_operator_loc().map_or(".", |d| text(&d));
        let good = good_method(&format!("where{dot}not"), &column, &value);
        let s = call.message_loc().unwrap().start_offset();
        let (_, e) = send_range(&call);
        cx.add_fix(s, e, format!("Use `{good}` instead of manually constructing negated SQL in `where`."), |c| {
            c.replace(s, e, &good)
        });
    }
}
