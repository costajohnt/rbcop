//! Expected results were recorded by running the same operations through parser 3.3.12.0's
//! TreeRewriter (`None` = ClobberingError). Cases follow parser's docs and tests: nesting,
//! insertion order at one position, and each policy.
use super::*;

const S: &str = "puts(:hello, :world)";
const RAISE: bool = true;
const ACC: bool = false;

type Op<'a> = (&'a str, usize, usize, &'a str, &'a str);
type Out = Option<(String, Vec<(usize, usize, String)>)>;

fn run(src: &str, raise: bool, ops: &[Op]) -> Out {
    let p = if raise {
        Policies {
            crossing_deletions: Policy::Raise,
            different_replacements: Policy::Raise,
            swallowed_insertions: Policy::Raise,
        }
    } else {
        Policies::default()
    };
    let mut r = TreeRewriter::new(Arc::from(src), p);
    for &(op, a, b, x, y) in ops {
        let res = match op {
            "replace" => r.replace(a, b, x),
            "remove" => r.remove(a, b),
            "wrap" => r.wrap(a, b, x, y),
            "insert_before" => r.insert_before(a, b, x),
            "insert_after" => r.insert_after(a, b, x),
            _ => unreachable!(),
        };
        res.ok()?;
    }
    Some((r.process(), r.as_replacements()))
}

