use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Gemspec/RequiredRubyVersion";
pub static COP: RequiredRubyVersion = RequiredRubyVersion;
pub struct RequiredRubyVersion;

#[derive(Default)]
struct State {
    found: bool,
}

/// `node.each_descendant(:send, *VARIABLES).any?` (the node itself included for variables).
struct Dynamic {
    /// Range of the node itself (not its own descendant).
    root: (usize, usize),
    hit: bool,
}

impl<'pr> Visit<'pr> for Dynamic {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'pr>) {
        if (node.location().start_offset(), node.location().end_offset()) != self.root && !is_csend(node) {
            self.hit = true;
        }
        ruby_prism::visit_call_node(self, node);
    }
    fn visit_local_variable_read_node(&mut self, _: &ruby_prism::LocalVariableReadNode<'pr>) {
        self.hit = true;
    }
    fn visit_it_local_variable_read_node(&mut self, _: &ruby_prism::ItLocalVariableReadNode<'pr>) {
        self.hit = true;
    }
    fn visit_instance_variable_read_node(&mut self, _: &ruby_prism::InstanceVariableReadNode<'pr>) {
        self.hit = true;
    }
    fn visit_class_variable_read_node(&mut self, _: &ruby_prism::ClassVariableReadNode<'pr>) {
        self.hit = true;
    }
    fn visit_global_variable_read_node(&mut self, _: &ruby_prism::GlobalVariableReadNode<'pr>) {
        self.hit = true;
    }
}

fn dynamic_version(node: &Node) -> bool {
    if node.as_call_node().is_some_and(|c| !is_csend(&c) && c.receiver().is_none()) {
        return true;
    }
    let mut v = Dynamic { root: (node.location().start_offset(), node.location().end_offset()), hit: false };
    v.visit(node);
    v.hit
}

fn str_content(node: &Node) -> Option<String> {
    node.as_string_node().map(|s| String::from_utf8_lossy(s.unescaped()).into_owned())
}

/// `defined_ruby_version` + `extract_ruby_version`.
fn ruby_version(node: &Node) -> Option<String> {
    let detect = |strs: Vec<String>| strs.into_iter().find(|s| s.contains(['>', '=']));
    let content = if let Some(s) = str_content(node) {
        s
    } else if let Some(a) = node.as_array_node() {
        // (array (str _) (str _))
        let strs: Vec<String> = a.elements().iter().filter_map(|e| str_content(&e)).collect();
        if a.elements().iter().count() != 2 || strs.len() != 2 {
            return None;
        }
        detect(strs)?
    } else {
        // (send (const (const nil? :Gem) :Requirement) :new $str+)
        let c = node.as_call_node()?;
        let recv = c.receiver()?;
        let path = recv.as_constant_path_node()?;
        let gem = path.parent()?;
        if is_csend(&c)
            || method_name(&c) != "new"
            || c.block().is_some()
            || path.name()?.as_slice() != b"Requirement"
            || gem.as_constant_read_node()?.name().as_slice() != b"Gem"
        {
            return None;
        }
        let args = call_args(&c);
        let strs: Vec<String> = args.iter().filter_map(str_content).collect();
        if strs.is_empty() || strs.len() != args.len() {
            return None;
        }
        detect(strs)?
    };
    Some(content.chars().filter(char::is_ascii_digit).take(2).map(String::from).collect::<Vec<_>>().join("."))
}

impl Cop for RequiredRubyVersion {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "required_ruby_version=" {
            return;
        }
        let args = call_args(&call);
        // required_ruby_version? search: (send _ :required_ruby_version= _)
        if !is_csend(&call) && send_args(&call).len() == 1 {
            cx.state::<State>().found = true;
        }
        if is_csend(&call) {
            return;
        }
        let Some(version_def) = args.first() else { return };
        if dynamic_version(version_def) {
            return;
        }
        let target = cx.config.target_ruby;
        if ruby_version(version_def).is_some_and(|v| v == format!("{target:?}")) {
            return;
        }
        let loc = version_def.location();
        let msg = format!(
            "`required_ruby_version` and `TargetRubyVersion` ({target:?}, which may be specified in .rubocop.yml) should be equal."
        );
        cx.add(loc.start_offset(), loc.end_offset(), msg);
    }

    fn on_file(&self, cx: &mut Cx) {
        if !cx.state::<State>().found {
            // add_global_offense
            cx.add_global("`required_ruby_version` should be specified.");
        }
    }
}
