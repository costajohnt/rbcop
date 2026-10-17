use crate::prelude::*;
use ruby_prism::{CallNode, Visit};

pub const NAME: &str = "FactoryBot/AssociationStyle";
pub static COP: AssociationStyle = AssociationStyle;
pub struct AssociationStyle;

const KEYWORDS: &[&str] = &[
    "alias", "and", "begin", "break", "case", "class", "def", "defined?", "do", "else", "elsif", "end", "ensure", "false",
    "for", "if", "in", "module", "next", "nil", "not", "or", "redo", "rescue", "retry", "return", "self", "super", "then",
    "true", "undef", "unless", "until", "when", "while", "yield", "__FILE__", "__LINE__", "__ENCODING__",
];

static NAME_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A[\p{L}_][\p{L}\p{N}_]*[?!]?\z").unwrap());

impl Cop for AssociationStyle {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || !matches!(method_name(&call), "factory" | "trait") {
            return;
        }
        // children_of_factory_block: the statements of the block attached to this send
        let Some(block) = helpers::rspec::block_of(&call) else { return };
        let Some(body) = helpers::rspec::parser_body(block.body()) else { return };
        let children = helpers::rspec::begin_children(&body).filter(|_| body.as_statements_node().is_some()).unwrap_or_else(|| vec![body]);
        let explicit = cx.cfg.str("EnforcedStyle") == Some("explicit");
        let msg = format!("Use {} style to define associations.", cx.cfg.str("EnforcedStyle").unwrap_or_default());
        let target = cx.src.target_ruby;
        let sym = |n: &Node| (n.as_symbol_node().is_some() && !is_parser_dsym(n, target)).then(|| helpers::rails::sym_value(n)).flatten();
        for child in &children {
            let Some(c) = helpers::rspec::send_call(child).filter(|c| !is_csend(c) && c.receiver().is_none()) else { continue };
            let name = method_name(&c);
            let args = send_args(&c);
            if explicit {
                // implicit_association?: a receiver-less call that is not a reserved method
                let mut non_implicit: Vec<String> = helpers::factory_bot::RESERVED_METHODS.iter().map(|s| s.to_string()).collect();
                non_implicit.extend(cx.cfg.strings("NonImplicitAssociationMethodNames"));
                if non_implicit.iter().any(|n| n == name) {
                    continue;
                }
                // trait_factory_node: the outermost `factory` block around (with this send's block)
                let anc = cx.ancestors;
                let factory = anc
                    .iter()
                    .chain(std::iter::once(node))
                    .find(|a| helpers::rspec::plain_block_call(a).is_some_and(|f| method_name(&f) == "factory"));
                let Some(factory) = factory else { continue };
                // trait_within_trait?: a `trait :name` in that factory
                struct T<'a>(&'a str, bool, f64);
                impl<'pr> Visit<'pr> for T<'_> {
                    fn visit_call_node(&mut self, n: &CallNode<'pr>) {
                        if n.receiver().is_none()
                            && method_name(n) == "trait"
                            && let [a] = &send_args(n)[..]
                            && a.as_symbol_node().is_some()
                            && !is_parser_dsym(a, self.2)
                            && helpers::rails::sym_value(a).as_deref() == Some(self.0)
                        {
                            self.1 = true;
                        }
                        ruby_prism::visit_call_node(self, n);
                    }
                }
                let mut t = T(name, false, cx.src.target_ruby);
                t.visit(factory);
                if t.1 {
                    continue;
                }
                // autocorrect_to_explicit_style
                let mut parts = vec![format!(":{name}")];
                parts.extend(args.iter().map(|a| node_text(a).to_string()));
                let replacement = format!("association {}", parts.join(", "));
                let (s, e) = send_range(&c);
                cx.add_fix(s, e, msg.clone(), |cr| cr.replace(s, e, &replacement));
                continue;
            }
            // explicit_association?: (send nil? :association sym ...)
            if name != "association" || args.first().is_none_or(|a| sym(a).is_none()) {
                continue;
            }
            // with_strategy_build_option?: a last hash with `strategy: :build`
            let last_hash = args.last().filter(|_| args.len() > 1).and_then(helpers::rails::hash_elements);
            let strategy_build = last_hash.as_ref().is_some_and(|ps| {
                ps.iter().filter_map(|p| p.as_assoc_node()).any(|p| {
                    sym(&p.key()).as_deref() == Some("strategy") && sym(&p.value()).as_deref() == Some("build")
                })
            });
            if strategy_build {
                continue;
            }
            // keyword?: an association argument (anywhere in it) that is a keyword symbol
            struct K<'a>(bool, f64, &'a [&'a str]);
            impl<'pr> Visit<'pr> for K<'_> {
                fn visit_call_node(&mut self, n: &CallNode<'pr>) {
                    if n.receiver().is_none() && method_name(n) == "association" {
                        for a in send_args(n) {
                            if a.as_symbol_node().is_some()
                                && !is_parser_dsym(&a, self.1)
                                && helpers::rails::sym_value(&a).is_some_and(|v| self.2.contains(&v.as_str()))
                            {
                                self.0 = true;
                            }
                        }
                    }
                    ruby_prism::visit_call_node(self, n);
                }
            }
            let mut k = K(false, cx.src.target_ruby, KEYWORDS);
            k.visit(child);
            if k.0 {
                continue;
            }
            // autocorrect_to_implicit_style
            let first = sym(&args[0]).unwrap_or_default();
            let mut inert = NAME_RE.is_match(&first);
            let mut options: Vec<(String, String)> = Vec::new();
            if let Some(ps) = args.last().and_then(helpers::rails::hash_elements) {
                for p in ps.iter().filter_map(|p| p.as_assoc_node()) {
                    let key = p.key();
                    let kv = match sym(&key) {
                        Some(v) => v,
                        None if helpers::rspec::is_str(&key) => String::from_utf8_lossy(key.as_string_node().unwrap().unescaped()).into_owned(),
                        // `pair.key.value` of anything else raises
                        None => {
                            inert = false;
                            node_text(&key).to_string()
                        }
                    };
                    inert &= NAME_RE.is_match(&kv);
                    let value = helpers::rails::pair_value_source(&p).to_string();
                    match options.iter_mut().find(|(k, _)| *k == kv) {
                        Some(o) => o.1 = value,
                        None => options.push((kv, value)),
                    }
                }
            }
            // factory_names_from_explicit: the factory option's names, then trait symbols
            let traits: Vec<String> = args.iter().skip(1).map_while(sym).collect();
            let mut factories: Vec<String> = Vec::new();
            if let Some(ps) = args.last().filter(|_| args.len() > 1).and_then(helpers::rails::hash_elements) {
                for p in ps.iter().filter_map(|p| p.as_assoc_node()) {
                    if sym(&p.key()).as_deref() != Some("factory") {
                        continue;
                    }
                    let v = p.value();
                    if let Some(s) = sym(&v) {
                        factories = vec![s];
                    } else if let Some(a) = v.as_array_node() {
                        let names: Option<Vec<String>> = a.elements().iter().map(|e| sym(&e)).collect();
                        if let Some(n) = names {
                            factories = n;
                        }
                    }
                    break;
                }
            }
            let mut names = factories.clone();
            names.extend(traits.iter().cloned());
            if factories.is_empty() && !traits.is_empty() {
                names.insert(0, first.clone());
            }
            if !names.is_empty() {
                inert &= names.iter().all(|n| NAME_RE.is_match(n));
                let value = format!("%i[{}]", names.join(" "));
                match options.iter_mut().find(|(k, _)| k == "factory") {
                    Some(o) => o.1 = value,
                    None => options.push(("factory".into(), value)),
                }
            }
            let mut source = first.clone();
            if !options.is_empty() {
                source.push(' ');
                source.push_str(&options.iter().map(|(k, v)| format!("{k}: {v}")).collect::<Vec<_>>().join(", "));
            }
            let (s, e) = send_range(&c);
            // The names become method calls, labels and %i[] items: only plain ones are written.
            if !inert {
                cx.add_withheld_fix(s, e, msg.clone());
                continue;
            }
            cx.add_fix(s, e, msg.clone(), |cr| cr.replace(s, e, &source));
        }
    }
}
