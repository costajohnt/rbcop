use crate::prelude::*;
use crate::project_index::{self, Visibility};

pub const NAME: &str = "Lint/UnusedPrivateMethod";
pub static COP: UnusedPrivateMethod = UnusedPrivateMethod;
pub struct UnusedPrivateMethod;

const IMPLICITLY_INVOKED_METHODS: &[&str] = &[
    "initialize",
    "initialize_copy",
    "initialize_clone",
    "initialize_dup",
    "method_missing",
    "respond_to_missing?",
    "marshal_dump",
    "marshal_load",
    "coerce",
    "instance_variables_to_inspect",
    "inherited",
    "included",
    "extended",
    "prepended",
    "append_features",
    "extend_object",
    "prepend_features",
    "const_missing",
    "const_added",
    "method_added",
    "method_removed",
    "method_undefined",
    "singleton_method_added",
    "singleton_method_removed",
    "singleton_method_undefined",
];

#[derive(Default)]
struct State {
    literals: Option<project_index::Literals>,
}

/// Only with `AllCops/UseProjectIndex`: private instance methods no call, symbol or string in
/// the project names.
impl Cop for UnusedPrivateMethod {
    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(index) = project_index::get() else { return };
        let def = node.as_def_node().unwrap();
        if def.receiver().is_some() {
            return;
        }
        let name = String::from_utf8_lossy(def.name().as_slice()).into_owned();
        // checkable_declaration
        if IMPLICITLY_INVOKED_METHODS.contains(&name.as_str())
            || cx.cfg.array("AllowedNames").contains(&name)
            || helpers::allowed::matches_allowed_pattern(cx.cfg, &name)
        {
            return;
        }
        let anc = cx.ancestors;
        let nested = anc.iter().any(|a| {
            matches!(a, Node::DefNode { .. } | Node::BlockNode { .. } | Node::LambdaNode { .. } | Node::SingletonClassNode { .. })
        });
        if nested {
            return;
        }
        let Some(ns) = project_index::enclosing_namespace(index, anc) else { return };
        let member = format!("{name}()");
        // Before the read guard below: it takes one of its own the first time.
        let referenced = index.method_reference_names().contains(&name);
        {
            let ix = index.read();
            let Some(decl) = ix.member(ns, &member) else { return };
            if ix.visibility(decl) != Some(Visibility::Private) || referenced {
                return;
            }
            // owner_with_descendants? / override?
            let Some(owner) = ix.owner(decl).filter(|&o| ix.is_namespace(o)) else {
                return self.report(cx, &def, &name);
            };
            let owner_name = ix.name(owner);
            if ix.descendants(owner).into_iter().any(|d| ix.name(d) != owner_name)
                || ix.ancestors(owner).into_iter().any(|a| ix.name(a) != owner_name && ix.member(a, &member).is_some())
            {
                return;
            }
        }
        self.report(cx, &def, &name);
    }
}

impl UnusedPrivateMethod {
    fn report(&self, cx: &mut Cx, def: &ruby_prism::DefNode, name: &str) {
        if cx.state::<State>().literals.is_none() {
            let lits = cx.ancestors.first().map(|root| project_index::literals(cx.src, root, true));
            cx.state::<State>().literals = Some(lits.unwrap_or(project_index::Literals {
                names: Default::default(),
                prefixes: Default::default(),
            }));
        }
        let lits = cx.state::<State>().literals.as_ref().unwrap();
        if lits.names.contains(name) || lits.prefixes.iter().any(|p| name.starts_with(p.as_str())) {
            return;
        }
        let (s, e) = (def.def_keyword_loc().start_offset(), def.name_loc().end_offset());
        cx.add(s, e, format!("Private method `{name}` appears to be unused."));
    }
}
