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
pub fn correct<'a>(input: &str, dictionary: &[&'a str]) -> Vec<&'a str> {
    let ni = normalize(input);
    let len = ni.chars().count();
    let threshold = if len > 3 { 0.834 } else { 0.77 };
    let words: Vec<&str> = dictionary
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
    let mut keyed: Vec<(f64, &str)> = words.iter().map(|w| (jaro_winkler(w, &ni), *w)).collect();
    ruby_sort_by_float(&mut keyed);
    let mut words: Vec<&str> = keyed.into_iter().map(|(_, w)| w).collect();
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

/// Ruby's `sort_by` with Float keys (enum.c `rb_uniform_intro_sort_2`): an unstable introsort,
/// ported step for step so that ties come out in Ruby's order.
pub fn ruby_sort_by_float<T: Copy>(v: &mut [(f64, T)]) {
    let n = v.len();
    if n < 2 || v.windows(2).all(|w| w[0].0 <= w[1].0) {
        return;
    }
    let d = (usize::BITS - n.leading_zeros() - 1) as usize;
    quicksort_intro(v, 0, n, d << 1);
}

fn insertion_sort<T: Copy>(v: &mut [(f64, T)], begin: usize, end: usize) {
    for index in begin + 1..end {
        let tmp = v[index];
        let mut j = index;
        if tmp.0 < v[begin].0 {
            while begin < j {
                v[j] = v[j - 1];
                j -= 1;
            }
        } else {
            while tmp.0 < v[j - 1].0 {
                v[j] = v[j - 1];
                j -= 1;
            }
        }
        v[j] = tmp;
    }
}

fn heap_down<T: Copy>(v: &mut [(f64, T)], mut offset: usize, len: usize) {
    let tmp = v[offset];
    loop {
        let mut c = (offset << 1) + 1;
        if c > len {
            break;
        }
        if c < len && v[c].0 < v[c + 1].0 {
            c += 1;
        }
        if tmp.0 >= v[c].0 {
            break;
        }
        v[offset] = v[c];
        offset = c;
    }
    v[offset] = tmp;
}

fn heapsort<T: Copy>(v: &mut [(f64, T)]) {
    let n = v.len();
    for offset in (0..n >> 1).rev() {
        heap_down(v, offset, n - 1);
    }
    for offset in (1..n).rev() {
        v.swap(0, offset);
        heap_down(v, 0, offset - 1);
    }
}

fn quicksort_intro<T: Copy>(v: &mut [(f64, T)], begin: usize, end: usize, d: usize) {
    if end - begin <= 16 {
        insertion_sort(v, begin, end);
        return;
    }
    if d == 0 {
        heapsort(&mut v[begin..end]);
        return;
    }
    let (a, b, c) = (v[begin].0, v[begin + ((end - begin) >> 1)].0, v[end - 1].0);
    // med3_val
    let x = if a < b {
        if b < c {
            b
        } else if c < a {
            a
        } else {
            c
        }
    } else if c < b {
        b
    } else if a < c {
        a
    } else {
        c
    };
    let (mut i, mut j) = (begin as isize, end as isize - 1);
    loop {
        while v[i as usize].0 < x {
            i += 1;
        }
        while x < v[j as usize].0 {
            j -= 1;
        }
        if i <= j {
            v.swap(i as usize, j as usize);
            i += 1;
            j -= 1;
        }
        if i > j {
            break;
        }
    }
    let (i, j) = (i as usize, (j + 1) as usize);
    if end - j > 1 {
        quicksort_intro(v, j, end, d - 1);
    }
    if i - begin > 1 {
        quicksort_intro(v, begin, i, d - 1);
    }
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

    // Ruby 4.0.6: `(0...n).map { |i| [k(i), i] }.sort_by(&:first).map(&:last)` (#612)
    #[test]
    fn sort_by_float_ties_in_ruby_order() {
        let order = |n: usize, k: &dyn Fn(usize) -> f64| {
            let mut v: Vec<(f64, usize)> = (0..n).map(|i| (k(i), i)).collect();
            ruby_sort_by_float(&mut v);
            v.into_iter().map(|p| p.1).collect::<Vec<_>>()
        };
        let ruby = [
            39, 36, 33, 30, 27, 24, 21, 18, 15, 12, 0, 9, 3, 6, 37, 1, 34, 31, 28, 4, 25, 22, 10, 19, 13, 7, 16, 38, 11,
            35, 14, 2, 32, 8, 29, 17, 26, 5, 23, 20,
        ];
        assert_eq!(order(40, &|i| (i % 3) as f64 * 0.5), ruby);
        // `s.each_with_index.sum { |v, i| v * (i + 1) }`
        let sum = |s: Vec<usize>| s.iter().enumerate().map(|(i, v)| v * (i + 1)).sum::<usize>();
        assert_eq!(sum(order(300, &|i| ((i * 7919) % 11) as f64 * 0.25)), 6708209);
        assert_eq!(sum(order(3000, &|i| ((i * 7919) % 5) as f64 * 0.25)), 6710274865);
    }
}
