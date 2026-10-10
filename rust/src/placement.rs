//! Automatic caption placement.
//!
//! Picks a height for each caption so it lands on the calmest part of the
//! picture instead of across a face or a busy background. The video is sampled
//! as small greyscale frames, each row is scored for how much is going on in it,
//! and each caption is then assigned to one of a handful of candidate heights.
//!
//! The assignment is a Viterbi pass rather than a per-caption minimum, because
//! captions that each independently pick their best spot jump around and read
//! far worse than captions that mostly stay put.

use crate::types::{AutoPlaceParams, AutoPlaceResult, PositionOverride};
use anyhow::{anyhow, Result};
use tokio::io::AsyncReadExt;
use tokio::process::Command as TokioCommand;

/// Frames sampled per second. Captions last a second or more, so a couple of
/// looks each is plenty and keeps one ffmpeg pass cheap.
const SAMPLE_FPS: u32 = 2;
/// Width the frames are scaled to before scoring. Placement is a decision about
/// horizontal bands, so detail beyond this buys nothing.
const SAMPLE_WIDTH: u32 = 160;

/// Candidate anchor heights, as a percentage of frame height.
///
/// Floored at the vertical middle of the frame, not just above any blocked
/// platform UI: a face is usually framed somewhere in the top two-thirds of a
/// portrait shot, and a plain wall or ceiling above someone's head can score
/// as perfectly calm even though putting text there covers them. Staying in
/// the lower half by default avoids that regardless of how "calm" the pixels
/// up there measure; the style's own (typically lower-third) position is
/// still favoured within this range via HOME_WEIGHT.
const MIN_BAND_PCT: f32 = 50.0;
const MAX_BAND_PCT: f32 = 92.0;
const BAND_COUNT: usize = 13;

/// How strongly captions resist moving away from the style's own position.
const HOME_WEIGHT: f32 = 0.35;
/// How strongly consecutive captions resist sitting at different heights.
const SWITCH_PENALTY: f32 = 0.40;
/// Weight of "this row is bright", which matters for light text.
const BRIGHTNESS_WEIGHT: f32 = 0.25;
/// Weight of sitting under a platform's own interface. Large enough to outrank
/// any picture-based argument — a caption the app covers cannot be read at all,
/// however calm the background behind it is — but still finite, so a frame with
/// nowhere clean left still gets a decision instead of nothing.
const BLOCKED_WEIGHT: f32 = 2.0;

/// What a caption needs for a placement decision.
#[derive(Debug, Clone)]
pub struct PlacementCaption {
    pub start_ms: u64,
    pub end_ms: u64,
    /// Where the style would put it, as a percentage of frame height.
    pub default_y_pct: f32,
    /// Height of the text block as a percentage of frame height.
    pub height_pct: f32,
    /// Which edge of the block sits at the anchor: "top", "center" or "bottom".
    pub anchor: String,
}

/// The candidate anchor positions, in percent from the top.
///
/// The style's own position is always among them. Without it the nearest grid
/// band wins by default and every caption comes back "moved" by a percent or
/// two, which buries the handful of real moves in noise.
pub fn candidate_bands(default_y_pct: f32) -> Vec<f32> {
    let mut bands: Vec<f32> = (0..BAND_COUNT)
        .map(|i| {
            MIN_BAND_PCT + (MAX_BAND_PCT - MIN_BAND_PCT) * (i as f32) / ((BAND_COUNT - 1) as f32)
        })
        .collect();

    if !bands.iter().any(|b| (b - default_y_pct).abs() < 1.0) {
        bands.push(default_y_pct);
        bands.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    }
    bands
}

/// The span a caption covers when its anchor sits at `y_pct`, as (top, bottom)
/// percentages of frame height.
pub fn caption_span(anchor: &str, y_pct: f32, height_pct: f32) -> (f32, f32) {
    match anchor {
        "top" => (y_pct, y_pct + height_pct),
        "center" => (y_pct - height_pct / 2.0, y_pct + height_pct / 2.0),
        _ => (y_pct - height_pct, y_pct),
    }
}

