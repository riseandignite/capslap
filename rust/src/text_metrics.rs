//! Glyph-accurate text measurement over the bundled caption fonts.
//!
//! Caption layout used to guess line widths from a character count, which is
//! why lines overflowed the frame and why the editor preview never matched the
//! burn. Measuring the actual font removes the guess: the same numbers drive
//! the ASS document and the preview layout the editor draws.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Advance widths of one font, in em units (multiply by font size in px).
pub struct FontMetrics {
    advances: HashMap<char, f32>,
    units_per_em: f32,
    fallback_advance: f32,
    /// The em size, in px, of one px of ASS font size (`\fs`). libass, like
    /// VSFilter, sizes a font so its Windows ascent + descent is `\fs` tall,
    /// not its em: a font with tall ascenders sets narrower than its em says.
    em_per_font_px: f32,
}

impl FontMetrics {
    fn from_file(path: &Path) -> Option<FontMetrics> {
        let data = std::fs::read(path).ok()?;
        let face = ttf_parser::Face::parse(&data, 0).ok()?;
        let units_per_em = face.units_per_em() as f32;
        if units_per_em <= 0.0 {
            return None;
        }

        // Cover Latin plus the Nordic letters captions actually contain; a
        // character outside the set falls back to the average advance, which
        // only ever affects exotic punctuation.
        let mut advances = HashMap::new();
        let mut total = 0.0f32;
        let mut counted = 0u32;
        for c in (0x20u32..0x17Fu32).filter_map(char::from_u32) {
            if let Some(gid) = face.glyph_index(c) {
                if let Some(adv) = face.glyph_hor_advance(gid) {
                    let em_fraction = adv as f32 / units_per_em;
                    advances.insert(c, em_fraction);
                    total += em_fraction;
                    counted += 1;
                }
            }
        }
        if counted == 0 {
            return None;
        }

        // usWinAscent + usWinDescent, as libass reads it (ass_face_set_size);
        // hhea's ascent - descent when a font has no OS/2 numbers.
        let win_height = face
            .tables()
            .os2
            .map(|os2| os2.windows_ascender() as f32 - os2.windows_descender() as f32)
            .filter(|h| *h > 0.0);
        let hhea_height =
            Some(face.ascender() as f32 - face.descender() as f32).filter(|h| *h > 0.0);
        let em_per_font_px = win_height
            .or(hhea_height)
            .map(|h| units_per_em / h)
            .unwrap_or(1.0);

        Some(FontMetrics {
            advances,
            units_per_em,
            fallback_advance: total / counted as f32,
            em_per_font_px,
        })
    }

    /// Width of `text` when libass sets it at ASS font size `font_px`, in pixels.
    pub fn measure(&self, text: &str, font_px: f32) -> f32 {
        let em_width: f32 = text
            .chars()
            .map(|c| {
                self.advances
                    .get(&c)
                    .copied()
                    .unwrap_or(self.fallback_advance)
            })
            .sum();
        em_width * font_px * self.em_per_font_px
    }

    /// The font size at which `text` is exactly `target_px` wide.
    pub fn size_for_width(&self, text: &str, target_px: f32) -> f32 {
        let em_width = self.measure(text, 1.0);
        if em_width <= 0.0 {
            return 0.0;
        }
        target_px / em_width
    }

    #[allow(dead_code)]
    pub fn units_per_em(&self) -> f32 {
        self.units_per_em
    }
}

fn fonts_dir() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("CAPSLAP_FONTS_DIR") {
        let p = PathBuf::from(path);
        if p.is_dir() {
            return Some(p);
        }
    }
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/fonts");
    if dev.is_dir() {
        return Some(dev);
    }
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    [dir.join("fonts"), dir.parent()?.join("Resources/fonts")]
        .into_iter()
        .find(|candidate| candidate.is_dir())
}

