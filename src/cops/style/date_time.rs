use crate::helpers::parser_parent::is_top_const;
use crate::prelude::*;

pub const NAME: &str = "Style/DateTime";
pub static COP: DateTime = DateTime;
pub struct DateTime;

impl Cop for DateTime {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let recv = call.receiver();
        let block_pass = call.block().is_some_and(|b| b.as_block_argument_node().is_some());
        let date_time = recv.as_ref().is_some_and(|r| is_top_const(r, "DateTime"));
        // (call !nil? :to_datetime)
        let to_datetime =
            recv.is_some() && method_name(&call) == "to_datetime" && call.arguments().is_none() && !block_pass;
        if !(date_time || (to_datetime && !cx.cfg.bool("AllowCoercion").unwrap_or(false))) {
            return;
        }
        // historic_date?: (call _ _ _ (const (const {nil? (cbase)} :Date) _))
        let args = call_args(&call);
        if args.len() == 2 && !block_pass {
            let historic = args[1]
                .as_constant_path_node()
                .and_then(|p| p.parent())
                .is_some_and(|p| is_top_const(&p, "Date"));
            if historic {
                return;
            }
        }
        let msg = if to_datetime { "Do not use `#to_datetime`." } else { "Prefer `Time` over `DateTime`." };
        // receiver.loc.name
        let name = recv.as_ref().and_then(|r| {
            r.as_constant_path_node()
                .map(|p| (p.name_loc().start_offset(), p.name_loc().end_offset()))
                .or_else(|| r.as_constant_read_node().map(|_| (r.location().start_offset(), r.location().end_offset())))
        });
        let (start, end) = send_range(&call);
        cx.add_fix(start, end, msg, |c| {
            if to_datetime {
                return;
            }
            if let Some((a, b)) = name {
                c.replace(a, b, "Time");
            }
        });
    }
}
