use crate::prelude::*;

pub const NAME: &str = "Style/BlockComments";
pub static COP: BlockComments = BlockComments;
pub struct BlockComments;

impl Cop for BlockComments {
    fn correctable(&self) -> bool {
        true
    }

    fn on_file(&self, cx: &mut Cx) {
        let src = cx.src;
        for c in src.comments.iter().filter(|c| c.embdoc) {
            let (start, end) = (c.start, c.end);
            let text = &src.text[start..end];
            // eq_begin: "=begin\n".length characters (bytes: `=begin` is ASCII, then one newline).
            let begin_end = start + 7;
            let eq_end = if text.ends_with('\n') { (end - 5, end) } else { (end - 6, end - 2) };
            cx.add_fix(start, end, "Do not use block comments.", |k| {
                k.remove(start, begin_end);
                if begin_end < eq_end.0 {
                    let contents = &src.text[begin_end..eq_end.0];
                    let s = format!("# {contents}").replace("\n\n", "\n#\n");
                    // gsub(/\n(?=[^#])/, "\n# ")
                    let mut out = String::with_capacity(s.len() + 16);
                    let mut it = s.chars().peekable();
                    while let Some(ch) = it.next() {
                        out.push(ch);
                        if ch == '\n' && it.peek().is_some_and(|&n| n != '#') {
                            out.push_str("# ");
                        }
                    }
                    k.replace(begin_end, eq_end.0, &out);
                }
                k.remove(eq_end.0, eq_end.1);
            });
        }
    }
}
