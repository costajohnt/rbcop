use crate::helpers::packaging::{Inspected, falls_in_lib, single_arg, str_value};
use crate::prelude::*;
use ruby_prism::CallNode;
use std::path::Path;

pub const NAME: &str = "Packaging/RequireHardcodingLib";
pub static COP: RequireHardcodingLib = RequireHardcodingLib;
pub struct RequireHardcodingLib;

static LIB_PREFIX: LazyLock<regex::bytes::Regex> = LazyLock::new(|| regex::bytes::Regex::new(r"(?m)^.*/lib/").unwrap());

impl Cop for RequireHardcodingLib {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn restrict_on_send(&self) -> &'static [&'static str] {
        &["require"]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) {
            return;
        }
        let Some(arg) = single_arg(&call, "require") else { return };
        let file = Inspected::new(cx);
        let target = match require_target(&arg, &file, !cx.src.parser_gem) {
            Ok(Some(t)) => t,
            Ok(None) => return,
            // `str.prepend(".")` on the Prism translation's frozen string: FrozenError.
            Err(()) => return crate::runner::cop_error_at(NAME, cx.src, crate::runner::visiting()),
        };
        if !file.not_in_lib_or_gemspec() {
            return;
        }
        // good_require_call: `@str.sub(%r{^.*/lib/}, "")`
        let good = String::from_utf8_lossy(&LIB_PREFIX.replacen(&target, 1, &b""[..])).into_owned();
        let (s, e) = send_range(&call);
        cx.add_fix(
            s,
            e,
            "Avoid using `require` with relative path to `lib/`. Use `require` with absolute path instead.",
            |c| c.replace(s, e, &format!("require \"{good}\"")),
        );
    }
}

/// The `require?` matcher's string (`@str`, with the `.` the dirname-plus forms prepend) when it
/// maps into lib/:
///   (str #falls_in_lib?)
///   (send (const nil? :File) :expand_path (str #falls_in_lib?) (send nil? :__dir__))
///   (send (const nil? :File) :expand_path (str #falls_in_lib_using_file?) (str _))
///   (send (send (const nil? :File) :dirname {(str _) (send nil? _)}) :+ (str #falls_in_lib_with_file_dirname_plus_str?))
///   (dstr (begin (send (const nil? :File) :dirname {(str _) (send nil? _)})) (str #falls_in_lib_with_file_dirname_plus_str?))
/// Err when the dirname-plus forms raise: their predicate prepends to the string value, which the
/// Prism translation freezes.
fn require_target(arg: &Node, file: &Inspected, frozen: bool) -> Result<Option<Vec<u8>>, ()> {
    let Some(rhs) = dirname_plus(arg) else { return Ok(simple_target(arg, file)) };
    if frozen {
        return Err(());
    }
    // `str.prepend(".")`, then target_falls_in_lib?.
    let s = [&b"."[..], &rhs].concat();
    Ok(falls_in_lib(&s, &file.dir).then_some(s))
}

/// The first three forms.
fn simple_target(arg: &Node, file: &Inspected) -> Option<Vec<u8>> {
    let hit = |s: &[u8], base: &Path| falls_in_lib(s, base).then(|| s.to_vec());
    if let Some(s) = str_value(arg) {
        return hit(&s, &file.dir);
    }
    if let Some(call) = send(arg)
        && file_call(&call, "expand_path")
    {
        let [path, base] = <[Node; 2]>::try_from(send_args(&call)).ok()?;
        let s = str_value(&path)?;
        if send(&base).is_some_and(|b| b.receiver().is_none() && method_name(&b) == "__dir__" && send_args(&b).is_empty())
        {
            return hit(&s, &file.dir);
        }
        return if is_str(&base) { hit(&s, &file.path) } else { None };
    }
    None
}

/// The string of the dirname-plus forms (`File.dirname(...) + "str"`, `"#{File.dirname(...)}str"`).
fn dirname_plus(arg: &Node) -> Option<Vec<u8>> {
    if let Some(call) = send(arg) {
        let [rhs] = <[Node; 1]>::try_from(send_args(&call)).ok()?;
        let lhs = call.receiver().and_then(|r| send(&r));
        return (method_name(&call) == "+" && lhs.is_some_and(|r| dirname(&r))).then(|| str_value(&rhs)).flatten();
    }
    let parts: Vec<Node> = arg.as_interpolated_string_node()?.parts().iter().collect();
    let [first, rhs] = parts.as_slice() else { return None };
    let embedded = first.as_embedded_statements_node()?;
    let stmts: Vec<Node> = embedded.statements()?.body().iter().collect();
    let [inner] = stmts.as_slice() else { return None };
    if !send(inner).is_some_and(|c| dirname(&c)) {
        return None;
    }
    str_value(rhs)
}

/// A parser `send`: a call without a literal block, not `&.`.
fn send<'pr>(node: &Node<'pr>) -> Option<CallNode<'pr>> {
    node.as_call_node().filter(|c| !is_csend(c) && c.block().is_none_or(|b| b.as_block_node().is_none()))
}

/// `(str _)`: a string literal or `__FILE__`.
fn is_str(node: &Node) -> bool {
    str_value(node).is_some() || node.as_source_file_node().is_some()
}

/// `(send (const nil? :File) :<name> ...)`
fn file_call(call: &CallNode, name: &str) -> bool {
    method_name(call) == name && call.receiver().is_some_and(|r| r.as_constant_read_node().is_some_and(|c| c.name().as_slice() == b"File"))
}

/// `(send (const nil? :File) :dirname {(str _) (send nil? _)})`
fn dirname(call: &CallNode) -> bool {
    if !file_call(call, "dirname") {
        return false;
    }
    let Ok([arg]) = <[Node; 1]>::try_from(send_args(call)) else { return false };
    is_str(&arg) || send(&arg).is_some_and(|c| c.receiver().is_none() && send_args(&c).is_empty())
}
