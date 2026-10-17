use crate::prelude::*;

pub const NAME: &str = "RSpec/SpecFilePathFormat";
pub static COP: SpecFilePathFormat = SpecFilePathFormat;
pub struct SpecFilePathFormat;

static CAMEL1: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"([^A-Z])([A-Z]+)").unwrap());
static CAMEL2: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"([A-Z])([A-Z][^A-Z0-9]+)").unwrap());
static SPACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[ \t\n\x0b\x0c\r]").unwrap());
static NON_WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^A-Za-z0-9_]").unwrap());

/// DefaultInflector (ActiveSupport's `underscore` for `active_support`).
fn snake(name: &str, active_support: bool) -> String {
    if active_support {
        return helpers::inflector::underscore(name);
    }
    let s = CAMEL1.replace_all(name, "${1}_${2}");
    CAMEL2.replace_all(&s, "${1}_${2}").to_lowercase()
}

/// The names of the classes and modules a top-level node sits in (TopLevelGroup recurses into a
/// lone class or module).
fn namespace(program: &Node, target: (usize, usize)) -> Option<Vec<String>> {
    let mut out = Vec::new();
    let mut node = program.as_program_node()?.statements().as_node();
    loop {
        let items = helpers::rspec::body_children(Some(node));
        if items.len() != 1 {
            return Some(out);
        }
        let item = items.into_iter().next().unwrap();
        if helpers::rspec::node_range(&item) == target {
            return Some(out);
        }
        let body = match (item.as_module_node(), item.as_class_node()) {
            (Some(m), _) => m.body(),
            (_, Some(c)) => c.body(),
            _ => return Some(out),
        };
        out.extend(helpers::rails::defined_module_name(&item)?.split("::").map(String::from));
        node = body?;
    }
}

impl Cop for SpecFilePathFormat {
    fn kinds(&self) -> &'static [usize] {
        &[kind::PROGRAM_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let l = helpers::rspec::lang(cx);
        let groups = helpers::rspec::top_level_groups(&l, node);
        let [group] = &groups[..] else { return };
        // example_group_arguments: (block $(send #rspec? #ExampleGroups.all $_ $...) ...)
        let Some(call) = helpers::rspec::plain_block_call(group) else { return };
        if !helpers::rspec::rspec_recv(&call) || !l.example_group(method_name(&call)) {
            return;
        }
        let args = send_args(&call);
        let Some(class_name) = args.first() else { return };
        if !matches!(kind::of(class_name), kind::CONSTANT_READ_NODE | kind::CONSTANT_PATH_NODE) {
            return;
        }
        let rest = &args[1..];
        // ignore_metadata?: a `key: :value` pair the IgnoreMetadata setting lists
        let ignore = cx.cfg.get("IgnoreMetadata").and_then(Value::as_map).cloned().unwrap_or_default();
        let ignored = rest.iter().any(|a| {
            let mut found = false;
            helpers::rspec::each_sym_pair(cx, a, &mut |k, v| {
                found |= match ignore.get(k) {
                    Some(Value::Str(s)) => s == v,
                    Some(other) => other.as_string().as_deref() == Some(v),
                    None => false,
                }
            });
            found
        });
        if ignored {
            return;
        }
        let active_support = cx.cfg.str("EnforcedInflector") == Some("active_support");
        // ActiveSupportInflector.prepare_availability raises when InflectorPath does not exist.
        // ponytail: an existing inflections file is not evaluated; ActiveSupport's default
        // inflections apply (docs/known-gaps.md).
        if active_support && !cx.cfg.str("InflectorPath").is_some_and(|p| std::path::Path::new(p).exists()) {
            crate::runner::cop_error_at(cx.cop, cx.src, None);
            return;
        }
        let custom = cx.cfg.get("CustomTransform").and_then(Value::as_map).cloned().unwrap_or_default();
        let Some(mut names) = namespace(node, helpers::rspec::node_range(group)) else { return };
        names.extend(helpers::const_name::const_name(class_name).unwrap_or_default().split("::").map(String::from));
        let parts: Vec<String> = names
            .iter()
            .filter_map(|n| {
                let p = match custom.get(n) {
                    Some(v) => v.as_string().unwrap_or_default(),
                    None => snake(n, active_support),
                };
                (!p.is_empty()).then_some(p)
            })
            .collect();
        // method_name_pattern
        let method = rest.first().filter(|m| helpers::rspec::is_str(m) && cx.cfg.bool("IgnoreMethods") != Some(true)).map(|m| {
            let v = String::from_utf8_lossy(m.as_string_node().unwrap().unescaped()).into_owned();
            format!(".*{}", NON_WORD.replace_all(&SPACE.replace_all(&v, "_"), ""))
        });
        let pattern = format!("{}(?![[:alnum:]]){}[^/]*_spec\\.rb", parts.join("/"), method.unwrap_or_default());
        let path = cx.src.absolute_path();
        let Some(re) = crate::config::value::ruby_regex(&format!("(?:\\A|/){pattern}$")) else { return };
        if re.is_match(&path) {
            return;
        }
        let suffix = pattern.replacen("(?![[:alnum:]])", "", 1).replacen(".*", "*", 1).replacen("[^/]*", "*", 1).replacen("\\.", ".", 1);
        let (s, e) = send_range(&call);
        cx.add(s, e, format!("Spec path should end with `{suffix}`."));
    }
}
