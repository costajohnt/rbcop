use crate::prelude::*;
use helpers::capybara::{self as css, AttrValue};
use ruby_prism::CallNode;

pub const NAME: &str = "Capybara/SpecificFinders";
pub static COP: SpecificFinders = SpecificFinders;
pub struct SpecificFinders;

/// SpecificFinders#ruby_literal of a String: single-quoted with `'` and `\` escaped.
fn quoted(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// SpecificFinders#ruby_literal
fn ruby_literal(v: &AttrValue) -> String {
    match v {
        AttrValue::Str(s) => quoted(s),
        AttrValue::Nil => "nil".into(),
        v => v.to_s(),
    }
}

/// SpecificFindersHelp.field_selector?: an `input[...]` selector `find_field` can take as options.
fn field_selector(arg: &str) -> bool {
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\Ainput(?:\[.+\])+\z").unwrap());
    if !RE.is_match(arg) {
        return false;
    }
    let attrs = css::attributes(arg);
    let ty = css::attr(&attrs, "type").map(|t| t.to_s().to_lowercase()).unwrap_or_default();
    if matches!(ty.as_str(), "hidden" | "image" | "submit") {
        return false;
    }
    let keys = || attrs.iter().map(|(k, _)| k.as_deref());
    if keys().any(|k| matches!(k, Some("checked" | "disabled" | "multiple" | "readonly"))) {
        return false;
    }
    let allowed = css::specific_options("field");
    css::replaceable_attributes(&attrs) && keys().all(|k| k.is_some_and(|k| allowed.contains(&k)))
}

/// Pairs as `key: literal` options.
fn options<'a>(attrs: impl Iterator<Item = &'a (Option<String>, AttrValue)>) -> String {
    attrs.map(|(k, v)| format!("{}: {}", k.as_deref().unwrap_or(""), ruby_literal(v))).collect::<Vec<_>>().join(", ")
}

impl Cop for SpecificFinders {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send _ :find $(sym {:css :id :link :field})? (str $_) ...)
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "find" {
            return;
        }
        let args = send_args(&call);
        let sym = args.first().and_then(|a| css::sym(cx, a)).filter(|s| matches!(s.as_str(), "css" | "id" | "link" | "field"));
        let Some(str) = args.get(usize::from(sym.is_some())).filter(|a| helpers::rspec::is_str(a)) else { return };
        let arg = String::from_utf8_lossy(str.as_string_node().unwrap().unescaped()).into_owned();
        let has_sym = sym.is_some();
        match sym.as_deref() {
            // on_sym_id
            Some("id") => return register(cx, &call, true, quoted(&arg), Vec::new()),
            // on_sym_selector
            Some(s @ ("link" | "field")) => {
                let replacement = format!("find_{s}");
                let sel = call.message_loc().unwrap();
                let (rs, re) = (args[0].location().start_offset(), args[1].location().start_offset());
                let (s, e) = offense_range(&call);
                return cx.add_fix(s, e, format!("Prefer `{replacement}` over `find`."), |c| {
                    c.replace(sel.start_offset(), sel.end_offset(), &replacement);
                    c.remove(rs, re);
                });
            }
            _ => {}
        }
        if !css::pseudo_classes(&arg).is_empty() || css::multiple_selectors(&arg) {
            return;
        }
        let attrs = css::attributes(&arg);
        if field_selector(&arg) {
            // on_field
            let sel = call.message_loc().unwrap();
            let (fs, fe) = helpers::rspec::node_range(&args[0]);
            let delete = has_sym.then(|| (fe, args[1].location().end_offset()));
            let opts = options(attrs.iter());
            let (s, e) = offense_range(&call);
            return cx.add_fix(s, e, "Prefer `find_field` over `find`.", |c| {
                c.replace(sel.start_offset(), sel.end_offset(), "find_field");
                c.replace(fs, fe, &opts);
                if let Some((ds, de)) = delete {
                    c.remove(ds, de);
                }
            });
        }
        if css::is_attribute(&arg) && css::common_attributes(&arg) {
            // on_attr
            let Some(id) = css::attr(&attrs, "id").filter(|v| **v != AttrValue::Nil) else { return };
            if css::attr(&attrs, "class").is_some_and(AttrValue::truthy) {
                return;
            }
            let id = quoted(&id.to_s().replace('\\', ""));
            let opts = options(attrs.iter().filter(|(k, _)| k.as_deref() != Some("id")));
            let replacement = if opts.is_empty() { id } else { format!("{id}, {opts}") };
            return register(cx, &call, has_sym, replacement, Vec::new());
        }
        if let Some(id) = css::id(&arg) {
            // on_id
            if !attrs.is_empty() {
                return;
            }
            let classes = css::classes(&arg.replacen(&format!("#{id}"), "", 1));
            register(cx, &call, has_sym, quoted(&id.replace('\\', "")), classes);
        }
    }
}

