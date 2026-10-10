//! Two-line captions flush to the same width, each line sized to fit it.
//!
//! The look every short-form editor calls "justified": the cue is split into
//! two lines and each line gets its own font size so both end up exactly as
//! wide as the caption box. A short line therefore renders big and a long one
//! small, and the block reads as one solid rectangle of text.

use crate::text_metrics::{self, FontMetrics};

/// One laid-out line: which of the cue's words it holds, and at what size.
#[derive(Debug, Clone, PartialEq)]
pub struct JustifiedLine {
    /// Index range into the cue's token list, `start..end`.
    pub start: usize,
    pub end: usize,
    pub font_px: u32,
}

/// How far a line's size may stray from the style's own font size before the
/// split is considered ugly and another one is tried.
/// A long line is *meant* to render small in this style, so the floor is
/// about readability, not about staying near the style's size.
const MIN_SIZE_RATIO: f32 = 0.35;
const MAX_SIZE_RATIO: f32 = 2.2;

/// Split `tokens` into at most two lines, each scaled to `target_w`.
///
/// Falls back to a single line when there is only one word, or when no split
/// keeps both lines within a sane size range.
pub fn justify_two_lines(
    tokens: &[String],
    font_family: &str,
    target_w: f32,
    base_px: u32,
) -> Vec<JustifiedLine> {
    if tokens.is_empty() || target_w <= 0.0 {
        return Vec::new();
    }

    let metrics = match text_metrics::metrics_for(font_family) {
        Some(m) => m,
        // No measurable font: one line at the style's own size, and libass
        // wraps it like any other cue.
        None => {
            return vec![JustifiedLine {
                start: 0,
                end: tokens.len(),
                font_px: base_px,
            }]
        }
    };

    if tokens.len() == 1 {
        return vec![JustifiedLine {
            start: 0,
            end: 1,
            font_px: line_size(metrics, tokens, 0, 1, target_w, base_px),
        }];
    }

    let min_px = (base_px as f32 * MIN_SIZE_RATIO).max(8.0);
    let max_px = base_px as f32 * MAX_SIZE_RATIO;

    // Start from an even word split — the first line ends up holding the
    // shorter half, which is what makes it the big one — and walk outwards
    // until both lines sit within the allowed size range.
    let n = tokens.len();
    let preferred = (n / 2).max(1);
    let mut candidates: Vec<usize> = vec![preferred];
    for offset in 1..n {
        for split in [preferred.wrapping_sub(offset), preferred + offset] {
            if split >= 1 && split < n && !candidates.contains(&split) {
                candidates.push(split);
            }
        }
    }

    for split in candidates {
        let first = raw_size(metrics, tokens, 0, split, target_w);
        let second = raw_size(metrics, tokens, split, n, target_w);
        if first >= min_px && first <= max_px && second >= min_px && second <= max_px {
            return vec![
                JustifiedLine {
                    start: 0,
                    end: split,
                    font_px: first.round() as u32,
                },
                JustifiedLine {
                    start: split,
                    end: n,
                    font_px: second.round() as u32,
                },
            ];
        }
    }

    // Nothing balanced: one line, clamped so it still fits the box.
    vec![JustifiedLine {
        start: 0,
        end: n,
        font_px: line_size(metrics, tokens, 0, n, target_w, base_px),
    }]
}

