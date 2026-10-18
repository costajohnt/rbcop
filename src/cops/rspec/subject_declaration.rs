use crate::prelude::*;

pub const NAME: &str = "RSpec/SubjectDeclaration";
pub static COP: SubjectDeclaration = SubjectDeclaration;
pub struct SubjectDeclaration;

impl Cop for SubjectDeclaration {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send nil? ${#Subjects.all #Helpers.all} ({sym str} #Subjects.all) ...)
        let call = node.as_call_node().unwrap();
        // NodePattern checks the arity before it calls the predicates.
        let args = send_args(&call);
        let Some(first) = args.first() else { return };
        let l = helpers::rspec::lang(cx);
        let name = method_name(&call);
        if call.receiver().is_some() || !(l.subject(name) || l.helper(name)) {
            return;
        }
        let value = if helpers::rspec::is_sym(cx, first) {
            helpers::rails::sym_value(first)
        } else if helpers::rspec::is_str(first) {
            Some(String::from_utf8_lossy(first.as_string_node().unwrap().unescaped()).into_owned())
        } else {
            None
        };
        if !value.is_some_and(|v| l.subject(&v)) {
            return;
        }
        let msg = if l.helper(name) { "Use subject explicitly rather than using let" } else { "Ambiguous declaration of subject" };
        let sel = call.message_loc().unwrap();
        let (ss, se) = (sel.start_offset(), sel.end_offset());
        let new_name = name.replacen("let", "subject", 1);
        // first_argument_range: the parentheses with one argument (RuboCop raises without them)
        let remove = if args.len() == 1 {
            match (call.opening_loc(), call.closing_loc()) {
                (Some(o), Some(c)) => (o.start_offset(), c.end_offset()),
                _ => return,
            }
        } else {
            (first.location().start_offset(), args[1].location().start_offset())
        };
        let (s, e) = send_range(&call);
        cx.add_fix(s, e, msg, |c| {
            c.replace(ss, se, &new_name);
            c.remove(remove.0, remove.1);
        });
    }
}