/// offense_range: from the selector to the closing parenthesis (or the end of the send).
fn offense_range(call: &CallNode) -> (usize, usize) {
    let start = call.message_loc().unwrap().start_offset();
    (start, call.closing_loc().map_or(send_range(call).1, |c| c.end_offset()))
}

/// register_offense: `find_by_id`, the replacement as the first argument, and the classes as a
/// `class:` option.
fn register(cx: &mut Cx, call: &CallNode, sym: bool, replacement: String, mut classes: Vec<String>) {
    let (start, end) = offense_range(call);
    let msg = "Prefer `find_by_id` over `find`.";
    let args = send_args(call);
    // class_option: `(send _ :find ... (hash <(pair (sym :class) $_) ...>))`
    let class_option = args.last().and_then(|h| helpers::rails::hash_elements(h)).and_then(|pairs| {
        pairs.into_iter().find_map(|p| {
            let a = p.as_assoc_node()?;
            (css::sym(cx, &a.key()).as_deref() == Some("class")).then(|| a.value())
        })
    });
    // autocorrectable_classes?
    let autocorrectable = classes.is_empty()
        || class_option.as_ref().is_none_or(|o| {
            helpers::rspec::is_str(o)
                || o.as_array_node().is_some_and(|a| a.elements().iter().all(|e| helpers::rspec::is_str(&e)))
        });
    if !autocorrectable {
        return cx.add_fix(start, end, msg, |_| {});
    }
    let (fs, fe) = helpers::rspec::node_range(&args[0]);
    let inspect = |c: &[String]| format!("[{}]", c.iter().map(|s| helpers::ruby_string::inspect(s.as_bytes())).collect::<Vec<_>>().join(", "));
    // (replace?, range, text)
    let class_edit = if classes.is_empty() {
        None
    } else if let Some(o) = &class_option {
        // append_options
        if let Some(s) = o.as_string_node() {
            classes.push(String::from_utf8_lossy(s.unescaped()).into_owned());
        } else if let Some(a) = o.as_array_node() {
            classes.extend(a.elements().iter().map(|e| String::from_utf8_lossy(e.as_string_node().unwrap().unescaped()).into_owned()));
        }
        let (s, e) = helpers::rspec::node_range(o);
        Some((true, s, e, inspect(&classes)))
    } else {
        let value = if classes.len() > 1 { inspect(&classes) } else { format!("'{}'", classes[0]) };
        Some((false, fs, fe, format!(", class: {value}")))
    };
    let sel = call.message_loc().unwrap();
    let delete = sym.then(|| args.get(1).map(|a| (fe, a.location().end_offset()))).flatten();
    cx.add_fix(start, end, msg, |c| {
        c.replace(sel.start_offset(), sel.end_offset(), "find_by_id");
        c.replace(fs, fe, &replacement);
        if let Some((replace, s, e, text)) = &class_edit {
            if *replace {
                c.replace(*s, *e, text);
            } else {
                c.insert_after(*s, *e, text);
            }
        }
        if let Some((ds, de)) = delete {
            c.remove(ds, de);
        }
    });
}