/// Line breaks for a cue at the style's own size, decided once for the whole
/// cue so they never move while its words are highlighted one by one.
///
/// Every line must fit `target_w` even with its widest word grown by `grow`
/// (the highlighted word is set bigger) and the whole line stretched by
/// `stretch` (the entrance animation). Within the fewest lines that fit, the
/// breaks are balanced the way libass's smart wrap balances them: the widest
/// line as narrow as it can be. Returns one line when the font can't be
/// measured; the caller then leaves wrapping to libass.
#[allow(clippy::needless_range_loop)] // j is a split point, not just an index
pub fn steady_lines(
    tokens: &[String],
    font_family: &str,
    target_w: f32,
    base_px: u32,
    grow: f32,
    stretch: f32,
) -> Vec<JustifiedLine> {
    let one_line = || {
        vec![JustifiedLine {
            start: 0,
            end: tokens.len(),
            font_px: base_px,
        }]
    };
    let Some(metrics) = text_metrics::metrics_for(font_family) else {
        return one_line();
    };
    let n = tokens.len();
    if n <= 1 || target_w <= 0.0 {
        return one_line();
    }
    let px = base_px as f32;
    let word_w: Vec<f32> = tokens.iter().map(|t| metrics.measure(t, px)).collect();
    let space = metrics.measure(" ", px);
    // Worst-case width of tokens[a..b] as one line.
    let width = |a: usize, b: usize| -> f32 {
        let text: f32 = word_w[a..b].iter().sum::<f32>() + space * (b - a - 1) as f32;
        let widest = word_w[a..b].iter().cloned().fold(0.0, f32::max);
        (text + widest * (grow - 1.0).max(0.0)) * stretch
    };

    // best[k][i]: the narrowest possible widest line when tokens[i..] are set
    // on k lines, and where the first of them ends. A single word wider than
    // the box still gets a line of its own.
    let fits = |a: usize, b: usize| b - a == 1 || width(a, b) <= target_w;
    let mut best: Vec<Vec<Option<(f32, usize)>>> = vec![vec![None; n + 1]; n + 1];
    best[0][n] = Some((0.0, n));
    for k in 1..=n {
        for i in (0..n).rev() {
            let mut choice: Option<(f32, usize)> = None;
            for j in i + 1..=n {
                if !fits(i, j) {
                    break;
                }
                let Some((rest, _)) = best[k - 1][j] else {
                    continue;
                };
                let worst = rest.max(width(i, j));
                if choice.is_none_or(|(w, _)| worst < w) {
                    choice = Some((worst, j));
                }
            }
            best[k][i] = choice;
        }
        if best[k][0].is_some() {
            let mut lines = Vec::with_capacity(k);
            let (mut start, mut left) = (0, k);
            while start < n {
                let (_, end) = best[left][start].expect("a layout was found");
                lines.push(JustifiedLine {
                    start,
                    end,
                    font_px: base_px,
                });
                start = end;
                left -= 1;
            }
            return lines;
        }
    }
    one_line()
}

fn line_text(tokens: &[String], start: usize, end: usize) -> String {
    tokens[start..end].join(" ")
}

fn raw_size(
    metrics: &FontMetrics,
    tokens: &[String],
    start: usize,
    end: usize,
    target_w: f32,
) -> f32 {
    metrics.size_for_width(&line_text(tokens, start, end), target_w)
}

