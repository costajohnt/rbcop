use crate::helpers::configurable_naming as naming;
use crate::prelude::*;

pub const NAME: &str = "Naming/MethodName";
pub static COP: MethodName = MethodName;
pub struct MethodName;

/// `sym`/`str` value (parser `(sym $_)` / `(str $_)`).
fn literal_name(node: &Node) -> Option<String> {
    match node {
        Node::SymbolNode { .. } => Some(String::from_utf8_lossy(node.as_symbol_node()?.unescaped()).into_owned()),
        Node::StringNode { .. } if !is_parser_dstr(node) => {
            Some(String::from_utf8_lossy(node.as_string_node()?.unescaped()).into_owned())
        }
        _ => None,
    }
}

fn range(node: &Node) -> (usize, usize) {
    let l = node.location();
    (l.start_offset(), l.end_offset())
}

/// `(send (const {nil? cbase} :Name) ...)`
fn const_receiver(call: &ruby_prism::CallNode, name: &str) -> bool {
    match call.receiver() {
        Some(Node::ConstantReadNode { .. }) => node_text(&call.receiver().unwrap()) == name,
        Some(Node::ConstantPathNode { .. }) => {
            let p = call.receiver().unwrap();
            let p = p.as_constant_path_node().unwrap();
            p.parent().is_none() && text(&p.name_loc()) == name
        }
        _ => false,
    }
}

impl MethodName {
    fn style<'a>(&self, cx: &'a Cx) -> &'a str {
        cx.cfg.str("EnforcedStyle").unwrap_or("snake_case")
    }

    fn check_name(&self, cx: &mut Cx, name: &str, (start, end): (usize, usize)) {
        let style = self.style(cx);
        if naming::naming_valid(style, name) {
            cx.correct_style_detected();
        } else {
            let msg = format!("Use {style} for method names.");
            naming::report_opposing_styles(cx, start, end, |s| naming::naming_valid(s, name));
            cx.add(start, end, msg);
        }
    }

    fn forbidden(&self, cx: &mut Cx, name: &str, (start, end): (usize, usize)) {
        cx.add(start, end, format!("`{name}` is forbidden, use another method name instead."));
    }

    /// handle_method_name: `forbidden_at` is where register_forbidden_name reports.
    fn handle_method_name(&self, cx: &mut Cx, name: &str, at: (usize, usize), forbidden_at: (usize, usize)) {
        if helpers::allowed::matches_allowed_pattern(cx.cfg, name) {
            return;
        }
        if naming::forbidden_name(cx.cfg, name) {
            self.forbidden(cx, name, forbidden_at);
        } else if !helpers::node_types::is_operator_method(name) {
            self.check_name(cx, name, at);
        }
    }

    fn on_send(&self, cx: &mut Cx, call: &ruby_prism::CallNode) {
        let method = method_name(call);
        let args = call_args(call);
        // range_position(send): from one past the selector to the end of the send
        let after_selector = || {
            let start = call.message_loc().map_or(0, |m| m.end_offset() + 1);
            (start, send_range(call).1)
        };
        match method {
            "define_method" | "define_singleton_method" => {
                let Some(first) = args.first() else { return };
                if let Some(name) = literal_name(first) {
                    self.handle_method_name(cx, &name, after_selector(), range(first));
                }
            }
            "new" if const_receiver(call, "Struct") => {
                let skip = args.first().is_some_and(|a| matches!(a, Node::StringNode { .. }) && !is_parser_dstr(a));
                for arg in args.iter().skip(skip as usize) {
                    if let Some(name) = literal_name(arg) {
                        self.handle_method_name(cx, &name, range(arg), range(arg));
                    }
                }
            }
            "define" if const_receiver(call, "Data") => {
                for arg in &args {
                    if let Some(name) = literal_name(arg) {
                        self.handle_method_name(cx, &name, range(arg), range(arg));
                    }
                }
            }
            "alias_method" => {
                if args.len() == 2
                    && let Some(name) = literal_name(&args[0])
                {
                    self.handle_method_name(cx, &name, range(&args[0]), range(&args[0]));
                }
            }
            "attr" | "attr_reader" | "attr_writer" | "attr_accessor" => {
                if call.receiver().is_some() || args.is_empty() {
                    return;
                }
                let last = args.last().unwrap();
                let last_name = literal_name(last).unwrap_or_default();
                for arg in &args {
                    let Some(name) = literal_name(arg) else { continue };
                    if helpers::allowed::matches_allowed_pattern(cx.cfg, &name) {
                        continue;
                    }
                    if naming::forbidden_name(cx.cfg, &name) {
                        self.forbidden(cx, &last_name, range(last));
                    } else {
                        self.check_name(cx, &name, after_selector());
                    }
                }
            }
            _ => {}
        }
    }
}

impl Cop for MethodName {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE, kind::DEF_NODE, kind::ALIAS_METHOD_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        match node {
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                if !is_csend(&call) {
                    self.on_send(cx, &call);
                }
            }
            Node::DefNode { .. } => {
                let def = node.as_def_node().unwrap();
                let name = String::from_utf8_lossy(def.name().as_slice()).into_owned();
                let loc = def.name_loc();
                let at = (loc.start_offset(), loc.end_offset());
                if helpers::node_types::is_operator_method(&name) || helpers::allowed::matches_allowed_pattern(cx.cfg, &name) {
                    return;
                }
                if naming::forbidden_name(cx.cfg, &name) {
                    return self.forbidden(cx, &name, at);
                }
                if def.receiver().is_some() && naming::class_emitter_method(cx.ancestors, &name) {
                    return cx.correct_style_detected();
                }
                self.check_name(cx, &name, at);
            }
            _ => {
                let alias = node.as_alias_method_node().unwrap();
                let new = alias.new_name();
                if let Some(name) = new.as_symbol_node().map(|s| String::from_utf8_lossy(s.unescaped()).into_owned()) {
                    self.handle_method_name(cx, &name, range(&new), range(&new));
                }
            }
        }
    }
}
