//! Ruby's DidYouMean::SpellChecker (Jaro-Winkler + Levenshtein), as used by RuboCop's NameSimilarity.

fn jaro(a: &[char], b: &[char]) -> f64 {
    let (s1, s2) = if a.len() > b.len() { (b, a) } else { (a, b) };
    let (l1, l2) = (s1.len(), s2.len());
    let range = if l2 > 3 { l2 / 2 - 1 } else { 0 };
    let mut flags1 = vec![false; l1];
    let mut flags2 = vec![false; l2 + range + 1];
    let mut m = 0.0;
    for i in 0..l1 {
        let last = i + range;
        let mut j = i.saturating_sub(range);
        while j <= last {
            if !flags2[j] && s2.get(j) == Some(&s1[i]) {
                flags2[j] = true;
                flags1[i] = true;
                m += 1.0;
                break;
            }
            j += 1;
        }
    }
    let mut t = 0.0;
    let mut k = 0;
    for i in 0..l1 {
        if flags1[i] {
            let mut j = k;
            let mut index = k;
            while j < l2 {
                index = j;
                if flags2[j] {
                    break;
                }
                j += 1;
            }
            k = j + 1;
            if s2.get(index) != Some(&s1[i]) {
                t += 1.0;
            }
        }
    }
    let t = (t / 2.0_f64).floor();
    if m == 0.0 { 0.0 } else { (m / l1 as f64 + m / l2 as f64 + (m - t) / m) / 3.0 }
}

fn jaro_winkler(s1: &str, s2: &str) -> f64 {
    let (a, b): (Vec<char>, Vec<char>) = (s1.chars().collect(), s2.chars().collect());
    let d = jaro(&a, &b);
    if d > 0.7 {
        let mut prefix = 0;
        for c in &a {
            if b.get(prefix) == Some(c) && prefix < 4 {
                prefix += 1;
            } else {
                break;
            }
        }
        d + prefix as f64 * 0.1 * (1.0 - d)
    } else {
        d
    }
}

pub fn levenshtein(s1: &str, s2: &str) -> usize {
    let a: Vec<char> = s1.chars().collect();
    let b: Vec<char> = s2.chars().collect();
    let (n, m) = (a.len(), b.len());
    if n == 0 {
        return m;
    }
    if m == 0 {
        return n;
    }
    let mut d: Vec<usize> = (0..=m).collect();
    let mut x = 0;
    for (i0, c1) in a.iter().enumerate() {
        let mut i = i0 + 1;
        for j in 0..m {
            let cost = usize::from(*c1 != b[j]);
            let (p, q, r) = (d[j + 1] + 1, i + 1, d[j] + cost);
            x = if p < q && p < r { p } else if q < r { q } else { r };
            d[j] = i;
            i = x;
        }
        d[m] = x;
    }
    x
}

fn normalize(s: &str) -> String {
    s.to_lowercase().replace('@', "")
}

/// `DidYouMean::SpellChecker.new(dictionary:).correct(input)`.
/// ponytail: Ruby's `sort_by!` is not stable; ties here keep reversed dictionary order.
pub fn correct<'a>(input: &str, dictionary: &[&'a str]) -> Vec<&'a str> {
    let ni = normalize(input);
    let len = ni.chars().count();
    let threshold = if len > 3 { 0.834 } else { 0.77 };
    let mut words: Vec<&str> = dictionary
        .iter()
        .copied()
        // Jaro-Winkler <= 0.6 * Jaro + 0.4 and Jaro <= (2 + shorter / longer) / 3, so lengths more
        // than 6x apart never reach 0.834. Skipping them keeps a huge name (a 1 MB cop name in a
        // directive) from costing O(length * dictionary) per lookup; the result is unchanged.
        .filter(|w| {
            let l = normalize(w).chars().count();
            len <= 3 || l.min(len) * 6 >= l.max(len)
        })
        .filter(|w| jaro_winkler(&normalize(w), &ni) >= threshold)
        .filter(|w| *w != input)
        .collect();
    words.sort_by(|a, b| jaro_winkler(a, &ni).partial_cmp(&jaro_winkler(b, &ni)).unwrap());
    words.reverse();
    let threshold = (len as f64 * 0.25).ceil() as usize;
    let corrections: Vec<&str> = words.iter().copied().filter(|c| levenshtein(&normalize(c), &ni) <= threshold).collect();
    if !corrections.is_empty() {
        return corrections;
    }
    words
        .into_iter()
        .filter(|w| {
            let w = normalize(w);
            levenshtein(&w, &ni) < len.min(w.chars().count())
        })
        .take(1)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spell_checker() {
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        let dict = ["Layout/LineLength", "Lint/SelfAssignment", "Style/Encoding"];
        assert_eq!(correct("Layout/LineLenght", &dict), vec!["Layout/LineLength"]);
        assert_eq!(correct("lint/SelfAssignment", &dict), vec!["Lint/SelfAssignment"]);
        assert!(correct("Department/Cop", &dict).is_empty());
    }
}