#[test]
fn matches_parser() {
    type Case<'a> = (&'a str, bool, &'a [Op<'a>], Option<&'a str>, &'a [(usize, usize, &'a str)]);
    #[rustfmt::skip]
    let cases: &[Case] = &[
        (S, ACC, &[("replace", 5, 11, ":hi", ""), ("remove", 11, 13, "", ""), ("wrap", 13, 19, "[", "]")], Some("puts(:hi[:world])"), &[(5, 11, ":hi"), (11, 13, ""), (13, 13, "["), (19, 19, "]")]),
        ("abc", ACC, &[("wrap", 1, 3, "<", ">"), ("wrap", 0, 3, "{", "}"), ("wrap", 1, 2, "(", ")")], Some("{a<(b)c>}"), &[(0, 0, "{"), (1, 1, "<"), (1, 1, "("), (2, 2, ")"), (3, 3, ">"), (3, 3, "}")]),
        ("abc", ACC, &[("insert_before", 1, 2, "1", ""), ("insert_before", 1, 2, "2", ""), ("insert_after", 1, 2, "3", ""), ("insert_after", 1, 2, "4", "")], Some("a21b34c"), &[(1, 1, "21"), (2, 2, "34")]),
        ("abc", ACC, &[("insert_before", 1, 2, "B", ""), ("insert_after", 0, 1, "A", "")], Some("aABbc"), &[(1, 1, "A"), (1, 1, "B")]),
        (S, ACC, &[("wrap", 5, 19, "[", "]"), ("wrap", 5, 11, "{", "}"), ("replace", 13, 19, ":everybody", ""), ("wrap", 13, 19, "{", "}")], Some("puts([{:hello}, {:everybody}])"), &[(5, 5, "["), (5, 5, "{"), (11, 11, "}"), (13, 13, "{"), (13, 19, ":everybody"), (19, 19, "}"), (19, 19, "]")]),
        (S, ACC, &[("wrap", 5, 11, "[", "]"), ("replace", 5, 11, ":hey", ""), ("insert_before", 13, 19, "<", "")], Some("puts([:hey], <:world)"), &[(5, 5, "["), (5, 11, ":hey"), (11, 11, "]"), (13, 13, "<")]),
        (S, ACC, &[("remove", 5, 13, "", ""), ("remove", 11, 19, "", "")], Some("puts()"), &[(5, 19, "")]),
        (S, RAISE, &[("remove", 5, 13, "", ""), ("remove", 11, 19, "", "")], None, &[]),
        (S, RAISE, &[("remove", 5, 19, "", ""), ("remove", 11, 13, "", "")], Some("puts()"), &[(5, 19, "")]),
        (S, ACC, &[("wrap", 5, 13, "[", "]"), ("wrap", 11, 19, "(", ")")], None, &[]),
        (S, ACC, &[("remove", 5, 13, "", ""), ("replace", 11, 19, "x", "")], None, &[]),
        (S, ACC, &[("replace", 5, 11, ":hi", ""), ("replace", 5, 11, ":hey", "")], Some("puts(:hey, :world)"), &[(5, 11, ":hey")]),
        (S, RAISE, &[("replace", 5, 11, ":hi", ""), ("replace", 5, 11, ":hey", "")], None, &[]),
        (S, RAISE, &[("replace", 5, 11, ":hi", ""), ("replace", 5, 11, ":hi", "")], Some("puts(:hi, :world)"), &[(5, 11, ":hi")]),
        (S, ACC, &[("wrap", 5, 11, "[", "]"), ("replace", 5, 19, "x", "")], Some("puts(x)"), &[(5, 19, "x")]),
        (S, RAISE, &[("wrap", 5, 11, "[", "]"), ("replace", 5, 19, "x", "")], None, &[]),
        (S, RAISE, &[("remove", 5, 11, "", ""), ("replace", 5, 19, "x", "")], Some("puts(x)"), &[(5, 19, "x")]),
        (S, ACC, &[("replace", 0, 20, "x", ""), ("insert_before", 5, 11, "!", "")], Some("x"), &[(0, 20, "x")]),
        (S, RAISE, &[("replace", 0, 20, "x", ""), ("insert_before", 5, 11, "!", "")], None, &[]),
        (S, RAISE, &[("wrap", 5, 11, "(", ")"), ("replace", 5, 11, ":x", "")], Some("puts((:x), :world)"), &[(5, 5, "("), (5, 11, ":x"), (11, 11, ")")]),
        (S, RAISE, &[("remove", 5, 11, "", ""), ("remove", 11, 13, "", ""), ("insert_after", 11, 13, "!", "")], Some("puts(!:world)"), &[(5, 11, ""), (11, 13, ""), (13, 13, "!")]),
        (S, RAISE, &[("remove", 5, 11, "", ""), ("insert_before", 11, 11, "x", "")], Some("puts(x, :world)"), &[(5, 11, ""), (11, 11, "x")]),
        (S, RAISE, &[("insert_before", 11, 11, "x", ""), ("remove", 5, 11, "", "")], Some("puts(x, :world)"), &[(5, 11, ""), (11, 11, "x")]),
        (S, RAISE, &[("insert_after", 5, 5, "x", ""), ("remove", 5, 11, "", "")], Some("puts(x, :world)"), &[(5, 5, "x"), (5, 11, "")]),
        (S, ACC, &[("remove", 5, 7, "", ""), ("remove", 9, 15, "", ""), ("remove", 6, 10, "", "")], Some("puts(orld)"), &[(5, 15, "")]),
        (S, ACC, &[("remove", 2, 4, "", ""), ("remove", 6, 8, "", ""), ("remove", 10, 12, "", ""), ("remove", 3, 11, "", "")], Some("pu :world)"), &[(2, 12, "")]),
        (S, ACC, &[("insert_before", 5, 5, "a", ""), ("insert_before", 5, 11, "b", ""), ("insert_after", 0, 5, "c", ""), ("insert_before", 5, 5, "d", "")], Some("puts(cdab:hello, :world)"), &[(5, 5, "c"), (5, 5, "da"), (5, 5, "b")]),
        (S, ACC, &[("replace", 5, 5, "a", ""), ("replace", 5, 5, "b", ""), ("wrap", 5, 5, "<", ">")], Some("puts(<b>:hello, :world)"), &[(5, 5, "<"), (5, 5, "b"), (5, 5, ">")]),
    ];
    for (i, &(src, raise, ops, out, reps)) in cases.iter().enumerate() {
        let expected = out.map(|o| (o.to_string(), reps.iter().map(|&(a, b, s)| (a, b, s.to_string())).collect()));
        assert_eq!(run(src, raise, ops), expected, "case {i}");
    }
}

#[test]
fn empty_actions_are_ignored() {
    let mut r = TreeRewriter::new(Arc::from(S), Policies::default());
    r.remove(3, 3).unwrap();
    r.wrap(3, 7, "", "").unwrap();
    assert!(r.is_empty());
    r.replace(3, 3, "x").unwrap();
    assert!(!r.is_empty());
}

#[test]
fn merge_combines_and_keeps_self_on_error() {
    let src: Arc<str> = Arc::from(S);
    let p =
        Policies { different_replacements: Policy::Raise, swallowed_insertions: Policy::Raise, ..Policies::default() };
    let mut a = TreeRewriter::new(src.clone(), p);
    a.replace(5, 11, ":hi").unwrap();
    let mut b = TreeRewriter::new(src.clone(), p);
    b.insert_before(13, 19, "*").unwrap();
    b.insert_before(5, 11, "&").unwrap();
    a.merge(&b).unwrap();
    assert_eq!(a.process(), "puts(&:hi, *:world)");
    let mut c = TreeRewriter::new(src, p);
    c.remove(0, 4).unwrap();
    c.replace(5, 11, ":ho").unwrap();
    assert_eq!(a.merge(&c), Err(Clobbering));
    assert_eq!(a.process(), "puts(&:hi, *:world)");
}

#[test]
fn utf8_offsets() {
    let mut r = TreeRewriter::new(Arc::from("é = \"ü\"  "), Policies::default());
    r.remove(9, 11).unwrap();
    r.replace(0, 2, "e").unwrap();
    assert_eq!(r.process(), "e = \"ü\"");
}
