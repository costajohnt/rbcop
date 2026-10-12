use crate::prelude::*;

pub const NAME: &str = "Rails/ResponseParsedBody";
pub static COP: ResponseParsedBody = ResponseParsedBody;
pub struct ResponseParsedBody;

const HTML: &[&str] = &["HTML", "HTML4", "HTML5"];

/// (send (send nil? :response) :body)
fn response_body(n: &Node) -> bool {
    helpers::rails::send_node(n).is_some_and(|c| {
        method_name(&c) == "body"
            && send_args(&c).is_empty()
            && c.receiver().and_then(|r| helpers::rails::send_node(&r)).is_some_and(|r| {
                helpers::rails::is_command(&r, "response") && send_args(&r).is_empty()
            })
    })
}

/// (const #nokogiri? HTML)
fn nokogiri_html(n: &Node) -> bool {
    let Some(p) = n.as_constant_path_node() else { return false };
    p.name().is_some_and(|x| HTML.contains(&String::from_utf8_lossy(x.as_slice()).as_ref()))
        && p.parent().is_some_and(|ns| helpers::parser_parent::is_top_const(&ns, "Nokogiri"))
}

impl Cop for ResponseParsedBody {
    fn correctable(&self) -> bool {
        true
    }

    fn min_rails(&self) -> f64 {
        5.0
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        let name = method_name(&call);
        let args = send_args(&call);
        let [arg] = args.as_slice() else { return };
        if !response_body(arg) {
            return;
        }
        let Some(recv) = call.receiver() else { return };
        let json = name == "parse" && helpers::parser_parent::is_top_const(&recv, "JSON");
        let html = cx.config.target_rails_version() >= 7.1
            && ((HTML.contains(&name) && helpers::parser_parent::is_top_const(&recv, "Nokogiri"))
                || (name == "parse" && nokogiri_html(&recv))
                || (name == "parse"
                    && recv.as_constant_path_node().is_some_and(|p| {
                        p.name().is_some_and(|x| x.as_slice() == b"Document") && p.parent().is_some_and(|ns| nokogiri_html(&ns))
                    })));
        if !json && !html {
            return;
        }
        let (s, e) = send_range(&call);
        cx.add_fix(s, e, "Prefer `response.parsed_body`.", |c| c.replace(s, e, "response.parsed_body"));
    }
}