/// Sort and merge overlapping blocked stretches.
///
/// Two platforms selected at once overlap — TikTok's caption area and Shorts'
/// title area cover much of the same strip — and counting that strip twice
/// would make a partly-covered band look worse than a fully covered one.
pub fn merge_bands(bands: &[(f32, f32)]) -> Vec<(f32, f32)> {
    let mut sorted: Vec<(f32, f32)> = bands
        .iter()
        .map(|&(a, b)| if a <= b { (a, b) } else { (b, a) })
        .collect();
    sorted.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let mut merged: Vec<(f32, f32)> = Vec::with_capacity(sorted.len());
    for band in sorted {
        match merged.last_mut() {
            Some(last) if band.0 <= last.1 => last.1 = last.1.max(band.1),
            _ => merged.push(band),
        }
    }
    merged
}

/// How much of `top..bottom` falls inside already-merged blocked stretches,
/// as a fraction of the span.
pub fn blocked_fraction(top: f32, bottom: f32, blocked: &[(f32, f32)]) -> f32 {
    let span = bottom - top;
    if span <= 0.0 || blocked.is_empty() {
        return 0.0;
    }
    let covered: f32 = blocked
        .iter()
        .map(|&(start, end)| (end.min(bottom) - start.max(top)).max(0.0))
        .sum();
    (covered / span).clamp(0.0, 1.0)
}

