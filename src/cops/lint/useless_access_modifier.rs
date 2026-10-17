use crate::prelude::*;
use helpers::variable_force::{Tree, T};

type Id = usize;

pub const NAME: &str = "Lint/UselessAccessModifier";
pub static COP: UselessAccessModifier = UselessAccessModifier;
pub struct UselessAccessModifier;

fn any_block(t: T) -> bool {
    matches!(t, T::Block | T::Numblock | T::Itblock)
}

struct Ctx<'a> {
    tree: &'a Tree,
    text: &'a str,
    active_support: bool,
    method_creating: Vec<String>,
    context_creating: Vec<String>,
    /// (start, end, message, removal range)
    hits: Vec<(usize, usize, String, (usize, usize))>,
}

impl Ctx<'_> {
    fn send(&self, n: Id) -> bool {
        self.tree.t(n) == T::Send
    }

    fn name(&self, n: Id) -> &str {
        &self.tree.nodes[n].name
    }

    fn receiver(&self, n: Id) -> Option<Id> {
        self.tree.child(n, 0)
    }

    fn has_args(&self, n: Id) -> bool {
        self.tree.nodes[n].nargs > 0
    }

    fn is_const(&self, n: Id) -> bool {
        matches!(self.tree.nodes[n].kind, kind::CONSTANT_READ_NODE | kind::CONSTANT_PATH_NODE)
    }

    /// `(const {nil? cbase} name)`
    fn global_const(&self, n: Option<Id>, names: &[&str]) -> bool {
        n.is_some_and(|c| {
            let (s, e) = self.tree.range(c);
            let text = &self.text[s..e];
            self.is_const(c) && names.contains(&text.strip_prefix("::").unwrap_or(text))
        })
    }

    fn constructor_send(&self, n: Id) -> bool {
        self.send(n)
            && ((self.name(n) == "new" && self.global_const(self.receiver(n), &["Class", "Module", "Struct"]))
                || (self.name(n) == "define" && self.global_const(self.receiver(n), &["Data"])))
    }

    /// rubocop-ast `class_constructor?`.
    fn class_constructor(&self, n: Id) -> bool {
        self.constructor_send(n) || (any_block(self.tree.t(n)) && self.tree.child(n, 0).is_some_and(|s| self.constructor_send(s)))
    }

    /// rubocop-ast `in_macro_scope?`.
    fn in_macro_scope(&self, n: Id) -> bool {
        let t = self.tree;
        let Some(p) = t.parent(n) else { return true };
        match t.t(p) {
            T::Sclass | T::Class | T::Module => true,
            _ if self.class_constructor(p) => true,
            T::Kwbegin | T::Begin => self.in_macro_scope(p),
            pt if any_block(pt) => self.in_macro_scope(p),
            T::If => t.child(p, 0) != Some(n) && self.in_macro_scope(p),
            _ => false,
        }
    }

    fn bare_access_modifier(&self, n: Id) -> bool {
        self.send(n)
            && self.receiver(n).is_none()
            && !self.has_args(n)
            && matches!(self.name(n), "public" | "protected" | "private" | "module_function")
            && self.in_macro_scope(n)
    }

    fn access_modifier(&self, n: Id) -> bool {
        self.bare_access_modifier(n) || self.name(n) == "private_class_method"
    }

    fn block_method(&self, n: Id, name: &str) -> bool {
        self.tree.child(n, 0).is_some_and(|s| self.tree.t(s) == T::Send && self.name(s) == name)
    }

    fn included_block(&self, n: Id) -> bool {
        self.active_support && self.block_method(n, "included")
    }

    fn bare_send(&self, n: Id, names: &[&str]) -> bool {
        self.send(n) && self.receiver(n).is_none() && names.contains(&self.name(n))
    }

    fn method_definition(&self, n: Id) -> bool {
        let t = self.tree.t(n);
        t == T::Def
            || self.bare_send(n, &["attr", "attr_reader", "attr_writer", "attr_accessor"])
            || self.bare_send(n, &["define_method"])
            || (any_block(t) && self.tree.child(n, 0).is_some_and(|s| self.bare_send(s, &["define_method"])))
            || self.method_creating.iter().any(|m| m != "included" && self.bare_send(n, &[m.as_str()]))
    }

    fn eval_call(&self, n: Id) -> bool {
        let t = self.tree;
        let class_or_instance_eval = any_block(t.t(n))
            && t.child(n, 0).is_some_and(|s| {
                self.send(s) && !self.has_args(s) && matches!(self.name(s), "class_eval" | "instance_eval")
            });
        class_or_instance_eval
            || self.class_constructor(n)
            || (any_block(t.t(n))
                && self.context_creating.iter().any(|m| {
                    m != "included"
                        && t.child(n, 0).is_some_and(|s| {
                            self.send(s)
                                && self.receiver(s).is_none_or(|r| self.is_const(r))
                                && self.name(s) == m
                        })
                }))
    }

    fn start_of_new_scope(&self, n: Id) -> bool {
        matches!(self.tree.t(n), T::Module | T::Class | T::Sclass) || self.eval_call(n)
    }

    fn offense(&mut self, n: Id, vis: &str) {
        let (s, e) = self.tree.range(n);
        let range = helpers::whole_lines::range_by_whole_lines(self.text, s, e, true);
        self.hits.push((s, e, format!("Useless `{vis}` access modifier."), range));
    }

    fn check_node(&mut self, n: Option<Id>) {
        let Some(n) = n else { return };
        if self.tree.t(n) == T::Begin {
            self.check_scope(n);
        } else if self.bare_access_modifier(n) {
            let name = self.name(n).to_string();
            self.offense(n, &name);
        }
    }

    fn check_scope(&mut self, n: Id) {
        let (cur, unused) = self.check_child_nodes(n, None, Some("public".to_string()));
        if let Some(u) = unused {
            self.offense(u, cur.as_deref().unwrap_or(""));
        }
    }

    fn check_child_nodes(&mut self, n: Id, mut unused: Option<Id>, mut cur: Option<String>) -> (Option<String>, Option<Id>) {
        let kids: Vec<Id> = self.tree.nodes[n].children.iter().flatten().copied().collect();
        for child in kids {
            let t = self.tree.t(child);
            if self.send(child) && self.access_modifier(child) {
                (cur, unused) = self.check_send_node(child, cur, unused);
            } else if t == T::Block && self.included_block(child) {
                continue;
            } else if self.method_definition(child) {
                unused = None;
            } else if self.start_of_new_scope(child) {
                self.check_scope(child);
            } else if t != T::Defs {
                (cur, unused) = self.check_child_nodes(child, unused, cur);
            }
        }
        (cur, unused)
    }

    /// Returns `[cur_vis, unused]`, or (nil, nil) when neither branch applies (Ruby's implicit nil).
    fn check_send_node(&mut self, n: Id, cur: Option<String>, unused: Option<Id>) -> (Option<String>, Option<Id>) {
        if self.bare_access_modifier(n) {
            let new = self.name(n).to_string();
            if Some(&new) == cur.as_ref() {
                self.offense(n, &new);
                (Some(new), unused)
            } else {
                if let Some(u) = unused {
                    self.offense(u, cur.as_deref().unwrap_or(""));
                }
                (Some(new), Some(n))
            }
        } else if self.name(n) == "private_class_method" && !self.has_args(n) {
            self.offense(n, "private_class_method");
            (cur, unused)
        } else {
            (None, None)
        }
    }
}

