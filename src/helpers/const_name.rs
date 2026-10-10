//! rubocop-ast `ConstNode#const_name` on Prism constants.

use ruby_prism::Node;

/// `node.const_name`: `Foo::Bar` for a constant (a leading `::` dropped), None for anything else.
pub fn const_name(node: &Node) -> Option<String> {
    if let Some(c) = node.as_constant_read_node() {
        return Some(String::from_utf8_lossy(c.name().as_slice()).into_owned());
    }
    let p = node.as_constant_path_node()?;
    let name = String::from_utf8_lossy(p.name()?.as_slice()).into_owned();
    Some(match p.parent() {
        None => name,
        // rubocop-ast interpolates a non-constant namespace's nil const_name as "".
        Some(ns) => format!("{}::{name}", const_name(&ns).unwrap_or_default()),
    })
}
