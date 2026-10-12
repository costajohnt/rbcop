use crate::prelude::*;

pub const NAME: &str = "Rails/MailerName";
pub static COP: MailerName = MailerName;
pub struct MailerName;

const MSG: &str = "Mailer should end with `Mailer` suffix.";

fn mailer_base(n: &Node) -> bool {
    helpers::rails::is_const(n, "ActionMailer::Base") || helpers::rails::is_const(n, "ApplicationMailer")
}

fn short_name(n: &Node) -> Option<String> {
    if let Some(c) = n.as_constant_read_node() {
        return Some(String::from_utf8_lossy(c.name().as_slice()).into_owned());
    }
    n.as_constant_path_node()?.name().map(|x| String::from_utf8_lossy(x.as_slice()).into_owned())
}

impl Cop for MailerName {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE, kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if let Some(class) = node.as_class_node() {
            // (class $(const _ !#mailer_suffix?) #mailer_base_class? ...)
            let path = class.constant_path();
            let Some(name) = short_name(&path) else { return };
            if name.ends_with("Mailer") || !class.superclass().is_some_and(|s| mailer_base(&s)) {
                return;
            }
            let (s, e) = (path.location().start_offset(), path.location().end_offset());
            let replacement = format!("{name}Mailer");
            cx.add_fix(s, e, MSG, |c| c.replace(s, e, &replacement));
            return;
        }
        // (send (const {nil? cbase} :Class) :new #mailer_base_class?)
        let Some(call) = helpers::rails::send(node) else { return };
        let args = send_args(&call);
        if method_name(&call) != "new"
            || !call.receiver().is_some_and(|r| helpers::rails::is_const(&r, "Class"))
            || !matches!(args.as_slice(), [a] if mailer_base(a))
        {
            return;
        }
        let Some(casgn) = cx.ancestors.iter().rev().find(|a| {
            a.as_constant_write_node().is_some() || a.as_constant_path_write_node().is_some()
        }) else {
            return;
        };
        let (name, loc) = match casgn.as_constant_write_node() {
            Some(w) => (String::from_utf8_lossy(w.name().as_slice()).into_owned(), w.name_loc()),
            None => {
                let t = casgn.as_constant_path_write_node().unwrap().target();
                let Some(n) = t.name() else { return };
                (String::from_utf8_lossy(n.as_slice()).into_owned(), t.name_loc())
            }
        };
        if name.ends_with("Mailer") {
            return;
        }
        let (s, e) = (loc.start_offset(), loc.end_offset());
        let replacement = format!("{name}Mailer");
        cx.add_fix(s, e, MSG, |c| c.replace(s, e, &replacement));
    }
}
