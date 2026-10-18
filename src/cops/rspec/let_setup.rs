use crate::prelude::*;
use helpers::rspec::Language;
use ruby_prism::Visit;

pub const NAME: &str = "RSpec/LetSetup";
pub static COP: LetSetup = LetSetup;
pub struct LetSetup;

/// `(block {(send #rspec? {#SharedGroups.all #ExampleGroups.all} ...) (send nil? #Includes.all ...)} ...)`
fn group_or_including(l: &Language, node: &Node) -> bool {
    helpers::rspec::plain_block_call(node).is_some_and(|c| {
        let n = method_name(&c);
        (helpers::rspec::rspec_recv(&c) && (l.shared_group(n) || l.example_group(n))) || (c.receiver().is_none() && l.include(n))
    })
}

/// A `let!` name as RuboCop compares it: a symbol's or a string's value (never equal to each other).
#[derive(PartialEq)]
enum Name {
    Sym(String),
    Str(String),
}

/// let_bang: `{(block $(send nil? :let! {(sym $_) (str $_)}) ...) $(send nil? :let! {(sym $_) (str $_)} block_pass)}`
fn let_bang(cx: &Cx, node: &Node) -> Option<((usize, usize), Name)> {
    let call = node.as_call_node()?;
    if call.receiver().is_some() || method_name(&call) != "let!" {
        return None;
    }
    let args = send_args(&call);
    match helpers::rspec::block_of(&call) {
        Some(b) if helpers::rspec::is_plain(&b) && args.len() == 1 => {}
        None if args.len() == 2 && call.block().is_some() => {}
        _ => return None,
    }
    let name = if helpers::rspec::is_sym(cx, &args[0]) {
        Name::Sym(helpers::rails::sym_value(&args[0])?)
    } else if helpers::rspec::is_str(&args[0]) {
        Name::Str(String::from_utf8_lossy(args[0].as_string_node().unwrap().unescaped()).into_owned())
    } else {
        return None;
    };
    Some((send_range(&call), name))
}

impl Cop for LetSetup {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let l = helpers::rspec::lang(cx);
        if !group_or_including(&l, node) {
            return;
        }
        let lets = helpers::rspec::find_all_in_scope(&l, node, helpers::rspec::let_);
        for let_node in &lets {
            let Some((range, name)) = let_bang(cx, let_node) else { continue };
            // overrides_outer_let_bang?
            let overrides = cx.ancestors.iter().rev().any(|a| {
                group_or_including(&l, a)
                    && helpers::rspec::find_all_in_scope(&l, a, helpers::rspec::let_)
                        .iter()
                        .any(|o| let_bang(cx, o).is_some_and(|(_, n)| n == name))
            });
            if overrides {
                continue;
            }
            // method_called?: (send nil? %) anywhere in the group
            let wanted = match &name {
                Name::Sym(s) | Name::Str(s) => s.clone(),
            };
            struct V<'a>(&'a str, bool);
            impl<'pr> Visit<'pr> for V<'_> {
                fn visit_call_node(&mut self, n: &ruby_prism::CallNode<'pr>) {
                    if n.receiver().is_none() && method_name(n) == self.0 && send_args(n).is_empty() {
                        self.1 = true;
                    }
                    ruby_prism::visit_call_node(self, n);
                }
            }
            let mut v = V(&wanted, false);
            v.visit(node);
            if !v.1 {
                cx.add(range.0, range.1, "Do not use `let!` to setup objects not referenced in tests.");
            }
        }
    }
}