/// Nudge a caption clear of the platform's own interface.
///
/// Applies to the style's chosen position — "bottom", "bottom quarter" and so
/// on — so picking a position does not silently put captions underneath the
/// app's buttons. Moves up by preference, since the room above a caption is
/// usually picture and the room below it is usually more interface. Returns the
/// anchor unchanged when nothing is in the way, or when there is nowhere clear
/// to go: a caption in an awkward spot still beats no caption.
pub fn dodge_blocked(y_pct: f32, anchor: &str, height_pct: f32, blocked: &[(f32, f32)]) -> f32 {
    if blocked.is_empty() || height_pct <= 0.0 {
        return y_pct;
    }

    let clears = |candidate: f32| {
        let (top, bottom) = caption_span(anchor, candidate, height_pct);
        top >= 0.0 && bottom <= 100.0 && blocked_fraction(top, bottom, blocked) <= 0.0
    };

    if clears(y_pct) {
        return y_pct;
    }

    // Try sitting just above each blocked stretch, nearest first, then just
    // below. Small gap so the caption does not touch the interface it dodged.
    const GAP: f32 = 1.0;
    let mut candidates: Vec<f32> = Vec::with_capacity(blocked.len() * 2);
    for &(start, end) in blocked {
        let above = start - GAP;
        let below = end + GAP;
        candidates.push(match anchor {
            "top" => above - height_pct,
            "center" => above - height_pct / 2.0,
            _ => above,
        });
        candidates.push(match anchor {
            "top" => below,
            "center" => below + height_pct / 2.0,
            _ => below + height_pct,
        });
    }

    candidates
        .into_iter()
        .filter(|&c| clears(c))
        .min_by(|a, b| {
            (a - y_pct)
                .abs()
                .partial_cmp(&(b - y_pct).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(y_pct)
}

/// Per-row "how much is going on here" for one sampled frame.
///
/// Combines gradient energy — edges, texture, faces — with how much the row
/// varies along its length, so a flat wall scores low even if it is bright.
/// Values are not normalised here; `normalise_profiles` does that across the
/// whole video so one busy shot cannot flatten the rest.
pub fn row_activity(gray: &[u8], width: usize, height: usize) -> Vec<f32> {
    if width < 3 || height < 3 || gray.len() < width * height {
        return vec![0.0; height];
    }

    let mut rows = Vec::with_capacity(height);

    for y in 0..height {
        // Rows on the very edge have no full neighbourhood; reuse the nearest.
        let yc = y.clamp(1, height - 2);

        let r_top = &gray[(yc - 1) * width..yc * width];
        let r_mid = &gray[yc * width..(yc + 1) * width];
        let r_bot = &gray[(yc + 1) * width..(yc + 2) * width];
        let r_curr = &gray[y * width..(y + 1) * width];

        let mut energy = 0.0f32;
        let mut sum = 0.0f32;
        let mut sum_sq = 0.0f32;

        for x in 1..width - 1 {
            let t_prev = r_top[x - 1] as f32;
            let t_next = r_top[x + 1] as f32;
            let m_prev = r_mid[x - 1] as f32;
            let m_next = r_mid[x + 1] as f32;
            let b_prev = r_bot[x - 1] as f32;
            let b_next = r_bot[x + 1] as f32;

            let gx = -t_prev - 2.0 * m_prev - b_prev + t_next + 2.0 * m_next + b_next;
            let gy = -t_prev - 2.0 * (r_top[x] as f32) - t_next
                + b_prev
                + 2.0 * (r_bot[x] as f32)
                + b_next;
            energy += gx.abs() + gy.abs();

            let v = r_curr[x] as f32;
            sum += v;
            sum_sq += v * v;
        }

        let n = (width - 2) as f32;
        let mean = sum / n;
        let variance = (sum_sq / n - mean * mean).max(0.0);

        // Gradient dominates; variance catches smooth but strong transitions.
        rows.push(energy / n / 255.0 + variance.sqrt() / 255.0);
    }

    rows
}

/// Mean brightness per row, 0..1.
pub fn row_brightness(gray: &[u8], width: usize, height: usize) -> Vec<f32> {
    if width == 0 || gray.len() < width * height {
        return vec![0.0; height];
    }
    let norm = (width as f32 * 255.0).max(1.0);
    (0..height)
        .map(|y| {
            let row = &gray[y * width..(y + 1) * width];
            let sum: u64 = row.iter().map(|&v| v as u64).sum();
            (sum as f32) / norm
        })
        .collect()
}

/// Scale profiles so the busiest row in the video is 1.0.
pub fn normalise_profiles(profiles: &mut [Vec<f32>]) {
    let peak = profiles
        .iter()
        .flat_map(|rows| rows.iter())
        .fold(0.0f32, |acc, v| acc.max(*v));
    if peak <= f32::EPSILON {
        return;
    }
    for rows in profiles.iter_mut() {
        for v in rows.iter_mut() {
            *v /= peak;
        }
    }
}

/// Cost of putting `caption` at each candidate band.
///
/// `activity` and `brightness` are the worst case across every frame the
/// caption is on screen for — a face that walks into shot halfway through still
/// counts against the band it walks into.
pub fn band_costs(
    caption: &PlacementCaption,
    bands: &[f32],
    activity: &[f32],
    brightness: &[f32],
    blocked: &[(f32, f32)],
) -> Vec<f32> {
    let rows = activity.len();
    bands
        .iter()
        .map(|&band| {
            let (top_pct, bottom_pct) = caption_span(&caption.anchor, band, caption.height_pct);

            // Off-frame placements are never worth considering.
            if top_pct < 0.0 || bottom_pct > 100.0 {
                return f32::INFINITY;
            }

            let first = ((top_pct / 100.0) * rows as f32).floor().max(0.0) as usize;
            let last = (((bottom_pct / 100.0) * rows as f32).ceil() as usize).min(rows);
            if first >= last {
                return f32::INFINITY;
            }

            let covered = (last - first) as f32;
            let busy: f32 = activity[first..last].iter().sum::<f32>() / covered;
            let bright: f32 = brightness[first..last].iter().sum::<f32>() / covered;

            // Staying near the style's own position is worth something on its
            // own; the picture has to actually argue for a move.
            let drift = (band - caption.default_y_pct).abs() / 100.0;
            let covered = blocked_fraction(top_pct, bottom_pct, blocked);

            busy + BRIGHTNESS_WEIGHT * bright + HOME_WEIGHT * drift + BLOCKED_WEIGHT * covered
        })
        .collect()
}

/// Assign every caption a band, trading each caption's own cost against the
/// cost of moving relative to the previous caption.
///
/// Straight Viterbi: `best[i][b]` is the cheapest way to reach band `b` at
/// caption `i`, and the winning path is walked back from the cheapest end state.
pub fn choose_bands(costs: &[Vec<f32>], switch_penalty: f32) -> Vec<usize> {
    if costs.is_empty() {
        return Vec::new();
    }
    let band_count = costs[0].len();
    if band_count == 0 {
        return vec![0; costs.len()];
    }

    let mut best: Vec<Vec<f32>> = vec![vec![f32::INFINITY; band_count]; costs.len()];
    let mut back: Vec<Vec<usize>> = vec![vec![0; band_count]; costs.len()];
    best[0].clone_from_slice(&costs[0]);

    for i in 1..costs.len() {
        for b in 0..band_count {
            let own = costs[i][b];
            let mut cheapest = f32::INFINITY;
            let mut from = 0usize;
            for (p, previous) in best[i - 1].iter().enumerate() {
                if !previous.is_finite() {
                    continue;
                }
                let moved = if p == b { 0.0 } else { switch_penalty };
                let total = previous + moved;
                if total < cheapest {
                    cheapest = total;
                    from = p;
                }
            }
            best[i][b] = cheapest + own;
            back[i][b] = from;
        }
    }

    // Walk back from the cheapest final state.
    let mut path = vec![0usize; costs.len()];
    let last = costs.len() - 1;
    let mut current = (0..band_count)
        .min_by(|&a, &b| {
            best[last][a]
                .partial_cmp(&best[last][b])
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(0);

    path[last] = current;
    for i in (1..costs.len()).rev() {
        current = back[i][current];
        path[i - 1] = current;
    }
    path
}

/// Turn chosen bands into overrides, dropping the ones that agree with the
/// style so the editor only shows genuinely moved captions.
pub fn overrides_from_bands(
    captions: &[PlacementCaption],
    bands: &[f32],
    chosen: &[usize],
) -> Vec<PositionOverride> {
    captions
        .iter()
        .zip(chosen)
        .filter_map(|(caption, &band_index)| {
            let y_pct = *bands.get(band_index)?;
            if (y_pct - caption.default_y_pct).abs() < 0.5 {
                return None;
            }
            Some(PositionOverride {
                start_ms: caption.start_ms,
                end_ms: caption.end_ms,
                y_pct,
            })
        })
        .collect()
}

/// Default switching penalty, exposed so callers do not repeat the constant.
pub fn switch_penalty() -> f32 {
    SWITCH_PENALTY
}

/// One sampled frame, reduced to the two profiles placement cares about.
struct FrameProfile {
    activity: Vec<f32>,
    brightness: Vec<f32>,
}

/// Walk the video once, emitting small greyscale frames, and reduce each to its
/// row profiles as it arrives. Only the profiles are kept, so a long video costs
/// kilobytes rather than the tens of megabytes the frames themselves would.
async fn sample_profiles(
    input_video: &str,
    target_w: u32,
    target_h: u32,
    crop_strategy: &str,
    is_hdr: bool,
) -> Result<Vec<FrameProfile>> {
    // Decoding is the whole cost of this walk, so it goes to VideoToolbox where
    // there is one; frames come back to system memory for the greyscale filters.
    // If that run yields nothing (a source the GPU decoder refuses), walk again
    // the way every machine can.
    if crate::video::get_best_hardware_encoder().await
        == crate::video::HardwareEncoder::VideoToolbox
        && !is_hdr
    {
        if let Ok(profiles) =
            sample_profiles_with(input_video, target_w, target_h, crop_strategy, is_hdr, true).await
        {
            return Ok(profiles);
        }
    }
    sample_profiles_with(
        input_video,
        target_w,
        target_h,
        crop_strategy,
        is_hdr,
        false,
    )
    .await
}

async fn sample_profiles_with(
    input_video: &str,
    target_w: u32,
    target_h: u32,
    crop_strategy: &str,
    is_hdr: bool,
    gpu_decode: bool,
) -> Result<Vec<FrameProfile>> {
    let sample_h = crate::video::round_even(
        ((SAMPLE_WIDTH as f32) * (target_h as f32) / (target_w as f32)).round() as u32,
    )
    .max(2);

    let mut vf = crate::video::build_fitpad_filter_with_options(
        target_w,
        target_h,
        None,
        crate::video::HardwareEncoder::Software,
        crop_strategy,
        is_hdr,
    );
    vf.push_str(&format!(
        ",fps={},scale={}:{}:flags=bilinear,format=gray",
        SAMPLE_FPS, SAMPLE_WIDTH, sample_h
    ));

    let mut command = TokioCommand::new(crate::video::get_ffmpeg_path_sync());
    if gpu_decode {
        command.args(crate::video::vt_decode_args(false));
    }
    let mut child = command
        .arg("-i")
        .arg(input_video)
        .arg("-an")
        .arg("-threads")
        .arg("2")
        .arg("-vf")
        .arg(&vf)
        .arg("-f")
        .arg("rawvideo")
        .arg("-pix_fmt")
        .arg("gray")
        .arg("-")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| anyhow!("Failed to run ffmpeg for placement sampling: {}", e))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("ffmpeg produced no output to sample"))?;

    let mut reader = tokio::io::BufReader::with_capacity(65536, stdout);

    let frame_bytes = (SAMPLE_WIDTH * sample_h) as usize;
    let mut buffer = vec![0u8; frame_bytes];
    let mut profiles = Vec::new();

    // A short read just means the stream ended, possibly mid-frame.
    while reader.read_exact(&mut buffer).await.is_ok() {
        profiles.push(FrameProfile {
            activity: row_activity(&buffer, SAMPLE_WIDTH as usize, sample_h as usize),
            brightness: row_brightness(&buffer, SAMPLE_WIDTH as usize, sample_h as usize),
        });
    }

    let _ = child.wait().await;

    if profiles.is_empty() {
        return Err(anyhow!(
            "Could not read any frames from the video for placement"
        ));
    }
    Ok(profiles)
}

/// Worst case across the frames a caption is on screen for.
///
/// Worst rather than average: a face that enters halfway through still rules
/// out the band it enters, which is what a viewer would notice.
fn worst_case(profiles: &[FrameProfile], start_ms: u64, end_ms: u64) -> (Vec<f32>, Vec<f32>) {
    let rows = profiles[0].activity.len();
    let per_frame_ms = 1000.0 / SAMPLE_FPS as f32;

    let first = ((start_ms as f32) / per_frame_ms).floor() as usize;
    let last = ((end_ms as f32) / per_frame_ms).ceil() as usize;
    let first = first.min(profiles.len() - 1);
    let last = last.clamp(first + 1, profiles.len());

    let mut activity = vec![0.0f32; rows];
    let mut brightness = vec![0.0f32; rows];
    for profile in &profiles[first..last] {
        for r in 0..rows {
            activity[r] = activity[r].max(profile.activity[r]);
            brightness[r] = brightness[r].max(profile.brightness[r]);
        }
    }
    (activity, brightness)
}

/// Choose a height for every caption in a video.
pub async fn auto_place_captions(params: AutoPlaceParams) -> Result<AutoPlaceResult> {
    if params.segments.is_empty() {
        return Err(anyhow!("No captions to place"));
    }

    let probe_result = crate::video::probe("auto_place_probe", &params.input_video, |_| {}).await?;
    let src_w = probe_result.width.unwrap_or(1920) as u32;
    let src_h = probe_result.height.unwrap_or(1080) as u32;
    let (target_w, target_h) = if params.export_format == "source" {
        (src_w, src_h)
    } else {
        let target_ar = crate::video::parse_target_ar(&params.export_format)?;
        crate::video::target_dimensions(params.output_size.as_deref(), src_w, src_h, target_ar)
    };

    // Ask the caption renderer where the captions are and how tall they get, so
    // placement reasons about the same blocks the viewer will see.
    let layout = crate::captions::generate_preview_layout(crate::types::PreviewLayoutParams {
        segments: params.segments.clone(),
        width: target_w,
        height: target_h,
        font_name: params.font_name.clone(),
        font_size: params.font_size,
        text_color: params.text_color.clone(),
        highlight_word_color: params.highlight_word_color.clone(),
        outline_color: params.outline_color.clone(),
        outline_width: params.outline_width,
        background_box: params.background_box,
        position: params.position.clone(),
        karaoke: params.karaoke,
        multiline: params.multiline,
        justify_lines: params.justify_lines,
        glow_effect: params.glow_effect,
        position_overrides: Vec::new(),
        blocked_bands: Vec::new(),
        // Already the export canvas: target_w/target_h above.
        export_format: None,
    })?;

    // ASS lines sit about 1.2 line heights apart; the outline adds a little.
    let line_height_pct = (layout.font_size_px as f32 * 1.25) / target_h as f32 * 100.0;

    let mut captions: Vec<PlacementCaption> = Vec::new();
    for cue in &layout.cues {
        if captions.last().is_some_and(|last| {
            last.start_ms == cue.group_start_ms && last.end_ms == cue.group_end_ms
        }) {
            continue;
        }
        captions.push(PlacementCaption {
            start_ms: cue.group_start_ms,
            end_ms: cue.group_end_ms,
            default_y_pct: cue.y_pct,
            height_pct: line_height_pct * cue.lines.len().max(1) as f32,
            anchor: cue.anchor.clone(),
        });
    }

    if captions.is_empty() {
        return Err(anyhow!("No captions to place"));
    }

    let mut profiles = sample_profiles(
        &params.input_video,
        target_w,
        target_h,
        params.crop_strategy.as_deref().unwrap_or("fit"),
        crate::video::is_hdr(&probe_result),
    )
    .await?;

    let mut activity_profiles: Vec<Vec<f32>> = profiles
        .iter()
        .map(|profile| profile.activity.clone())
        .collect();
    normalise_profiles(&mut activity_profiles);
    for (profile, normalised) in profiles.iter_mut().zip(activity_profiles) {
        profile.activity = normalised;
    }

    let bands = candidate_bands(captions[0].default_y_pct);
    let blocked = merge_bands(&params.blocked_bands);
    let costs: Vec<Vec<f32>> = captions
        .iter()
        .map(|caption| {
            let (activity, brightness) = worst_case(&profiles, caption.start_ms, caption.end_ms);
            band_costs(caption, &bands, &activity, &brightness, &blocked)
        })
        .collect();

    let chosen = choose_bands(&costs, switch_penalty());
    let position_overrides = overrides_from_bands(&captions, &bands, &chosen);

    Ok(AutoPlaceResult {
        moved: position_overrides.len(),
        total: captions.len(),
        position_overrides,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caption(start_ms: u64, end_ms: u64) -> PlacementCaption {
        PlacementCaption {
            start_ms,
            end_ms,
            default_y_pct: 88.0,
            height_pct: 10.0,
            anchor: "bottom".to_string(),
        }
    }

    #[test]
    fn caption_span_follows_the_anchor() {
        assert_eq!(caption_span("bottom", 88.0, 10.0), (78.0, 88.0));
        assert_eq!(caption_span("top", 12.0, 10.0), (12.0, 22.0));
        assert_eq!(caption_span("center", 50.0, 10.0), (45.0, 55.0));
    }

    #[test]
    fn row_activity_sees_an_edge() {
        // Top half black, bottom half white: the busiest row is the seam.
        let (w, h) = (16usize, 16usize);
        let mut gray = vec![0u8; w * h];
        for y in h / 2..h {
            for x in 0..w {
                gray[y * w + x] = 255;
            }
        }

        let rows = row_activity(&gray, w, h);
        let busiest = rows
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap()
            .0;
        assert!(
            (busiest as i32 - (h / 2) as i32).abs() <= 1,
            "expected the seam at row {}, got {}",
            h / 2,
            busiest
        );
        // A flat region has nothing going on.
        assert!(rows[1] < 0.01);
    }

    #[test]
    fn band_costs_prefer_the_calm_half() {
        // Tests the cost function itself, independent of candidate_bands'
        // default-range policy (which floors real placement at 50% so it
        // never climbs into the top half a face usually occupies — see
        // MIN_BAND_PCT). Bottom half of these explicit bands is busy, top
        // half is calm.
        let rows = 100;
        let activity: Vec<f32> = (0..rows).map(|y| if y >= 50 { 1.0 } else { 0.0 }).collect();
        let brightness = vec![0.0; rows];
        let bands = vec![10.0, 30.0, 49.0, 70.0, 90.0];

        let costs = band_costs(&caption(0, 1000), &bands, &activity, &brightness, &[]);
        let cheapest = costs
            .iter()
            .enumerate()
            .min_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap()
            .0;

        assert!(
            bands[cheapest] < 50.0,
            "expected a band in the calm top half, got {}",
            bands[cheapest]
        );
    }

    #[test]
    fn dodge_lifts_the_bottom_position_clear_of_the_interface() {
        // TikTok: tabs at the top, caption strip and nav along the bottom.
        let blocked = merge_bands(&[(0.0, 8.0), (78.0, 95.0), (95.0, 100.0)]);

        // The default bottom position lands inside the caption strip.
        let moved = dodge_blocked(88.0, "bottom", 7.5, &blocked);
        assert!(moved < 78.0, "expected a lift clear of 78%, got {}", moved);
        let (top, bottom) = caption_span("bottom", moved, 7.5);
        assert_eq!(blocked_fraction(top, bottom, &blocked), 0.0);
    }

    #[test]
    fn dodge_leaves_a_position_that_is_already_clear() {
        let blocked = merge_bands(&[(0.0, 8.0), (78.0, 100.0)]);
        // A quarter from the bottom already sits above the interface.
        assert_eq!(dodge_blocked(75.0, "bottom", 7.5, &blocked), 75.0);
        assert_eq!(dodge_blocked(50.0, "center", 7.5, &blocked), 50.0);
    }

    #[test]
    fn dodge_accounts_for_how_tall_the_caption_is() {
        let blocked = merge_bands(&[(78.0, 100.0)]);
        // A single line clears at a quarter from the bottom...
        assert_eq!(dodge_blocked(75.0, "bottom", 7.5, &blocked), 75.0);
        // ...but a four-line storyteller block anchored at the same point does
        // too, because it grows upwards.
        assert_eq!(dodge_blocked(75.0, "bottom", 30.0, &blocked), 75.0);
        // Centred, though, half of it would hang into the blocked strip.
        let moved = dodge_blocked(75.0, "center", 30.0, &blocked);
        let (top, bottom) = caption_span("center", moved, 30.0);
        assert_eq!(blocked_fraction(top, bottom, &blocked), 0.0);
        assert!(top >= 0.0 && bottom <= 100.0);
    }

    #[test]
    fn dodge_gives_up_rather_than_pushing_a_caption_off_screen() {
        // Nowhere to go: the whole frame is spoken for.
        let blocked = merge_bands(&[(0.0, 100.0)]);
        assert_eq!(dodge_blocked(88.0, "bottom", 7.5, &blocked), 88.0);
    }

    #[test]
    fn merge_bands_unions_overlapping_stretches() {
        // TikTok's caption strip and Shorts' title strip overlap.
        let merged = merge_bands(&[(78.0, 95.0), (82.0, 93.0), (0.0, 8.0)]);
        assert_eq!(merged, vec![(0.0, 8.0), (78.0, 95.0)]);
    }

    #[test]
    fn merge_bands_tolerates_reversed_input() {
        assert_eq!(merge_bands(&[(95.0, 78.0)]), vec![(78.0, 95.0)]);
    }

    #[test]
    fn blocked_fraction_measures_the_covered_share() {
        let blocked = vec![(78.0, 95.0)];
        // Fully inside.
        assert_eq!(blocked_fraction(80.0, 90.0, &blocked), 1.0);
        // Half in, half out.
        assert_eq!(blocked_fraction(73.0, 83.0, &blocked), 0.5);
        // Clear of it.
        assert_eq!(blocked_fraction(40.0, 50.0, &blocked), 0.0);
    }

    #[test]
    fn a_platform_overlay_pushes_captions_off_a_calm_but_covered_strip() {
        // The exact case the overlay exposed: the bottom of the frame is the
        // calmest place to put text, but it is where TikTok draws its own
        // caption, so the caption has to go up instead.
        let rows = 100;
        let activity: Vec<f32> = (0..rows).map(|y| if y < 70 { 0.5 } else { 0.0 }).collect();
        let brightness = vec![0.0; rows];
        let bands = candidate_bands(88.0);
        let blocked = merge_bands(&[(78.0, 95.0), (95.0, 100.0)]);

        let unaware = band_costs(&caption(0, 1000), &bands, &activity, &brightness, &[]);
        let aware = band_costs(&caption(0, 1000), &bands, &activity, &brightness, &blocked);

        let cheapest = |costs: &[f32]| {
            costs
                .iter()
                .enumerate()
                .min_by(|a, b| a.1.partial_cmp(b.1).unwrap())
                .unwrap()
                .0
        };

        // Judge the strip the text actually occupies, not its anchor: anchored
        // at its bottom edge, a caption sits well above the point it is placed.
        let covered_share = |costs: &[f32]| {
            let band = bands[cheapest(costs)];
            let (top, bottom) = caption_span("bottom", band, 10.0);
            blocked_fraction(top, bottom, &blocked)
        };

        // Left alone it settles into the calm — and covered — bottom.
        assert!(covered_share(&unaware) > 0.9);
        // Told about the platform, it climbs nearly clear of it.
        assert!(
            covered_share(&aware) < 0.2,
            "expected the text to clear the platform overlay, {:.0}% still covered",
            covered_share(&aware) * 100.0
        );
    }

    #[test]
    fn band_costs_reject_placements_that_fall_off_the_frame() {
        let bands = vec![2.0, 50.0];
        let activity = vec![0.0; 100];
        let brightness = vec![0.0; 100];

        // A 10%-tall caption anchored at its bottom cannot sit at y=2%.
        let costs = band_costs(&caption(0, 1000), &bands, &activity, &brightness, &[]);
        assert!(costs[0].is_infinite());
        assert!(costs[1].is_finite());
    }

    #[test]
    fn choose_bands_holds_still_for_a_single_bad_frame() {
        // Band 0 is best for every caption except the third, where band 1 wins
        // by a hair. Moving costs more than that, so the run should stay put.
        let costs = vec![
            vec![0.0, 1.0],
            vec![0.0, 1.0],
            vec![0.3, 0.0],
            vec![0.0, 1.0],
        ];
        assert_eq!(choose_bands(&costs, 0.4), vec![0, 0, 0, 0]);
    }

    #[test]
    fn choose_bands_moves_when_the_picture_insists() {
        // Band 1 wins by a wide margin for a sustained run.
        let costs = vec![
            vec![0.0, 1.0],
            vec![2.0, 0.0],
            vec![2.0, 0.0],
            vec![2.0, 0.0],
        ];
        let path = choose_bands(&costs, 0.4);
        assert_eq!(path[1..], [1, 1, 1]);
    }

    #[test]
    fn choose_bands_handles_an_empty_run() {
        assert!(choose_bands(&[], 0.4).is_empty());
    }

    #[test]
    fn overrides_skip_captions_left_where_the_style_put_them() {
        let captions = vec![caption(0, 1000), caption(1000, 2000)];
        let bands = vec![30.0, 88.0];
        // First caption moves to 30%, second stays at the style default.
        let overrides = overrides_from_bands(&captions, &bands, &[0, 1]);

        assert_eq!(overrides.len(), 1);
        assert_eq!(overrides[0].start_ms, 0);
        assert_eq!(overrides[0].y_pct, 30.0);
    }

    #[test]
    fn normalise_scales_the_busiest_row_to_one() {
        let mut profiles = vec![vec![0.0, 2.0], vec![1.0, 4.0]];
        normalise_profiles(&mut profiles);
        assert_eq!(profiles[1][1], 1.0);
        assert_eq!(profiles[0][1], 0.5);
    }
}
