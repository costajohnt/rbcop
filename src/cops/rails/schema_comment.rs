use crate::prelude::*;

pub const NAME: &str = "Rails/SchemaComment";
pub static COP: SchemaComment = SchemaComment;
pub struct SchemaComment;

/// RAILS_ABSTRACT_SCHEMA_DEFINITIONS | _HELPERS | POSTGRES | MYSQL
const COLUMN_METHODS: &[&str] = &[
    "bigint", "binary", "boolean", "date", "datetime", "decimal", "float", "integer", "json", "string", "text", "time",
    "timestamp", "virtual", "column", "references", "belongs_to", "primary_key", "numeric", "bigserial", "bit",
    "bit_varying", "cidr", "citext", "daterange", "hstore", "inet", "interval", "int4range", "int8range", "jsonb",
    "ltree", "macaddr", "money", "numrange", "oid", "point", "line", "lseg", "box", "path", "polygon", "circle", "serial",
    "tsrange", "tstzrange", "tsvector", "uuid", "xml", "blob", "tinyblob", "mediumblob", "longblob", "tinytext",
    "mediumtext", "longtext", "unsigned_integer", "unsigned_bigint", "unsigned_float", "unsigned_decimal",
];

/// (hash <(pair {(sym :comment) (str "comment")} !{nil (str blank?)}) ...>)
fn comment_present(n: &Node) -> bool {
    if helpers::rails::hash_elements(n).is_none() {
        return false;
    }
    helpers::rails::hash_pairs(n).iter().any(|p| {
        let k = p.key();
        let key = helpers::rails::sym_value(&k).as_deref() == Some("comment") || helpers::rails::str_value(&k).as_deref() == Some("comment");
        let v = p.value();
        let blank = v.as_nil_node().is_some() || helpers::rails::str_value(&v).is_some_and(|s| s.chars().all(char::is_whitespace));
        key && !blank
    })
}

/// t_column_without_comment?
fn t_column_without_comment(n: &Node) -> bool {
    let Some(c) = helpers::rails::send_node(n) else { return false };
    if !COLUMN_METHODS.contains(&method_name(&c)) {
        return false;
    }
    let args = send_args(&c);
    let with_comment = (2..=3).contains(&args.len()) && comment_present(args.last().unwrap());
    !with_comment
}

impl Cop for SchemaComment {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        if call.receiver().is_some() {
            return;
        }
        let args = send_args(&call);
        let (s, e) = send_range(&call);
        match method_name(&call) {
            "add_column" => {
                if (3..=4).contains(&args.len()) && !(args.len() == 4 && comment_present(&args[3])) {
                    cx.add(s, e, "New database column without `comment`.");
                }
            }
            "create_table" => {
                if (1..=2).contains(&args.len()) && !(args.len() >= 2 && comment_present(&args[1])) {
                    cx.add(s, e, "New database table without `comment`.");
                    return;
                }
                // create_table_with_block?(node.parent): check the block body's columns.
                let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return };
                let ok_params = match block.parameters() {
                    None => true,
                    Some(p) => p.as_block_parameters_node().is_some() || p.as_numbered_parameters_node().is_some() || p.as_it_parameters_node().is_some(),
                };
                if !ok_params {
                    return;
                }
                let Some(body) = block.body() else { return };
                for st in helpers::rails::body_statements(Some(body)) {
                    if t_column_without_comment(&st) {
                        cx.add(st.location().start_offset(), st.location().end_offset(), "New database column without `comment`.");
                    }
                }
            }
            _ => {}
        }
    }
}
