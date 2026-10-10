use crate::helpers::parser_parent::parser_parent;
use crate::prelude::*;
use ruby_prism::DefNode;

pub const NAME: &str = "Style/TrivialAccessors";
pub static COP: TrivialAccessors = TrivialAccessors;
pub struct TrivialAccessors;

/// The def body when it is a single expression.
fn single_body<'pr>(def: &DefNode<'pr>) -> Option<Node<'pr>> {
    let body = def.body()?;
    let Some(s) = body.as_statements_node() else { return Some(body) };
    let mut it = s.body().iter();
    let first = it.next()?;
    it.next().is_none().then_some(first)
}

fn bytes_str(b: &[u8]) -> &str {
    std::str::from_utf8(b).unwrap_or("")
}

/// `each_ancestor(:any_block, :class, :sclass, :module)`: true inside a module or an
/// `instance_eval` block before reaching a class.
fn in_module_or_instance_eval(ancestors: &[Node]) -> bool {
    for i in (0..ancestors.len()).rev() {
        let a = &ancestors[i];
        match kind::of(a) {
            kind::CLASS_NODE | kind::SINGLETON_CLASS_NODE => return false,
            kind::MODULE_NODE => return true,
            kind::BLOCK_NODE => {
                if i > 0
                    && let Some(call) = ancestors[i - 1].as_call_node()
                    && method_name(&call) == "instance_eval"
                {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

fn is_comparison(name: &str) -> bool {
    matches!(name, "==" | "===" | "!=" | "<=" | ">=" | ">" | "<")
}

impl Cop for TrivialAccessors {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let def = node.as_def_node().unwrap();
        let parent = parser_parent(cx.ancestors);
        if parent.is_none() || in_module_or_instance_eval(cx.ancestors) {
            return;
        }
        let defs = def.receiver().is_some();
        let cfg = cx.cfg;
        if defs && cfg.bool("IgnoreClassMethods").unwrap_or(false) {
            return;
        }
        let name = bytes_str(def.name().as_slice());
        let body = single_body(&def);
        let ivar_name = body.as_ref().and_then(|b| {
            b.as_instance_variable_read_node()
                .map(|r| r.name())
                .or_else(|| b.as_instance_variable_write_node().map(|w| w.name()))
        });
        let names_match = ivar_name.is_some_and(|iv| {
            let base = name.strip_suffix(['=', '?']).unwrap_or(name);
            bytes_str(iv.as_slice()).get(1..) == Some(base)
        });
        let allowed_name = name == "initialize"
            || helpers::allowed::allowed_method(cfg, name)
            || (cfg.bool("ExactNameMatch").unwrap_or(false) && !names_match);
        let predicate = name.ends_with('?');
        let dsl_writer = !(name.ends_with('=') && !is_comparison(name));

        let reader = def.parameters().is_none()
            && body.as_ref().is_some_and(|b| b.as_instance_variable_read_node().is_some())
            && !allowed_name
            && !(cfg.bool("AllowPredicates").unwrap_or(false) && predicate);
        let looks_writer = def.parameters().is_some_and(|p| {
            p.requireds().iter().count() == 1
                && p.requireds().iter().all(|r| r.as_required_parameter_node().is_some())
                && p.optionals().iter().count() == 0
                && crate::ast::parser_rest(p.rest()).is_none()
                && p.posts().iter().count() == 0
                && p.keywords().iter().count() == 0
                && p.keyword_rest().is_none()
                && p.block().is_none()
        }) && body.as_ref().and_then(|b| b.as_instance_variable_write_node()).is_some_and(|w| {
            w.value().as_local_variable_read_node().is_some()
        });
        let writer = looks_writer && !allowed_name && !(cfg.bool("AllowDSLWriters").unwrap_or(false) && dsl_writer);

        let kind = if reader {
            "reader"
        } else if writer {
            "writer"
        } else {
            return;
        };
        let msg = format!("Use `attr_{kind}` to define trivial {kind} methods.");

        // trivial_accessor_kind
        let fix_kind = if writer && !dsl_writer {
            Some("writer")
        } else if reader {
            Some("reader")
        } else {
            None
        };
        let parent_send = parent.and_then(|p| p.as_call_node()).is_some_and(|c| !is_csend(&c));
        let self_receiver = def.receiver().is_some_and(|r| r.as_self_node().is_some());
        let loc = node.location();
        let (start, end) = (loc.start_offset(), loc.end_offset());
        let replacement = fix_kind.filter(|_| !parent_send && names_match && !predicate).and_then(|k| {
            let accessor = format!("attr_{k} :{}", name.strip_suffix('=').unwrap_or(name));
            if !defs {
                Some(accessor)
            } else if self_receiver {
                let indent = " ".repeat(cx.src.column(start));
                Some(format!("class << self\n{indent}  {accessor}\n{indent}end"))
            } else {
                None
            }
        });
        let kw = def.def_keyword_loc();
        cx.add_fix(kw.start_offset(), kw.end_offset(), msg, |c| {
            if let Some(r) = &replacement {
                c.replace(start, end, r);
            }
        });
    }
}
