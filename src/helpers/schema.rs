//! rubocop-rails SchemaLoader: `db/schema.rb` (found upward from the working directory), read once
//! per run into tables, columns and indexes; plus ActiveRecordHelper's table_name and
//! resolve_relation_into_column.

use crate::prelude::*;
use ruby_prism::Visit;
use std::sync::OnceLock;

pub struct Index {
    /// Array columns (`None` for a non-string element, which never equals a column name).
    pub columns: Vec<Option<String>>,
    pub expression: Option<String>,
    pub unique: bool,
}

pub struct Table {
    /// The `create_table` name when it is a string (a Symbol never equals `table_name`).
    pub name: Option<String>,
    pub columns: Vec<Option<String>>,
    pub indices: Vec<Index>,
}

pub struct Schema {
    pub tables: Vec<Table>,
    /// Top-level `add_index` calls: table name and index.
    pub add_indices: Vec<(Option<String>, Index)>,
}

impl Table {
    pub fn with_column(&self, name: &str) -> bool {
        self.columns.iter().any(|c| c.as_deref() == Some(name))
    }
}

impl Schema {
    pub fn table_by(&self, name: &str) -> Option<&Table> {
        self.tables.iter().find(|t| t.name.as_deref() == Some(name))
    }

    pub fn add_indices_by<'a>(&'a self, table: &'a str) -> impl Iterator<Item = &'a Index> + 'a {
        self.add_indices.iter().filter(move |(t, _)| t.as_deref() == Some(table)).map(|(_, i)| i)
    }
}

/// The schema, or None when there is no db/schema.rb or it is not one `... do ... end` block.
pub fn schema() -> Option<&'static Schema> {
    static SCHEMA: OnceLock<Option<Schema>> = OnceLock::new();
    SCHEMA.get_or_init(load).as_ref()
}

fn load() -> Option<Schema> {
    let mut dir = crate::config::cwd();
    let path = loop {
        let p = dir.join("db/schema.rb");
        if p.is_file() {
            break p;
        }
        if !dir.pop() {
            return None;
        }
    };
    let text = std::fs::read(path).ok()?;
    let result = ruby_prism::parse(&text);
    let program = result.node();
    let stmts: Vec<Node> = program.as_program_node()?.statements().body().iter().collect();
    // `raise "Unexpected type"` unless the AST is a block.
    let [root] = stmts.as_slice() else { return None };
    let block = helpers::rails::block_call(root)?.block()?.as_block_node()?;
    let mut schema = Schema { tables: Vec::new(), add_indices: Vec::new() };
    let Some(body) = block.body() else { return Some(schema) };
    let multi = body.as_statements_node().filter(|s| s.body().iter().count() > 1);
    let statements: Vec<Node> = helpers::rails::body_statements(Some(body));
    for s in &statements {
        if let Some(c) = helpers::rails::block_call(s).filter(|c| method_name(c) == "create_table") {
            schema.tables.push(table(&c)?);
        }
        if multi.is_some()
            && let Some(c) = helpers::rails::send_node(s).filter(|c| method_name(c) == "add_index")
        {
            let args = send_args(&c);
            let mut index = index(args.get(1)?, args.last());
            index.unique = unique(args.last());
            schema.add_indices.push((args.first().and_then(helpers::rails::str_value), index));
        }
    }
    Some(schema)
}

fn unique(last: Option<&Node>) -> bool {
    last.is_some_and(|h| helpers::rails::hash_pairs(h).iter().any(|p| helpers::rails::sym_value(&p.key()).as_deref() == Some("unique")))
}

fn index(columns: &Node, last: Option<&Node>) -> Index {
    let (columns, expression) = match columns.as_array_node() {
        Some(a) => (a.elements().iter().map(|e| helpers::rails::str_value(&e)).collect(), None),
        None => (Vec::new(), helpers::rails::node_value(columns)),
    };
    Index { columns, expression, unique: unique(last) }
}

fn table(c: &ruby_prism::CallNode) -> Option<Table> {
    let name = send_args(c).first()?.as_string_node().and_then(|s| String::from_utf8(s.unescaped().to_vec()).ok());
    let body = c.block()?.as_block_node()?.body();
    let mut t = Table { name, columns: Vec::new(), indices: Vec::new() };
    for s in helpers::rails::body_statements(body) {
        let Some(col) = helpers::rails::send_node(&s) else { continue };
        let args = send_args(&col);
        if method_name(&col) == "index" {
            t.indices.push(index(args.first()?, args.last()));
        } else {
            // Column#name: first_argument.str_content (nil for anything but a string).
            t.columns.push(helpers::rails::str_value(args.first()?));
        }
    }
    Some(t)
}

