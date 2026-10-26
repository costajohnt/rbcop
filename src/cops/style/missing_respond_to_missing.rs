use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Style/MissingRespondToMissing";
pub static COP: MissingRespondToMissing = MissingRespondToMissing;
pub struct MissingRespondToMissing;

/// Finds a `respond_to_missing?` def (with or without receiver, per `defs`) whose enclosing
/// class/module/sclass is the search root's scope: `depth` counts scopes entered below it.
struct Finder {
    defs: bool,
    depth: i32,
    found: bool,
}

impl<'pr> Visit<'pr> for Finder {
    fn visit_def_node(&mut self, node: &ruby_prism::DefNode<'pr>) {
        if self.depth == 0 && node.receiver().is_some() == self.defs && node.name().as_slice() == b"respond_to_missing?" {
            self.found = true;
        }
        ruby_prism::visit_def_node(self, node);
    }
    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode<'pr>) {
        self.depth += 1;
        ruby_prism::visit_class_node(self, node);
        self.depth -= 1;
    }
    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode<'pr>) {
        self.depth += 1;
        ruby_prism::visit_module_node(self, node);
        self.depth -= 1;
    }
    fn visit_singleton_class_node(&mut self, node: &ruby_prism::SingletonClassNode<'pr>) {
        self.depth += 1;
        ruby_prism::visit_singleton_class_node(self, node);
        self.depth -= 1;
    }
}

fn is_scope(n: &Node) -> bool {
    matches!(kind::of(n), kind::CLASS_NODE | kind::MODULE_NODE | kind::SINGLETON_CLASS_NODE)
}

impl Cop for MissingRespondToMissing {
    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let def = node.as_def_node().unwrap();
        if def.name().as_slice() != b"method_missing" {
            return;
        }
        let anc = cx.ancestors;
        let scope = anc.iter().rev().find(|a| is_scope(a));
        let root = match scope {
            Some(s) => s,
            None => match helpers::parser_parent::parser_parent(anc) {
                Some(p) => p,
                None => {
                    let loc = node.location();
                    cx.add(loc.start_offset(), loc.end_offset(), MSG);
                    return;
                }
            },
        };
        let mut f = Finder { defs: def.receiver().is_some(), depth: if scope.is_some() { -1 } else { 0 }, found: false };
        f.visit(root);
        if !f.found && !respond_to_missing_elsewhere(anc, &def) {
            let loc = node.location();
            cx.add(loc.start_offset(), loc.end_offset(), MSG);
        }
    }
}

/// `respond_to_missing_elsewhere?`: with the project index, the class or module (or its singleton
/// class) defines `respond_to_missing?` in another file or definition.
fn respond_to_missing_elsewhere(anc: &[Node], def: &ruby_prism::DefNode) -> bool {
    let Some(index) = crate::project_index::get() else { return false };
    let Some(decl) = crate::project_index::enclosing_namespace(index, anc) else { return false };
    let ix = index.read();
    let singleton = def.receiver().is_some() || anc.iter().rev().find(|a| is_scope(a)).is_some_and(|a| a.as_singleton_class_node().is_some());
    let scope = if singleton { ix.singleton_of(decl) } else { Some(decl) };
    scope.and_then(|s| ix.member(s, "respond_to_missing?()")).is_some()
}

const MSG: &str = "When using `method_missing`, define `respond_to_missing?`.";