impl Cop for UselessAccessModifier {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::PROGRAM_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let tree = Tree::shared(&node.as_program_node().unwrap(), cx.src);
        let mut ctx = Ctx {
            tree,
            text: cx.src.text,
            active_support: cx.config.all_cops().truthy("ActiveSupportExtensionsEnabled"),
            method_creating: cx.cfg.strings("MethodCreatingMethods"),
            context_creating: cx.cfg.strings("ContextCreatingMethods"),
            hits: Vec::new(),
        };
        let Some(root) = tree.root else { return };
        let mut all = Vec::new();
        tree.each_node(root, &mut |n| all.push(n));
        for n in all {
            match tree.t(n) {
                T::Class => ctx.check_node(tree.child(n, 2)),
                T::Module | T::Sclass => ctx.check_node(tree.child(n, 1)),
                t if any_block(t) => {
                    if ctx.eval_call(n) || ctx.included_block(n) {
                        ctx.check_node(tree.child(n, 2));
                    }
                }
                T::Begin if n == root => {
                    let kids: Vec<Id> = tree.nodes[n].children.iter().flatten().copied().collect();
                    for child in kids {
                        if ctx.send(child) && ctx.access_modifier(child) {
                            let name = ctx.name(child).to_string();
                            ctx.check_send_node(child, Some(name), Some(child));
                        }
                    }
                }
                _ => {}
            }
        }
        for (s, e, msg, (rs, re)) in std::mem::take(&mut ctx.hits) {
            cx.add_fix(s, e, msg, |c| c.remove(rs, re));
        }
    }
}