/// ActiveRecordHelper#table_name for the class at `ancestors[ci]`.
pub fn table_name(ancestors: &[Node], ci: usize) -> String {
    let class = &ancestors[ci];
    // find_set_table_name: the last `self.table_name = 'x'` anywhere in the class.
    struct F(Option<String>);
    impl<'pr> Visit<'pr> for F {
        fn visit_call_node(&mut self, c: &ruby_prism::CallNode<'pr>) {
            if !is_csend(c)
                && method_name(c) == "table_name="
                && c.receiver().is_some_and(|r| r.as_self_node().is_some())
                && let [a] = send_args(c).as_slice()
                && let Some(v) = helpers::rails::str_or_sym(a)
            {
                self.0 = Some(v);
            }
            ruby_prism::visit_call_node(self, c);
        }
    }
    let mut f = F(None);
    f.visit(class);
    if let Some(v) = f.0 {
        return v;
    }
    let short = |n: &Node| -> String {
        if let Some(r) = n.as_constant_read_node() {
            String::from_utf8_lossy(r.name().as_slice()).into_owned()
        } else if let Some(p) = n.as_constant_path_node() {
            p.name().map(|x| String::from_utf8_lossy(x.as_slice()).into_owned()).unwrap_or_default()
        } else {
            String::new()
        }
    };
    // defined_module.each_node: the class's constant and its scopes, innermost name first.
    let mut names: Vec<String> = Vec::new();
    let mut n = class.as_class_node().unwrap().constant_path();
    loop {
        names.push(short(&n));
        let Some(p) = n.as_constant_path_node() else { break };
        match p.parent() {
            Some(parent) => n = parent,
            None => {
                // `::Foo`: the cbase node, whose children[1] is nil.
                names.push(String::new());
                break;
            }
        }
    }
    for a in ancestors[..ci].iter().rev() {
        if let Some(c) = a.as_class_node() {
            names.push(short(&c.constant_path()));
        } else if let Some(m) = a.as_module_node() {
            names.push(short(&m.constant_path()));
        }
    }
    names.reverse();
    helpers::inflector::tableize(&names.join("_"))
}

/// The `(send nil? :belongs_to {str sym} ...)` calls anywhere in the class, in source order.
fn belongs_tos<'pr>(class: &Node<'pr>) -> Vec<Node<'pr>> {
    struct F<'pr>(Vec<Node<'pr>>);
    impl<'pr> Visit<'pr> for F<'pr> {
        fn visit_call_node(&mut self, c: &ruby_prism::CallNode<'pr>) {
            if !is_csend(c)
                && helpers::rails::is_command(c, "belongs_to")
                && send_args(c).first().is_some_and(|a| a.as_string_node().is_some() || a.as_symbol_node().is_some())
            {
                self.0.push(c.as_node());
            }
            ruby_prism::visit_call_node(self, c);
        }
    }
    let mut f = F(Vec::new());
    f.visit(class);
    f.0
}

/// ActiveRecordHelper#resolve_relation_into_column: the column(s) for `name`, a column itself or a
/// belongs_to association's foreign key (and type for a polymorphic one).
pub fn resolve_relation_into_column(name: &str, class: &Node, table: Option<&Table>) -> Option<Vec<String>> {
    let table = table?;
    if table.with_column(name) {
        return Some(vec![name.to_string()]);
    }
    for b in belongs_tos(class) {
        let args = send_args(&b.as_call_node().unwrap());
        if helpers::rails::str_or_sym(&args[0]).as_deref() != Some(name) {
            continue;
        }
        let last = args.last().unwrap();
        let pairs = helpers::rails::hash_pairs(last);
        // foreign_key_of: the first `foreign_key:` pair with a str/sym value.
        let fk = pairs
            .iter()
            .filter(|p| p.key().as_symbol_node().is_some() && helpers::rails::sym_value(&p.key()).as_deref() == Some("foreign_key"))
            .find_map(|p| {
                let v = helpers::rails::pair_value(p);
                (v.as_string_node().is_some() || v.as_symbol_node().is_some()).then(|| helpers::rails::str_or_sym(&v)).flatten()
            })
            .unwrap_or_else(|| format!("{name}_id"));
        if !table.with_column(&fk) {
            continue;
        }
        let polymorphic = pairs.iter().any(|p| {
            helpers::rails::sym_value(&p.key()).as_deref() == Some("polymorphic") && helpers::rails::pair_value(p).as_true_node().is_some()
        });
        return Some(if polymorphic { vec![fk, format!("{name}_type")] } else { vec![fk] });
    }
    None
}