/// Find the font file whose name matches `family`.
///
/// A font is called different things by different name records, and the two
/// sides of this app pick different ones: the style presets use the full name
/// libass resolves ("Roboto Bold", name 4), while the editor's font menu shows
/// what Qt reports, which is the typographic family ("Montserrat", name 16,
/// for a file whose full name is "Montserrat Black"). Both have to land on the
/// same file, so all of them are matched, closest name first.
///
/// Returns None when nothing matches. There is deliberately no fallback font:
/// measuring one font's text against another's glyphs silently mislays every
/// caption, and a caller that gets None can fall back honestly instead.
fn find_font_file(family: &str) -> Option<PathBuf> {
    let dir = fonts_dir()?;
    let wanted = normalize_name(family);

    // Lower is closer: an exact full/family name beats a PostScript name,
    // which beats the typographic family a whole weight range shares.
    let rank_of = |name_id: u16| -> Option<u8> {
        match name_id {
            4 | 1 => Some(0),
            6 => Some(1),
            16 => Some(2),
            _ => None,
        }
    };

    let mut best: Option<(u8, PathBuf)> = None;
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase());
        if !matches!(ext.as_deref(), Some("ttf") | Some("otf")) {
            continue;
        }

        let Ok(data) = std::fs::read(&path) else {
            continue;
        };
        let Ok(face) = ttf_parser::Face::parse(&data, 0) else {
            continue;
        };
        for name in face.names().into_iter() {
            let Some(rank) = rank_of(name.name_id) else {
                continue;
            };
            if best.as_ref().is_some_and(|(b, _)| *b <= rank) {
                continue;
            }
            if let Some(value) = name.to_string() {
                if normalize_name(&value) == wanted {
                    if rank == 0 {
                        return Some(path);
                    }
                    best = Some((rank, path.clone()));
                }
            }
        }
    }
    best.map(|(_, path)| path)
}

fn normalize_name(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

type MetricsCache = HashMap<String, Option<&'static FontMetrics>>;

/// Metrics for a font family, loaded once per process.
pub fn metrics_for(family: &str) -> Option<&'static FontMetrics> {
    static CACHE: OnceLock<Mutex<MetricsCache>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));

    let mut guard = cache.lock().ok()?;
    if let Some(hit) = guard.get(family) {
        return *hit;
    }

    let loaded = find_font_file(family)
        .as_deref()
        .and_then(FontMetrics::from_file)
        // Leaked deliberately: one small table per font, alive for the process.
        .map(|m| &*Box::leak(Box::new(m)));
    guard.insert(family.to_string(), loaded);
    loaded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures_a_bundled_font_and_scales_linearly() {
        let m = metrics_for("Montserrat Black").expect("bundled font should load");

        let at_50 = m.measure("KAUPPA", 50.0);
        let at_100 = m.measure("KAUPPA", 100.0);

        assert!(at_50 > 0.0);
        assert!((at_100 - at_50 * 2.0).abs() < 0.01);
        // A wider string must measure wider at the same size.
        assert!(m.measure("MIESVALINNASTA", 50.0) > at_50);
    }

    #[test]
    fn size_for_width_round_trips() {
        let m = metrics_for("Montserrat Black").expect("bundled font should load");

        let size = m.size_for_width("VÄHÄN NIINKU", 900.0);
        assert!(size > 0.0);
        assert!((m.measure("VÄHÄN NIINKU", size) - 900.0).abs() < 0.5);
    }

    #[test]
    fn sizes_like_libass_does_from_the_windows_ascent_and_descent() {
        // Anton's ascent + descent is far taller than its em, so at the same
        // \fs it sets much narrower than an em-based measure would say.
        let anton = metrics_for("Anton").expect("bundled font should load");
        assert!(anton.em_per_font_px < 0.75, "{}", anton.em_per_font_px);
        let montserrat = metrics_for("Montserrat Black").unwrap();
        assert!(montserrat.em_per_font_px > 0.7 && montserrat.em_per_font_px < 1.0);
    }

    #[test]
    fn unknown_family_measures_nothing_rather_than_the_wrong_font() {
        // Silently measuring against whatever font the directory listed first
        // is worse than not measuring: the caller can fall back honestly, but
        // it cannot tell that flush lines were sized against another typeface.
        assert!(metrics_for("No Such Font 12345").is_none());
    }

    #[test]
    fn resolves_the_names_both_sides_of_the_app_use() {
        // The style presets carry full names ("Montserrat Black"); the font
        // menu shows the typographic family Qt reports ("Montserrat"). Both
        // have to find the same file, or the burn is laid out against one
        // font and drawn in another.
        for family in [
            "Montserrat Black",
            "Montserrat",
            "Roboto Bold",
            "Roboto",
            "Poppins",
            "THE BOLD FONT",
            "Komika Axis",
            "Fredoka Light",
            "Raleway Thin",
            "Merriweather Light 18pt",
        ] {
            assert!(
                metrics_for(family).is_some(),
                "font menu offers {family}, which the renderer cannot measure"
            );
        }

        let full = metrics_for("Montserrat Black").expect("bundled font");
        let family = metrics_for("Montserrat").expect("same file by family name");
        assert_eq!(
            full.measure("KANNETTAVA", 60.0),
            family.measure("KANNETTAVA", 60.0)
        );
    }
}
