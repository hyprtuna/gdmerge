//! One rule for showing a scene value to a person.
//!
//! Scene values run to thousands of characters, a whole tile map on one line,
//! and a report that prints one whole is unreadable. A report that cuts one
//! inside a token is wrong the other way: `SubResource("RectangleShape2D_g...`
//! can be neither read nor copied. So every value a report shows goes through
//! [`shown`], which bounds it and never cuts inside a quoted string: an id is
//! short and comes out whole, and a long string is replaced by `"..."` with a
//! count of what was left out, keeping the type around it.

/// Longest rendering of one value, in characters. Every renderer in the tool
/// goes through [`shown`], so this bounds any value it prints.
pub const MAX_SHOWN: usize = 120;

/// Where a long value is cut. What can follow is an id-sized string, or
/// `"..."` and a short tail, and then the elision note; together they fit in
/// the rest of [`MAX_SHOWN`].
const HEAD: usize = 48;
/// A string no longer than this that the cut lands in is kept whole rather
/// than replaced. Godot's longest ids, such as
/// `AnimationNodeStateMachineTransition_gdm10`, are a few characters shorter.
const ID_LENGTH: usize = 42;
/// How much may follow a replaced string and still be shown, such as `)`.
const TAIL: usize = 8;

/// Renders one value for a report, on one line and within [`MAX_SHOWN`].
pub fn shown(value: &str) -> String {
    let flat = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let chars: Vec<char> = flat.chars().collect();
    if chars.len() <= MAX_SHOWN {
        return flat;
    }

    // Is the cut inside a quoted string? Then where does that string start?
    let (mut open, mut escaped) = (None, false);
    for (i, &c) in chars.iter().enumerate().take(HEAD) {
        if escaped {
            escaped = false;
        } else if c == '\\' && open.is_some() {
            escaped = true;
        } else if c == '"' {
            open = if open.is_some() { None } else { Some(i) };
        }
    }
    let Some(open) = open else {
        // Not in a string: cut after the last space before HEAD, so a number
        // is not split either.
        let boundary = chars[HEAD / 2..HEAD]
            .iter()
            .rposition(|&c| c == ' ')
            .map(|i| HEAD / 2 + i + 1)
            .unwrap_or(HEAD);
        return elided(&chars, boundary);
    };
    let close = closing_quote(&chars, open);
    if close - open - 1 <= ID_LENGTH && close < chars.len() {
        // Keep the id, and the bracket or two closing the call around it.
        let brackets =
            chars[close + 1..].iter().take(2).take_while(|&&c| c == ')' || c == ']').count();
        return elided(&chars, close + 1 + brackets);
    }

    let prefix: String = chars[..open].iter().collect();
    let after = (close + 1).min(chars.len());
    let rest = chars.len() - after;
    if rest <= TAIL {
        let tail: String = chars[after..].iter().collect();
        let n = close.min(chars.len()) - open - 1;
        return format!("{prefix}\"...\"{tail} ({n} chars elided)");
    }
    format!("{prefix}\"...\"... ({} chars elided)", chars.len() - open)
}

/// `chars[..cut]` followed by an ellipsis and a count of what was left out.
fn elided(chars: &[char], cut: usize) -> String {
    let head: String = chars[..cut].iter().collect();
    format!("{head}... ({} chars elided)", chars.len() - cut)
}

