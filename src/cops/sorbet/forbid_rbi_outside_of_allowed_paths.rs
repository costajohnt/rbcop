use crate::prelude::*;

pub const NAME: &str = "Sorbet/ForbidRBIOutsideOfAllowedPaths";
pub static COP: ForbidRBIOutsideOfAllowedPaths = ForbidRBIOutsideOfAllowedPaths;
pub struct ForbidRBIOutsideOfAllowedPaths;

/// `File.fnmatch(pattern, path)` with no flags: `*` and `?` also match `/`, but not a leading `.`.
fn fnmatch(p: &[char], s: &[char], at_start: bool) -> bool {
    match p.first() {
        None => s.is_empty(),
        Some('*') => {
            if at_start && s.first() == Some(&'.') {
                return false;
            }
            (0..=s.len()).any(|i| fnmatch(&p[1..], &s[i..], false))
        }
        Some('?') => !s.is_empty() && !(at_start && s[0] == '.') && fnmatch(&p[1..], &s[1..], false),
        Some('[') => {
            let Some(&c) = s.first() else { return false };
            if at_start && c == '.' {
                return false;
            }
            let mut i = 1;
            let negate = matches!(p.get(1), Some('!' | '^'));
            if negate {
                i += 1;
            }
            let mut hit = false;
            let mut first = true;
            while i < p.len() && (first || p[i] != ']') {
                first = false;
                let lo = if p[i] == '\\' && i + 1 < p.len() { i += 1; p[i] } else { p[i] };
                if p.get(i + 1) == Some(&'-') && p.get(i + 2).is_some_and(|&h| h != ']') {
                    hit |= lo <= c && c <= p[i + 2];
                    i += 3;
                } else {
                    hit |= lo == c;
                    i += 1;
                }
            }
            // An unterminated bracket matches itself literally.
            if i >= p.len() {
                return c == '[' && fnmatch(&p[1..], &s[1..], false);
            }
            hit != negate && fnmatch(&p[i + 1..], &s[1..], false)
        }
        Some('\\') if p.len() > 1 => s.first() == Some(&p[1]) && fnmatch(&p[2..], &s[1..], false),
        Some(&c) => s.first() == Some(&c) && fnmatch(&p[1..], &s[1..], false),
    }
}

impl Cop for ForbidRBIOutsideOfAllowedPaths {
    fn on_file(&self, cx: &mut Cx) {
        let paths: Vec<String> = match cx.cfg.get("AllowedPaths") {
            Some(Value::Seq(items)) => items.iter().filter(|v| !matches!(v, Value::Null)).filter_map(Value::as_string).collect(),
            _ => return cx.add_global("AllowedPaths expects an array"),
        };
        if paths.is_empty() {
            return cx.add_global("AllowedPaths cannot be empty");
        }
        let cwd = crate::config::cwd();
        let rel = cx.src.absolute_path().replacen(&format!("{}/", cwd.display()), "", 1);
        let rel: Vec<char> = rel.chars().collect();
        if !paths.iter().any(|p| fnmatch(&p.chars().collect::<Vec<_>>(), &rel, true)) {
            cx.add_global(format!("RBI file path should match one of: {}", paths.join(", ")));
        }
    }
}
