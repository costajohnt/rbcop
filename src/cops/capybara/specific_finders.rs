use crate::prelude::*;
use helpers::capybara as css;
use ruby_prism::{CallNode, Visit};

pub const NAME: &str = "Capybara/SpecificFinders";
pub static COP: SpecificFinders = SpecificFinders;
pub struct SpecificFinders;

impl Cop for SpecificFinders {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send _ :find $(sym {:css :id})? (str $_) ...)
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "find" {
            return;
        }
        let args = send_args(&call);
        let sym = args
            .first()
            .filter(|a| helpers::rspec::is_sym(cx, a) && matches!(helpers::rails::sym_value(a).as_deref(), Some("css" | "id")));
        let Some(str) = args.get(usize::from(sym.is_some())).filter(|a| helpers::rspec::is_str(a)) else { return };
        let arg = String::from_utf8_lossy(str.as_string_node().unwrap().unescaped()).into_owned();
        if !css::pseudo_classes(&arg).is_empty() || css::multiple_selectors(&arg) {
            return;
        }
        if css::is_attribute(&arg) && css::common_attributes(&arg) {
            // on_attr
            let attrs = css::attributes(&arg);
            let get = |k: &str| attrs.iter().find(|(a, _)| a.as_deref() == Some(k)).map(|(_, v)| v.clone());
            if let Some(id) = get("id").filter(css::AttrValue::truthy)
                && !get("class").is_some_and(|c| c.truthy())
            {
                let options: Vec<String> = attrs
                    .iter()
                    .filter(|(k, _)| k.as_deref() != Some("id"))
                    .map(|(k, v)| format!("{}: {}", k.clone().unwrap_or_default(), v.to_s()))
                    .collect();
                let id = if options.is_empty() { id.to_s() } else { format!("{}, {}", id.to_s(), options.join(", ")) };
                register(cx, &call, sym.is_some(), &id, Vec::new(), true);
            }
        }
        if css::is_id(&arg) && css::attributes(&arg).is_empty() {
            let id = css::id(&arg).unwrap();
            let classes = css::classes(&arg.replacen(&format!("#{id}"), "", 1));
            register(cx, &call, sym.is_some(), &format!("'{id}'"), classes, !id.contains('\''));
        }
        if sym.is_some_and(|s| helpers::rails::sym_value(s).as_deref() == Some("id")) {
            register(cx, &call, true, &format!("'{arg}'"), Vec::new(), !arg.contains('\''));
        }
    }
}

/// A class option value's inspected element (`append_options`): a string, symbol or number.
fn inspect_value(cx: &Cx, v: &Node) -> Option<String> {
    if helpers::rspec::is_str(v) {
        return Some(helpers::ruby_string::inspect(v.as_string_node().unwrap().unescaped()));
    }
    if helpers::rspec::is_sym(cx, v) {
        return Some(helpers::ruby_string::symbol_inspect(v.as_symbol_node().unwrap().unescaped()));
    }
    if v.as_integer_node().is_some() {
        return int_literal_value(v).map(|i| i.to_string());
    }
    None
}

/// register_offense: `find_by_id`, the id as the first argument, and the classes as a `class:` option.
/// `inert`: the id text written between single quotes has no quote of its own (backslashes are
/// deleted from it); rbcop withholds the correction otherwise.
fn register(cx: &mut Cx, call: &CallNode, sym: bool, id: &str, classes: Vec<String>, inert: bool) {
    let sel = call.message_loc().unwrap();
    let start = sel.start_offset();
    let end = call.closing_loc().map_or(send_range(call).1, |c| c.end_offset());
    let msg = "Prefer `find_by_id` over `find`.";
    let args = send_args(call);
    let id = id.replace('\\', "");
    if !inert {
        return cx.add_withheld_fix(start, end, msg);
    }
    let mut items: Vec<String> = classes.iter().map(|c| helpers::ruby_string::inspect(c.as_bytes())).collect();
    // (insert after the first argument?, range, text)
    let mut class_edit: Option<(bool, usize, usize, String)> = None;
    if !classes.is_empty() {
        // class_options: the first `class:` pair in the send (receiver and arguments)
        struct P<'pr>(Option<Node<'pr>>, f64);
        impl<'pr> Visit<'pr> for P<'pr> {
            fn visit_assoc_node(&mut self, n: &ruby_prism::AssocNode<'pr>) {
                let k = n.key();
                if self.0.is_none() && k.as_symbol_node().is_some_and(|s| s.unescaped() == b"class") && !is_parser_dsym(&k, self.1) {
                    self.0 = Some(n.value());
                }
                ruby_prism::visit_assoc_node(self, n);
            }
        }
        let mut p = P(None, cx.src.target_ruby);
        if let Some(r) = call.receiver() {
            p.visit(&r);
        }
        if let Some(a) = call.arguments() {
            p.visit_arguments_node(&a);
        }
        match p.0 {
            Some(options) => {
                if helpers::rspec::is_str(&options) {
                    items.push(inspect_value(cx, &options).unwrap());
                } else if let Some(a) = options.as_array_node() {
                    for e in a.elements().iter() {
                        let Some(i) = inspect_value(cx, &e) else { return cx.add_withheld_fix(start, end, msg) };
                        items.push(i);
                    }
                }
                let (s, e) = helpers::rspec::node_range(&options);
                class_edit = Some((false, s, e, format!("[{}]", items.join(", "))));
            }
            None => {
                let value = if classes.len() > 1 { format!("[{}]", items.join(", ")) } else { format!("'{}'", classes[0]) };
                let (s, e) = helpers::rspec::node_range(&args[0]);
                class_edit = Some((true, s, e, format!(", class: {value}")));
            }
        }
    }
    let (fs, fe) = helpers::rspec::node_range(&args[0]);
    let delete_end = args.get(1).map(|a| a.location().end_offset());
    cx.add_fix(start, end, msg, |c| {
        c.replace(sel.start_offset(), sel.end_offset(), "find_by_id");
        c.replace(fs, fe, &id);
        if let Some((insert, s, e, text)) = &class_edit {
            if *insert {
                c.insert_after(*s, *e, text);
            } else {
                c.replace(*s, *e, text);
            }
        }
        if sym && let Some(de) = delete_end {
            c.remove(fe, de);
        }
    });
}
