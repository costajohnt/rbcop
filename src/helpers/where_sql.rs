//! Shared pieces of rubocop-rails' Rails/WhereEquals, Rails/WhereNot and Rails/WhereRange: the
//! `where("sql", values)` / `where(["sql", values])` call shape and the named-value lookup.

use crate::prelude::*;
use ruby_prism::CallNode;

/// `(call _ M (array $str_type? $_ ?))` / `(call _ M $str_type? $_ ?)` (`many`: `$_ +`): the SQL
/// template's value and the value nodes after it.
pub fn template<'pr>(call: &CallNode<'pr>, many: bool) -> Option<(String, Vec<Node<'pr>>)> {
    let args = send_args(call);
    let fits = |n: usize| if many { n >= 2 } else { (1..=2).contains(&n) };
    if let [arr] = args.as_slice()
        && let Some(a) = arr.as_array_node()
    {
        let els: Vec<Node> = a.elements().iter().collect();
        if fits(els.len()) {
            let t = helpers::rails::str_value(&els[0])?;
            return Some((t, els.into_iter().skip(1).collect()));
        }
        // An array argument that does not fit may still be the template form below (it is not).
    }
    if !fits(args.len()) {
        return None;
    }
    let t = helpers::rails::str_value(&args[0])?;
    Some((t, args.into_iter().skip(1).collect()))
}

/// `hash_node.pairs.find { |p| p.key.value.to_sym == name.to_sym }`: Ok(None) when no key matches,
/// Err when a key has no `value` (the cop raises).
pub fn find_pair<'pr>(hash: &Node<'pr>, name: &str) -> Result<Option<ruby_prism::AssocNode<'pr>>, ()> {
    for p in helpers::rails::hash_pairs(hash) {
        let k = p.key();
        match helpers::rails::str_or_sym(&k) {
            Some(v) if v == name => return Ok(Some(p)),
            Some(_) => {}
            // int/float keys: `1.to_sym` raises too.
            None => return Err(()),
        }
    }
    Ok(None)
}

/// `"#{method}(#{column}: #{value})"`, with `table.column` nested.
pub fn good_method(method: &str, column: &str, value: &str) -> String {
    match column.split_once('.') {
        Some((table, col)) => format!("{method}({table}: {{ {col}: {value} }})"),
        None => format!("{method}({column}: {value})"),
    }
}

/// Ruby's ASCII `\s` and `\w`, as character class bodies.
pub const S: &str = r"[ \t\r\n\x0b\x0c]";
pub const W: &str = r"[A-Za-z0-9_]";
