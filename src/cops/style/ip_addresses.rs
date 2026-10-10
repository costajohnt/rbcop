use crate::prelude::*;
use helpers::string_help;

pub const NAME: &str = "Style/IpAddresses";
pub static COP: IpAddresses = IpAddresses;
pub struct IpAddresses;

const R256: &str = r"(?:0|1(?:[0-9][0-9]?)?|2(?:[0-4][0-9]?|5[0-5]?|[6-9])?|[3-9][0-9]?)";
const H: &str = r"[0-9A-Fa-f]{1,4}";

/// `Resolv::IPv4::Regex` and `Resolv::IPv6::Regex` (ASCII `\d`). Contents are at most 45
/// characters before they get here, so matching stays cheap.
static IP: LazyLock<Regex> = LazyLock::new(|| {
    let hexseq = format!(r"(?:{H}(?::{H})*)?");
    let dec4 = r"[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+";
    let zone = r"%[-0-9A-Za-z._~]+";
    let alts = [
        format!(r"{R256}\.{R256}\.{R256}\.{R256}"),
        format!(r"(?:{H}:){{7}}{H}"),
        format!(r"{hexseq}::{hexseq}"),
        format!(r"(?:{H}:){{6}}{dec4}"),
        format!(r"{hexseq}::(?:{H}:)*{dec4}"),
        format!(r"[Ff][Ee]80(?::{H}){{7}}{zone}"),
        format!(r"[Ff][Ee]80:(?:{hexseq}::{hexseq}|:{hexseq})?:{H}{zone}"),
    ];
    let body = alts.map(|a| format!(r"\A(?:{a})\z")).join("|");
    Regex::new(&body).unwrap()
});

impl Cop for IpAddresses {
    fn kinds(&self) -> &'static [usize] {
        &[kind::STRING_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if string_help::delimited_str(node).is_none() || string_help::in_regexp(cx.ancestors) {
            return;
        }
        // `node.source[1...-1]`
        let src = node_text(node);
        let mut chars = src.chars();
        chars.next();
        chars.next_back();
        let contents = chars.as_str();
        let Some(first) = contents.chars().next() else { return };
        let lower = contents.to_lowercase();
        if cx.cfg.strings("AllowedAddresses").iter().any(|a| a.to_lowercase() == lower) {
            return;
        }
        if contents.chars().count() > 45 || !matches!(first, '0'..=':' | 'A'..='F' | 'a'..='f') {
            return;
        }
        if IP.is_match(contents) {
            let loc = node.location();
            cx.add(loc.start_offset(), loc.end_offset(), "Do not hardcode IP addresses.");
        }
    }
}