fn line_size(
    metrics: &FontMetrics,
    tokens: &[String],
    start: usize,
    end: usize,
    target_w: f32,
    base_px: u32,
) -> u32 {
    let size = raw_size(metrics, tokens, start, end, target_w);
    if size <= 0.0 {
        return base_px;
    }
    // A single line may be smaller than the style asks for (it has to fit) but
    // never larger, or one-word cues would fill the frame.
    size.min(base_px as f32 * MAX_SIZE_RATIO).round() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn both_lines_end_up_the_same_width() {
        let tokens = toks(&["VÄHÄN", "NIINKU", "HUONOSTA", "MIESVALINNASTA"]);
        let target = 900.0;

        let lines = justify_two_lines(&tokens, "Montserrat Black", target, 80);
        assert_eq!(lines.len(), 2);

        let metrics = text_metrics::metrics_for("Montserrat Black").unwrap();
        for line in &lines {
            let text = line_text(&tokens, line.start, line.end);
            let width = metrics.measure(&text, line.font_px as f32);
            // Rounding the size to whole pixels is the only slack allowed.
            assert!(
                (width - target).abs() < target * 0.01,
                "line {:?} rendered {width}px wide, wanted {target}px",
                text
            );
        }
    }

    #[test]
    fn the_shorter_line_gets_the_bigger_font() {
        let tokens = toks(&["VÄHÄN", "NIINKU", "HUONOSTA", "MIESVALINNASTA"]);
        let lines = justify_two_lines(&tokens, "Montserrat Black", 900.0, 80);

        assert_eq!(lines.len(), 2);
        assert!(
            lines[0].font_px > lines[1].font_px,
            "first line {:?} should be the big one",
            lines
        );
    }

    #[test]
    fn steady_lines_fit_the_box_even_with_the_biggest_word_highlighted() {
        let tokens = toks(&[
            "AGENTIT,",
            "OLIVAT",
            "LÖYTÄNEET",
            "TAVAN",
            "KOMMUNIKOIDA",
            "KESKENÄÄN.",
        ]);
        let (target, px, grow, stretch) = (600.0, 70, 1.1, 1.03);
        let lines = steady_lines(&tokens, "Montserrat Black", target, px, grow, stretch);
        assert!(lines.len() >= 2);
        assert_eq!(lines.first().unwrap().start, 0);
        assert_eq!(lines.last().unwrap().end, tokens.len());
        let m = text_metrics::metrics_for("Montserrat Black").unwrap();
        for line in &lines {
            assert_eq!(line.font_px, px);
            let words = &tokens[line.start..line.end];
            let widest = words
                .iter()
                .map(|w| m.measure(w, px as f32))
                .fold(0.0, f32::max);
            let w = (m.measure(&words.join(" "), px as f32) + widest * 0.1) * stretch;
            assert!(words.len() == 1 || w <= target, "{words:?} is {w}px");
        }
    }

    #[test]
    fn steady_lines_are_balanced_not_greedy() {
        // Greedy would put five words on top and one below.
        let tokens = toks(&["YKSI", "KAKSI", "KOLME", "NELJÄ", "VIISI", "KUUSI"]);
        let m = text_metrics::metrics_for("Montserrat Black").unwrap();
        let all = m.measure(&tokens.join(" "), 60.0);
        let lines = steady_lines(&tokens, "Montserrat Black", all * 0.9, 60, 1.0, 1.0);
        assert_eq!(lines.len(), 2);
        let sizes: Vec<usize> = lines.iter().map(|l| l.end - l.start).collect();
        assert_eq!(sizes, [3, 3]);
    }

    #[test]
    fn steady_lines_keep_a_short_cue_on_one_line() {
        let lines = steady_lines(
            &toks(&["JA", "SITTEN"]),
            "Montserrat Black",
            900.0,
            60,
            1.1,
            1.03,
        );
        assert_eq!(lines.len(), 1);
        let unknown = steady_lines(
            &toks(&["JA", "SITTEN"]),
            "No Such Font 123",
            10.0,
            60,
            1.1,
            1.03,
        );
        assert_eq!(unknown.len(), 1);
    }

    #[test]
    fn a_single_word_stays_on_one_line() {
        let lines = justify_two_lines(&toks(&["KAUPPAKESKUS"]), "Montserrat Black", 900.0, 80);
        assert_eq!(lines.len(), 1);
        assert_eq!((lines[0].start, lines[0].end), (0, 1));
    }

    #[test]
    fn a_wildly_lopsided_split_is_avoided() {
        // "I" alone would have to be rendered absurdly large to fill the box.
        let tokens = toks(&["I", "AJATTELIN", "ETTÄ", "TÄMÄ", "ON", "IHAN", "HYVÄ"]);
        let lines = justify_two_lines(&tokens, "Montserrat Black", 900.0, 80);

        for line in &lines {
            assert!(line.font_px as f32 <= 80.0 * MAX_SIZE_RATIO);
            assert!(line.font_px as f32 >= 80.0 * MIN_SIZE_RATIO);
        }
    }
}