/// Index of the quote closing the string opened at `open`, or the length when
/// the string never closes.
fn closing_quote(chars: &[char], open: usize) -> usize {
    let mut escaped = false;
    for (i, &c) in chars.iter().enumerate().skip(open + 1) {
        if escaped {
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == '"' {
            return i;
        }
    }
    chars.len()
}

#[cfg(test)]
mod tests {
    use super::{shown, MAX_SHOWN};

    fn count(s: &str) -> usize {
        s.chars().count()
    }

    #[test]
    fn a_value_within_the_bound_is_shown_whole() {
        assert_eq!(
            shown("SubResource(\"RectangleShape2D_gdm0\")"),
            "SubResource(\"RectangleShape2D_gdm0\")"
        );
        assert_eq!(shown("Vector2(1,\n  2)"), "Vector2(1, 2)");
    }

    #[test]
    fn a_long_string_is_replaced_and_its_type_kept() {
        let data = "A".repeat(16340);
        let out = shown(&format!("PackedByteArray(\"{data}\")"));
        assert_eq!(out, "PackedByteArray(\"...\") (16340 chars elided)");
    }

    #[test]
    fn an_id_across_the_cut_is_kept_whole() {
        let id = "AnimationNodeStateMachineTransition_gdm10";
        let value =
            format!("Array[RectangleShape2D]([SubResource(\"{id}\"), {}])", "1, ".repeat(60));
        let out = shown(&value);
        assert!(out.contains(&format!("SubResource(\"{id}\")")), "{out}");
        assert!(out.ends_with("chars elided)"), "{out}");
        assert!(count(&out) <= MAX_SHOWN, "{out}");
    }

    #[test]
    fn an_unquoted_value_is_cut_at_an_item_boundary() {
        let value = format!(
            "PackedFloat32Array({})",
            (0..60).map(|n| n.to_string()).collect::<Vec<_>>().join(", ")
        );
        let out = shown(&value);
        assert!(out.starts_with("PackedFloat32Array(0, 1, 2,"), "{out}");
        assert!(out.contains(", ... ("), "cut between items: {out}");
        assert!(count(&out) <= MAX_SHOWN, "{out}");
    }

    /// The generator is a seeded LCG, as in the tscn property tests: no
    /// dependency, and a failure reproduces from the seed.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            self.0 >> 33
        }

        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }

        fn text(&mut self, len: usize, alphabet: &[char]) -> String {
            (0..len).map(|_| alphabet[self.below(alphabet.len())]).collect()
        }
    }

    const ID: &[char] = &['a', 'b', 'c', '_', '1', '2', 'S', 'h', 'a', 'p', 'e'];
    const JUNK: &[char] = &['0', '1', '9', ',', ' ', '.', '-', 'x', '(', ')', '[', ']'];
    const TEXT: &[char] = &['A', 'z', '0', ' ', '\\', '"', ',', '\n', 'é'];

    #[test]
    fn every_rendering_stays_within_the_bound() {
        let mut rng = Rng(0x5eed);
        for case in 0..4000 {
            let value = match case % 5 {
                0 => {
                    let n = rng.below(5000);
                    rng.text(n, JUNK)
                }
                1 => {
                    let n = rng.below(5000);
                    format!("PackedByteArray(\"{}\")", rng.text(n, TEXT))
                }
                2 => {
                    let n = rng.below(100);
                    format!("SubResource(\"{}\")", rng.text(n, ID))
                }
                3 => {
                    let n = rng.below(40);
                    let refs: Vec<String> = (0..n)
                        .map(|_| {
                            let len = 1 + rng.below(30);
                            format!("ExtResource(\"{}\")", rng.text(len, ID))
                        })
                        .collect();
                    format!("Array[Resource]([{}])", refs.join(", "))
                }
                _ => {
                    let (a, b) = (rng.below(300), rng.below(300));
                    format!("\"{}\" {}", rng.text(a, TEXT), rng.text(b, JUNK))
                }
            };
            let out = shown(&value);
            assert!(count(&out) <= MAX_SHOWN, "case {case}: {} chars: {out}", count(&out));
            let flat = value.split_whitespace().collect::<Vec<_>>().join(" ");
            if count(&flat) <= MAX_SHOWN {
                assert_eq!(out, flat, "case {case}");
            }
            if case % 5 == 2 {
                assert_eq!(out, flat, "an id is never cut: case {case}");
            }
        }
    }
}
