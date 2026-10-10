use crate::helpers::gemspec_help::{enclosing_spec, lvar_name, spec_block_var};
use crate::prelude::*;

pub const NAME: &str = "Gemspec/DuplicatedAssignment";
pub static COP: DuplicatedAssignment = DuplicatedAssignment;
pub struct DuplicatedAssignment;

struct Assignment {
    receiver: Vec<u8>,
    /// Grouping key besides the enclosing specification: method name, or `meth[arg]` key.
    key: String,
    spec: Option<usize>,
    /// Name shown in the message.
    shown: String,
    start: usize,
    end: usize,
}

#[derive(Default)]
struct State {
    /// Block variable of the first `Gem::Specification.new do |x|` (match_block_variable_name?).
    spec_var: Option<Vec<u8>>,
    plain: Vec<Assignment>,
    indexed: Vec<Assignment>,
}

/// rubocop-ast `literal?`.
fn is_literal(node: &Node) -> bool {
    matches!(
        kind::of(node),
        kind::STRING_NODE
            | kind::INTERPOLATED_STRING_NODE
            | kind::X_STRING_NODE
            | kind::INTERPOLATED_X_STRING_NODE
            | kind::INTEGER_NODE
            | kind::FLOAT_NODE
            | kind::SYMBOL_NODE
            | kind::INTERPOLATED_SYMBOL_NODE
            | kind::ARRAY_NODE
            | kind::HASH_NODE
            | kind::REGULAR_EXPRESSION_NODE
            | kind::INTERPOLATED_REGULAR_EXPRESSION_NODE
            | kind::TRUE_NODE
            | kind::FALSE_NODE
            | kind::NIL_NODE
            | kind::RANGE_NODE
            | kind::IMAGINARY_NODE
            | kind::RATIONAL_NODE
            | kind::SOURCE_FILE_NODE
            | kind::SOURCE_LINE_NODE
    )
}

/// Structural-equality key for a literal (parser nodes compare by type and children).
/// ponytail: strings and symbols compare by value, other literals by source text.
fn literal_key(node: &Node) -> String {
    if let Some(s) = node.as_string_node() {
        return format!("str:{}", String::from_utf8_lossy(s.unescaped()));
    }
    if let Some(s) = node.as_symbol_node() {
        return format!("sym:{}", String::from_utf8_lossy(s.unescaped()));
    }
    format!("{}:{}", kind::of(node), node_text(node))
}

/// AST::Node#assignment_method?
fn assignment_method(name: &str) -> bool {
    name.ends_with('=') && !matches!(name, "==" | "===" | "!=" | "<=" | ">=")
}

impl Cop for DuplicatedAssignment {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE, kind::CALL_TARGET_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let spec = enclosing_spec(cx.ancestors);
        if let Some(t) = node.as_call_target_node() {
            // `spec.a, spec.b = ...`: parser's mlhs holds `(send (lvar spec) :a=)`.
            let name = String::from_utf8_lossy(t.name().as_slice()).into_owned();
            if let Some(recv) = lvar_name(&t.receiver()).filter(|_| !t.is_safe_navigation() && assignment_method(&name)) {
                let loc = t.location();
                let a = Assignment {
                    receiver: recv.to_vec(),
                    key: name.clone(),
                    spec,
                    shown: name,
                    start: loc.start_offset(),
                    end: loc.end_offset(),
                };
                cx.state::<State>().plain.push(a);
            }
            return;
        }
        let call = node.as_call_node().unwrap();
        if is_csend(&call) {
            return;
        }
        if let Some(var) = spec_block_var(&call) {
            let st = cx.state::<State>();
            if st.spec_var.is_none() {
                st.spec_var = Some(var.to_vec());
            }
        }
        let Some(recv) = call.receiver() else { return };
        let name = method_name(&call);
        let (start, end) = send_range(&call);
        if let Some(r) = lvar_name(&recv) {
            if assignment_method(name) {
                let a = Assignment { receiver: r.to_vec(), key: name.to_string(), spec, shown: name.to_string(), start, end };
                cx.state::<State>().plain.push(a);
            }
            return;
        }
        // (send (send (lvar X) _) :[]= literal? _)
        let args = send_args(&call);
        let Some(inner) = recv.as_call_node() else { return };
        let Some(r) = inner.receiver().as_ref().and_then(lvar_name) else { return };
        if name != "[]=" || args.len() != 2 || !is_literal(&args[0]) {
            return;
        }
        if is_csend(&inner) || inner.arguments().is_some() || inner.block().is_some() {
            return;
        }
        let inner_name = method_name(&inner);
        let a = Assignment {
            receiver: r.to_vec(),
            key: format!("{inner_name}\0{}", literal_key(&args[0])),
            spec,
            shown: format!("{inner_name}[{}]=", node_text(&args[0])),
            start,
            end,
        };
        cx.state::<State>().indexed.push(a);
    }

    fn on_file(&self, cx: &mut Cx) {
        let st = std::mem::take(cx.state::<State>());
        let matches_var =
            |a: &&Assignment| st.spec_var.as_deref() == Some(&a.receiver[..]) || matches!(&a.receiver[..], b"_1" | b"it");
        for list in [&st.plain, &st.indexed] {
            let list: Vec<&Assignment> = list.iter().filter(matches_var).collect();
            for (i, a) in list.iter().enumerate() {
                let Some(first) = list[..i].iter().find(|f| f.spec == a.spec && f.key == a.key) else { continue };
                let first_line = cx.src.line_of(first.start);
                let (s, e) = helpers::range_help::first_line_to_last_column(cx.src, a.start, a.end);
                let msg = format!("`{}` method calls already given on line {first_line} of the gemspec.", a.shown);
                cx.add(s, e, msg);
            }
        }
    }
}
