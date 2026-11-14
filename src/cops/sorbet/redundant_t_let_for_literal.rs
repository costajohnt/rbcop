use crate::cops::sorbet_obsolete_strict_memoization::t_send;
use crate::helpers::literal_node::{literal, parser_type};
use crate::helpers::sorbet_rbi_type::rbs_annotation_after;
use crate::helpers::sorbet_t_let::{replace_t_let, statically_scoped};
use crate::helpers::sorbet_target_version::enabled_for;
use crate::prelude::*;

pub const NAME: &str = "Sorbet/RedundantTLetForLiteral";
pub static COP: RedundantTLetForLiteral = RedundantTLetForLiteral;
pub struct RedundantTLetForLiteral;

const MIN: &str = "0.6.13304";

/// LITERAL_TYPE_TO_CLASS (also ELEMENT_TYPE_TO_CLASS without regexp).
fn literal_class(t: &str) -> Option<&'static str> {
    Some(match t {
        "dstr" | "str" => "String",
        "dsym" | "sym" => "Symbol",
        "float" => "Float",
        "int" => "Integer",
        "regexp" => "Regexp",
        _ => return None,
    })
}

fn message(annotation: &str, ty: &str) -> String {
    format!("Redundant {annotation} for {ty} literal. Sorbet can infer this type automatically.")
}

/// A parser `send` named `name` (not a csend, no literal block).
fn send_named<'pr>(node: &Node<'pr>, name: &str) -> Option<ruby_prism::CallNode<'pr>> {
    node.as_call_node()
        .filter(|c| parser_type(node) == "send" && method_name(c) == name && c.block().is_none_or(|b| b.as_block_node().is_none()))
}

/// The cop's version check, as a Result (Err: RuboCop raises).
struct Version(Result<bool, ()>);

impl RedundantTLetForLiteral {
    /// inferable_literal_node: the value itself unless it is a send; a regexp's `.freeze` receiver
    /// on supported targets.
    fn inferable_literal<'pr>(&self, value: &Node<'pr>, v: &Version) -> Result<Option<Node<'pr>>, ()> {
        if parser_type(value) != "send" {
            return Ok(None);
        }
        let Some(c) = send_named(value, "freeze") else { return Ok(None) };
        if !c.receiver().is_some_and(|r| parser_type(&r) == "regexp") {
            return Ok(None);
        }
        if !v.0? {
            return Ok(None);
        }
        Ok(c.receiver())
    }
}

fn inferable_array(node: &Node) -> bool {
    let Some(a) = node.as_array_node() else { return false };
    let elements: Vec<Node> = a.elements().iter().collect();
    !elements.is_empty()
        && elements.iter().all(|c| {
            if c.as_array_node().is_some() {
                inferable_array(c)
            } else {
                matches!(parser_type(c), "dstr" | "dsym" | "str" | "sym" | "int" | "float" | "true" | "false" | "nil")
            }
        })
}

fn inferred_array_type(node: &Node) -> Option<String> {
    let classes: Vec<Option<&str>> = node
        .as_array_node()?
        .elements()
        .iter()
        .map(|c| literal_class(parser_type(&c)).filter(|&k| k != "Regexp"))
        .collect();
    let first = (*classes.first()?)?;
    classes.iter().all(|c| *c == Some(first)).then(|| format!("T::Array[{first}]"))
}

/// `T::Array[...]` (with or without a leading `::`).
fn t_array_type(node: &Node) -> bool {
    send_named(node, "[]").is_some_and(|c| c.receiver().is_some_and(|r| node_text(&r).trim_start_matches("::") == "T::Array"))
}

fn normalize(s: &str) -> String {
    static WS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[ \t\r\n\x0b\x0c]+").unwrap());
    static COMMA: LazyLock<Regex> = LazyLock::new(|| Regex::new(r",([)\]}])").unwrap());
    COMMA.replace_all(&WS.replace_all(s, ""), "$1").into_owned()
}

fn strip_cbase(s: &str) -> &str {
    s.strip_prefix("::").unwrap_or(s)
}

impl Cop for RedundantTLetForLiteral {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CONSTANT_WRITE_NODE, kind::CONSTANT_PATH_WRITE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if !statically_scoped(cx.ancestors) {
            return;
        }
        let value = match node.as_constant_write_node() {
            Some(w) => w.value(),
            None => node.as_constant_path_write_node().unwrap().value(),
        };
        let v = Version(enabled_for(MIN).map_err(|_| ()));
        if self.check(cx, node, &value, &v).is_err() {
            crate::runner::cop_error_at(NAME, cx.src, crate::runner::visiting());
        }
    }
}

