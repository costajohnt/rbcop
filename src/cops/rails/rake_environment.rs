use crate::prelude::*;

pub const NAME: &str = "Rails/RakeEnvironment";
pub static COP: RakeEnvironment = RakeEnvironment;
pub struct RakeEnvironment;

/// A replacement: range and text.
type Edit = ((usize, usize), String);

static LABEL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A[A-Za-z_][A-Za-z0-9_]*\z").unwrap());

/// with_hash_style_dependencies?: the first pair's value is a non-empty array or anything else.
fn with_deps(hash: &Node) -> bool {
    let Some(p) = helpers::rails::hash_pairs(hash).into_iter().next() else { return false };
    match p.value().as_array_node() {
        Some(a) => a.elements().iter().next().is_some(),
        None => true,
    }
}

/// correct_empty_dependencies: `[]` as the first pair's value becomes `[:environment]`.
fn empty_deps(hash: &Node) -> Option<(usize, usize)> {
    let p = helpers::rails::hash_pairs(hash).into_iter().next()?;
    let v = p.value();
    let a = v.as_array_node()?;
    a.elements().iter().next().is_none().then(|| (v.location().start_offset(), v.location().end_offset()))
}

impl Cop for RakeEnvironment {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (block $(send nil? :task ...) ...)
        let Some(call) = helpers::rails::block_call(node) else { return };
        if !helpers::rails::is_command(&call, "task") || is_csend(&call) {
            return;
        }
        let args = send_args(&call);
        // task_name == :default
        let name = match args.first() {
            Some(a) if helpers::rails::str_or_sym(a).is_some() => helpers::rails::str_or_sym(a),
            Some(a) if a.as_hash_node().is_some() || a.as_keyword_hash_node().is_some() => {
                let elems = helpers::rails::hash_elements(a).unwrap();
                match elems.as_slice() {
                    [p] => p.as_assoc_node().and_then(|p| helpers::rails::str_or_sym(&p.key())),
                    _ => None,
                }
            }
            _ => None,
        };
        if name.as_deref() == Some("default") {
            return;
        }
        // with_dependencies?
        let deps = match args.first() {
            None => false,
            Some(a) if helpers::rails::hash_elements(a).is_some() => with_deps(a),
            Some(_) => args.get(1).is_some_and(|t| helpers::rails::hash_elements(t).is_some() && with_deps(t)),
        };
        if deps {
            return;
        }
        let (s, e) = send_range(&call);
        let range = |n: &Node| (n.location().start_offset(), n.location().end_offset());
        // autocorrect: (range, replacement), None for no edit; Err to withhold (non-label name).
        let fix: Result<Option<Edit>, ()> = match args.as_slice() {
            [] => Ok(None),
            [name] => {
                if helpers::rails::hash_elements(name).is_some() {
                    Ok(empty_deps(name).map(|r| (r, "[:environment]".to_string())))
                } else if name.as_symbol_node().is_some() {
                    let src: String = node_text(name).chars().filter(|c| !matches!(c, ':' | '|' | '\'' | '"')).collect();
                    // Deliberate difference: only a plain label is spliced in (docs/known-gaps.md).
                    if LABEL.is_match(&src) { Ok(Some((range(name), format!("{src}: :environment")))) } else { Err(()) }
                } else {
                    Ok(Some((range(name), format!("{} => :environment", node_text(name)))))
                }
            }
            [_, rest @ ..] => {
                if let [arg] = rest {
                    if helpers::rails::hash_elements(arg).is_some() {
                        Ok(empty_deps(arg).map(|r| (r, "[:environment]".to_string())))
                    } else if arg.as_array_node().is_some() {
                        Ok(Some((range(arg), format!("{} => :environment", node_text(arg)))))
                    } else if helpers::rails::str_or_sym(arg).is_some() {
                        Ok(Some((range(arg), format!("[{}] => :environment", node_text(arg)))))
                    } else {
                        Ok(None)
                    }
                } else if rest.iter().all(|a| helpers::rails::str_or_sym(a).is_some()) {
                    let srcs: Vec<&str> = rest.iter().map(|a| node_text(a)).collect();
                    let r = (rest[0].location().start_offset(), rest.last().unwrap().location().end_offset());
                    Ok(Some((r, format!("[{}] => :environment", srcs.join(", ")))))
                } else {
                    Ok(None)
                }
            }
        };
        let msg = "Include `:environment` task as a dependency for all Rake tasks.";
        match fix {
            Err(()) => cx.add_withheld_fix(s, e, msg),
            Ok(fix) => cx.add_fix(s, e, msg, |c| {
                if let Some(((a, b), text)) = &fix {
                    c.replace(*a, *b, text);
                }
            }),
        }
    }
}
