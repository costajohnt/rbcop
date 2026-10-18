use crate::prelude::*;

pub const NAME: &str = "RSpec/DescribeClass";
pub static COP: DescribeClass = DescribeClass;
pub struct DescribeClass;

static STRING_CONSTANT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^(?:(?:::)?[A-Z][A-Za-z0-9_]*)+$").unwrap());

impl Cop for DescribeClass {
    fn kinds(&self) -> &'static [usize] {
        &[kind::PROGRAM_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let l = helpers::rspec::lang(cx);
        let ignored = cx.cfg.get("IgnoredMetadata").and_then(Value::as_map).cloned().unwrap_or_default();
        helpers::rspec::each_top_level_group(&l, node, |_, group, _| {
            let call = group.as_call_node().unwrap();
            if is_csend(&call) || method_name(&call) != "describe" || !helpers::rspec::rspec_recv(&call) {
                return;
            }
            let args = send_args(&call);
            // example_group_with_ignored_metadata?: (send #rspec? :describe ... (hash <#ignored_metadata? ...>))
            if let Some(last) = args.last()
                && helpers::rails::hash_elements(last).is_some_and(|pairs| {
                    pairs.iter().filter_map(|p| p.as_assoc_node()).any(|p| {
                        let (k, v) = (p.key(), p.value());
                        helpers::rspec::is_sym(cx, &k)
                            && helpers::rspec::is_sym(cx, &v)
                            && ignored_includes(&ignored, &helpers::rails::sym_value(&k).unwrap(), &helpers::rails::sym_value(&v).unwrap())
                    })
                })
            {
                return;
            }
            let Some(first) = args.first() else { return };
            let is_const = matches!(kind::of(first), kind::CONSTANT_READ_NODE | kind::CONSTANT_PATH_NODE);
            let string_constant = helpers::rspec::is_str(first)
                && STRING_CONSTANT.is_match(&String::from_utf8_lossy(first.as_string_node().unwrap().unescaped()));
            if !is_const && !string_constant {
                let (s, e) = helpers::rspec::node_range(first);
                cx.add(s, e, "The first argument to describe should be the class or module being tested.");
            }
        });
    }
}

/// `ignored_metadata[key].to_a.include?(value)`
fn ignored_includes(ignored: &Map, key: &str, value: &str) -> bool {
    match ignored.get(key) {
        Some(Value::Seq(items)) => items.iter().any(|v| matches!(v, Value::Str(s) if s == value)),
        _ => false,
    }
}
