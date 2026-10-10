use crate::prelude::*;

pub const NAME: &str = "Lint/CircularArgumentReference";
pub static COP: CircularArgumentReference = CircularArgumentReference;
pub struct CircularArgumentReference;

impl Cop for CircularArgumentReference {
    fn kinds(&self) -> &'static [usize] {
        &[kind::OPTIONAL_PARAMETER_NODE, kind::OPTIONAL_KEYWORD_PARAMETER_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let (name, value) = match node.as_optional_parameter_node() {
            Some(p) => (p.name(), p.value()),
            None => {
                let p = node.as_optional_keyword_parameter_node().unwrap();
                (p.name(), p.value())
            }
        };
        let name = name.as_slice();
        let lvar = |n: &Node| n.as_local_variable_read_node().map(|l| l.name().as_slice().to_vec());
        if lvar(&value).is_some_and(|v| v == name) {
            return report(cx, &value, name);
        }
        // check_assignment_chain: `a = b = a`
        let mut seen: Vec<Vec<u8>> = Vec::new();
        let mut current = value;
        while let Some(w) = current.as_local_variable_write_node() {
            seen.push(w.name().as_slice().to_vec());
            current = w.value();
        }
        if seen.is_empty() {
            return;
        }
        if let Some(v) = lvar(&current)
            && (seen.contains(&v) || v == name)
        {
            report(cx, &current, name);
        }
    }
}

fn report(cx: &mut Cx, node: &Node, name: &[u8]) {
    let loc = node.location();
    let msg = format!("Circular argument reference - `{}`.", String::from_utf8_lossy(name));
    cx.add(loc.start_offset(), loc.end_offset(), msg);
}