impl RedundantTLetForLiteral {
    fn register(&self, cx: &mut Cx, t_let: &Node, value: &Node, ty: &str) {
        let (s, e) = (t_let.location().start_offset(), t_let.location().end_offset());
        let (rs, re, text) = replace_t_let(cx.src, (s, e), value);
        cx.add_fix(s, e, message("`T.let`", ty), |c| c.replace(rs, re, &text));
    }

    fn check(&self, cx: &mut Cx, node: &Node, value: &Node, v: &Version) -> Result<(), ()> {
        if let Some((_, a)) = t_send(value, "let", 2) {
            let lit = &a[0];
            let lit_ok = literal(lit)
                || send_named(lit, "freeze").is_some_and(|c| call_args(&c).is_empty() && c.receiver().is_some_and(|r| parser_type(&r) == "regexp"));
            if lit_ok && let Some(class) = a[1].as_constant_read_node() {
                let class = std::str::from_utf8(class.name().as_slice()).unwrap_or("");
                let literal_node = if parser_type(lit) == "send" { self.inferable_literal(lit, v)? } else { None };
                let t = match &literal_node {
                    Some(n) => parser_type(n),
                    None if parser_type(lit) != "send" => parser_type(lit),
                    None => "",
                };
                if literal_class(t) == Some(class) {
                    self.register(cx, value, lit, class);
                }
            }
        }
        if v.0?
            && let Some((_, a)) = t_send(value, "let", 2)
        {
            let lit = &a[0];
            let frozen = send_named(lit, "freeze").filter(|c| call_args(c).is_empty() && c.receiver().is_some_and(|r| r.as_array_node().is_some()));
            if lit.as_array_node().is_some() || frozen.is_some() {
                let array = frozen.as_ref().and_then(|c| c.receiver());
                let array = array.as_ref().unwrap_or(lit);
                let ty = &a[1];
                let tuple_elem = t_array_type(ty) && {
                    let args = call_args(&ty.as_call_node().unwrap());
                    args.len() == 1 && args[0].as_array_node().is_some()
                };
                let redundant = !tuple_elem
                    && if frozen.is_some() {
                        t_array_type(ty)
                    } else {
                        inferred_array_type(array).as_deref() == Some(strip_cbase(&normalize(node_text(ty))))
                    };
                if inferable_array(array) && redundant {
                    self.register(cx, value, lit, "Array");
                }
            }
        }
        self.check_rbs_annotation(cx, node, value, v)
    }

    fn check_rbs_annotation(&self, cx: &mut Cx, node: &Node, value: &Node, v: &Version) -> Result<(), ()> {
        let end = node.location().end_offset();
        let Some((cs, ce, annotation)) = rbs_annotation_after(cx.src, end) else { return Ok(()) };
        let annotated = normalize(&annotation);
        let literal_node = if parser_type(value) == "send" { self.inferable_literal(value, v)? } else { None };
        let t = match &literal_node {
            Some(n) => Some(parser_type(n)),
            None if parser_type(value) != "send" => Some(parser_type(value)),
            None => None,
        };
        if let Some(inferred) = t.and_then(literal_class) {
            if strip_cbase(&annotated) == inferred {
                self.register_rbs(cx, end, (cs, ce), inferred);
            }
            return Ok(());
        }
        if !v.0? {
            return Ok(());
        }
        let frozen = send_named(value, "freeze").and_then(|c| c.receiver());
        let array = frozen.as_ref().unwrap_or(value);
        if !inferable_array(array) {
            return Ok(());
        }
        static TUPLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\AArray\[\[.*\]\]\z").unwrap());
        static ARRAY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\AArray\[.+\]\z").unwrap());
        let rbs_type = strip_cbase(&annotated);
        let redundant = !TUPLE.is_match(rbs_type)
            && if frozen.is_some() {
                ARRAY.is_match(rbs_type)
            } else {
                inferred_array_type(array).is_some_and(|t| t.strip_prefix("T::").unwrap_or(&t) == rbs_type)
            };
        if redundant {
            self.register_rbs(cx, end, (cs, ce), "Array");
        }
        Ok(())
    }

    fn register_rbs(&self, cx: &mut Cx, node_end: usize, (cs, ce): (usize, usize), ty: &str) {
        cx.add_fix(cs, ce, message("RBS annotation", ty), |c| c.remove(node_end, ce));
    }
}
