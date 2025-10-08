use anyhow::{anyhow, Result};
use crate::rpc::RpcEvent;
use crate::types::{CaptionSegment, WordSpan, GenerateCaptionsParams, GenerateCaptionsResult, CaptionedVideoResult, ExtractAudioParams, TranscribeSegmentsParams};
use crate::video::probe;
use crate::{audio, whisper};
use std::{fs, path::PathBuf, process::Command};
use std::collections::{HashMap, HashSet, VecDeque};

pub async fn generate_captions(
    id: &str,
    params: GenerateCaptionsParams,
    emit: impl FnMut(RpcEvent)
) -> Result<GenerateCaptionsResult> {
    generate_captions_single_pass(id, params, emit).await
}

pub async fn generate_captions_single_pass(
    id: &str,
    params: GenerateCaptionsParams,
    mut emit: impl FnMut(RpcEvent)
) -> Result<GenerateCaptionsResult> {

    let temp_dir = std::env::temp_dir().join(format!("capslap_captions_{}", id));
    if let Err(e) = fs::create_dir_all(&temp_dir) {
        return Err(anyhow!("Failed to create temp directory: {}", e));
    }

    let probe_result = probe(id, &params.input_video, &mut emit).await?;

    let audio_filename = format!("audio_{}.mp3", id);
    let temp_audio_path = temp_dir.join(&audio_filename);
    let audio_params = ExtractAudioParams {
        input: params.input_video.clone(),
        codec: Some("mp3".to_string()),
        out: Some(temp_audio_path.to_string_lossy().to_string()),
    };
    let audio_result = audio::extract_audio(id, audio_params, &mut emit).await?;


    let transcribe_params = TranscribeSegmentsParams {
        audio: audio_result.audio.clone(),
        model: params.model,
        language: params.language,
        split_by_words: params.split_by_words,
        api_key: params.api_key.clone(),
        prompt: params.prompt,
        video_file: Some(params.input_video.clone()),
    };
    let transcription = whisper::transcribe_segments_with_temp(id, transcribe_params, Some(&temp_dir), &mut emit).await?;

    let captioned_videos = optimized_multi_format_encode(
        id,
        &params.input_video,
        &transcription.segments,
        &params.export_formats,
        &probe_result,
        &temp_dir,
        params.font_name,
        params.text_color,
        params.highlight_word_color,
        params.outline_color,
        params.glow_effect,
        params.karaoke,
        params.position,
        &mut emit
    ).await?;

    Ok(GenerateCaptionsResult {
        probe_result,
        audio_file: audio_result.audio,
        transcription,
        captioned_videos,
    })
}

async fn optimized_multi_format_encode(
    id: &str,
    input_video: &str,
    segments: &[CaptionSegment],
    export_formats: &[String],
    probe_result: &crate::video::ProbeResult,
    temp_dir: &PathBuf,
    font_name: Option<String>,
    text_color: Option<String>,
    highlight_word_color: Option<String>,
    outline_color: Option<String>,
    glow_effect: bool,
    karaoke: bool,
    position: Option<String>,
    emit: &mut impl FnMut(RpcEvent)
) -> Result<Vec<CaptionedVideoResult>> {
    if export_formats.is_empty() {
        return Err(anyhow!("No export formats specified"));
    }

    let input_path = std::path::Path::new(input_video)
        .with_extension("")
        .to_string_lossy()
        .to_string();

    // Pre-generate shared ASS files for each format (avoiding redundant subtitle processing)
    let mut format_ass_files = Vec::new();
    for format in export_formats {
        let target_ar = crate::video::parse_target_ar(format)?;
        let src_w = probe_result.width.unwrap_or(1920) as u32;
        let src_h = probe_result.height.unwrap_or(1080) as u32;
        let (target_w, target_h) = crate::video::canvas_no_downscale(src_w, src_h, target_ar);

        // Build ASS subtitle file optimized for this format
        let style = default_ass_style(
            target_w, target_h,
            font_name.as_deref(),
            text_color.as_deref(),
            highlight_word_color.as_deref(),
            outline_color.as_deref(),
            glow_effect,
            position.as_deref()
        );
        let ass_doc = build_ass_document(target_w, target_h, &style, segments, karaoke, glow_effect)?;

        let safe_format = format.replace(':', "x");
        let ass_filename = format!("captions_{}_{}.ass", id, safe_format);
        let ass_path = temp_dir.join(&ass_filename);
        fs::write(&ass_path, ass_doc)?;

        format_ass_files.push((format.clone(), ass_path, target_w, target_h));
    }

    // Process formats with limited concurrency (2 at a time for optimal resource usage)
    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(2));
    let mut tasks = Vec::new();

    for (idx, (format, ass_path, target_w, target_h)) in format_ass_files.into_iter().enumerate() {
        let format = format.clone();
        let input_video = input_video.to_string();
        let probe_result = probe_result.clone();
        let semaphore = semaphore.clone();
        let task_id = format!("{}_{}", id, idx);
        let input_path = input_path.clone();

        let task = tokio::spawn(async move {
            // Acquire semaphore permit for bounded concurrency
            let _permit = semaphore.acquire().await.unwrap();

            let safe_format = format.replace(':', "x");
            let captioned_path = format!("{}_{}.mp4", input_path, safe_format);

            // Single-pass format conversion + caption burning with hardware acceleration
            optimized_single_format_encode(
                &task_id,
                &input_video,
                &ass_path,
                &captioned_path,
                target_w,
                target_h,
                &probe_result,
            ).await?;

            Ok::<CaptionedVideoResult, anyhow::Error>(CaptionedVideoResult {
                format,
                raw_video: "".to_string(),
                captioned_video: captioned_path,
                width: target_w,
                height: target_h,
            })
        });

        tasks.push(task);
    }

    // Wait for all tasks to complete and collect results
    let mut captioned_videos = Vec::new();
    for (idx, task) in tasks.into_iter().enumerate() {
        let result = task.await.map_err(|e| anyhow!("Concurrent task failed: {}", e))??;
        captioned_videos.push(result);
    }

    Ok(captioned_videos)
}

/// Optimized single format encoding with hardware acceleration and modern FFmpeg flags
async fn optimized_single_format_encode(
    id: &str,
    input_video: &str,
    ass_path: &PathBuf,
    output_path: &str,
    target_w: u32,
    target_h: u32,
    probe_result: &crate::video::ProbeResult,
) -> Result<()> {
    // Determine the best available hardware encoder for H.264 first (for filter optimization)
    let hardware_encoder = crate::video::get_best_hardware_encoder().await;

    // Build optimized filter with format conversion AND subtitles in one pass
    // Use encoder-specific format optimization (NV12 for VideoToolbox/NVENC, yuv420p for software)
    let ass = ass_path.to_string_lossy().to_string();
    let vf = crate::video::build_fitpad_filter_with_format(target_w, target_h, Some(&ass), hardware_encoder);

    // Determine optimal audio codec and settings
    let (audio_codec, audio_args) = crate::video::determine_audio_codec(Some(probe_result));

    // Calculate GOP size based on original video FPS for better seeking
    let gop_size = if let Some(fps) = probe_result.fps {
        (fps * 2.0).round() as u32
    } else {
        48 // Default for 24fps content
    };
    let gop_size_str = gop_size.to_string();

    // Resolve FFmpeg path using unified async detector (bundled > project > system)
    let ffmpeg_path = crate::whisper::find_ffmpeg_binary()
        .await
        .map_err(|e| anyhow!("FFmpeg not found: {}", e))?;

    let status = Command::new(ffmpeg_path)
        .args({
            let mut args = vec![
                "-y", "-i", input_video,
                "-vf", &vf,
                "-fps_mode", "passthrough",       // Modern replacement for -vsync
                "-threads", "0",                  // Use all available CPU cores
                "-map", "0:v:0",                  // Map first video stream
                "-map", "0:a?",                   // Map audio if present (optional)
            ];

            // Add hardware-optimized encoding parameters
            match hardware_encoder {
                crate::video::HardwareEncoder::VideoToolbox => {
                    args.extend_from_slice(&[
                        "-c:v", "h264_videotoolbox",
                        "-b:v", "0",
                        "-crf", "16",
                        "-allow_sw", "1",
                        "-g", &gop_size_str,
                        "-pix_fmt", "yuv420p",
                    ]);
                },
                crate::video::HardwareEncoder::Nvenc => {
                    args.extend_from_slice(&[
                        "-c:v", "h264_nvenc",
                        "-cq", "16",
                        "-preset", "p5",
                        "-tune", "hq",
                        "-rc", "vbr",
                        "-g", &gop_size_str,
                        "-pix_fmt", "yuv420p",
                    ]);
                },
                crate::video::HardwareEncoder::Software => {
                    args.extend_from_slice(&[
                        "-c:v", "libx264",
                        "-preset", "medium",
                        "-crf", "16",
                        "-g", &gop_size_str,
                        "-pix_fmt", "yuv420p",
                    ]);
                }
            }

            args.push("-c:a");
            args.push(&audio_codec);

            // Add audio-specific args
            args.extend(audio_args.iter().copied());

            // Add explicit bitrate for re-encoded audio if not using copy
            if audio_codec != "copy" && audio_codec == "aac" && audio_args.is_empty() {
                args.extend_from_slice(&["-b:a", "160k"]);
            }

            args.extend_from_slice(&[
                "-movflags", "+faststart",       // Fast web playback
                output_path
            ]);
            args
        })
        .status()?;

    if !status.success() {
        return Err(anyhow!("FFmpeg failed to encode format for {}", id));
    }

    Ok(())
}


// ---- Constants for horizontal stretch animation ----
const STRETCH_X_PEAK: f32 = 1.03;  // 1.08–1.15 looks right
const STRETCH_UP_MIN_MS: i64 = 0;
const STRETCH_UP_MAX_MS: i64 = 150;
const BIG_FONT_SIZE_MULTIPLIER: f32 = 1.1;

// ---- Constants for bounce animation (non-karaoke) ----
const BOUNCE_START: f32 = 0.85;   // 95%
const BOUNCE_PEAK: f32 = 1.05;    // 103%
const BOUNCE_END: f32 = 1.0;      // 100%
const BOUNCE_UP_MS: i64 = 100;    // Time to reach peak
const BOUNCE_DOWN_MS: i64 = 66;  // Time to settle

// ---- Smart highlight tuning (non-karaoke) ----
const HL_BASE_T: f32 = 2.5;         // base threshold
const HL_HYSTERESIS: f32 = 0.7;     // make back-to-back highlights harder
const HL_MIN_GAP_MS: u64 = 1200;    // min time between highlights
const HL_MAX_RATIO: f32 = 0.35;     // cap ~35% of phrases highlighted
const HL_RECENT_WINDOW_MS: u64 = 5000; // window for repetition penalty

fn push_glow_and_stroke(
    lines: &mut String,
    start: &str, end: &str,
    text_body: &str,      // ONLY \1c, \fs, \t(...). No \bord/\blur/\shad here.
    x: i32, y: i32,
    stroke_w: f32,        // black outline width
    enable_glow: bool,    // whether to apply glow effect
    glow_w: f32, glow_blur: f32, glow_alpha_hex: &str, // e.g. "&H80" ~ 50% opacity
    alignment: u32,       // ASS alignment value (2 = bottom center, 5 = middle center)
) {
    let common = format!("{{\\an{}\\q2\\pos({},{})\\be0}}", alignment, x, y);

    // LAYER 0 — soft WHITE GLOW (outline only) - only if enabled
    if enable_glow {
        // hide fill (\1a&HFF), set white outline (\3c), set opacity (\3a), add blur
        let glow = format!(
            "{}{{\\1a&HFF\\bord{:.2}\\3c&HFFFFFF&\\3a{}\\blur{:.2}\\shad0}}",
            common, glow_w, glow_alpha_hex, glow_blur
        );
        lines.push_str(&format!("Dialogue: 0,{},{},TikTok,,0,0,0,,{}{}\n", start, end, glow, text_body));
    }

    // LAYER 1 (or 0 if no glow) — sharp black stroke + visible fill
    let layer = if enable_glow { 1 } else { 0 };
    let stroke_fill = format!(
        "{}{{\\1a&H00\\bord{:.2}\\3c&H000000&\\3a&H00\\blur0\\shad0}}",
        common, stroke_w
    );
    lines.push_str(&format!("Dialogue: {},{},{},TikTok,,0,0,0,,{}{}\n", layer, start, end, stroke_fill, text_body));
}

#[derive(Clone)]
#[allow(dead_code)]
struct Phrase {
    start_ms: u64,
    end_ms: u64,
    tokens: Vec<String>,     // plain words for layout
    spans:  Vec<WordSpan>,   // timings per token (same length as tokens)
}

// Heuristics: new phrase if punctuation on previous token or gap > 350ms or length > 3 words
fn coalesce_phrases(segments: &[CaptionSegment]) -> Vec<Phrase> {
    let mut all: Vec<WordSpan> = Vec::new();
    for s in segments {
        for w in &s.words {
            let t = w.text.trim();
            if !t.is_empty() { all.push(WordSpan { start_ms: w.start_ms, end_ms: w.end_ms, text: t.to_string() }); }
        }
        // Fallback: if a segment has text but no words, split evenly so nothing gets dropped
        if s.words.is_empty() && !s.text.trim().is_empty() {
            let toks: Vec<_> = s.text.split_whitespace().collect();
            let total = (s.end_ms - s.start_ms).max(1);
            let per = total / (toks.len().max(1) as u64);
            let mut t = s.start_ms;
            for tok in toks {
                let s0 = t; let e0 = (t + per).min(s.end_ms); t = e0;
                all.push(WordSpan { start_ms: s0, end_ms: e0, text: tok.to_string() });
            }
        }
    }

    let mut out: Vec<Phrase> = Vec::new();
    let mut cur: Vec<WordSpan> = Vec::new();
    for w in all.into_iter() {
        if cur.is_empty() { cur.push(w); continue; }
        let prev = cur.last().unwrap();
        let gap = w.start_ms.saturating_sub(prev.end_ms);
        let hard_break = [".","!","?"].iter().any(|p| prev.text.ends_with(p)) || gap > 350 || cur.len() >= 3;
        if hard_break {
            let tokens = cur.iter().map(|x| x.text.clone()).collect::<Vec<_>>();
            out.push(Phrase{ start_ms: cur.first().unwrap().start_ms, end_ms: cur.last().unwrap().end_ms, tokens, spans: cur.clone() });
            cur = vec![w];
        } else {
            cur.push(w);
        }
    }
    if !cur.is_empty() {
        let tokens = cur.iter().map(|x| x.text.clone()).collect::<Vec<_>>();
        out.push(Phrase{ start_ms: cur.first().unwrap().start_ms, end_ms: cur.last().unwrap().end_ms, tokens, spans: cur.clone() });
    }
    out
}


// ---- time quantization (ASS is 1/100s) ----
fn ms_to_cs(ms: u64) -> i64 { (ms / 10) as i64 }
fn cs_to_ass(cs: i64) -> String {
    let total = cs.max(0);
    let h = total / 360000; // 3600*100
    let m = (total % 360000) / 6000;
    let s = (total % 6000) / 100;
    let c = total % 100;
    format!("{:01}:{:02}:{:02}.{:02}", h, m, s, c)
}

// Contiguous, non-overlapping windows in cs
fn contiguous_cs_windows(words: &[WordSpan]) -> Vec<(i64,i64)> {
    let mut out = Vec::with_capacity(words.len());
    for (i, w) in words.iter().enumerate() {
        let s = ms_to_cs(w.start_ms);
        let e = if i + 1 < words.len() {
            ms_to_cs(words[i+1].start_ms) // [s, next_s)
        } else {
            ms_to_cs(w.end_ms)           // last word keeps its end
        };
        out.push((s, (e.max(s+1)))); // at least 1 cs
    }
    out
}

// Block stretch tag: X goes from peak -> 100%, Y stays 100%
fn stretch_tag_ms(dur_ms: i64) -> String {
    let up = dur_ms.clamp(STRETCH_UP_MIN_MS, STRETCH_UP_MAX_MS);
    let px = (STRETCH_X_PEAK * 100.0).round() as u32;
    format!(r"{{\fscx{px}\fscy100\t(0,{up},\fscx100)}}")
}

// Bounce animation: 95% → 103% → 100% (nice entrance effect)
fn bounce_tag() -> String {
    let start = (BOUNCE_START * 100.0).round() as u32;
    let peak = (BOUNCE_PEAK * 100.0).round() as u32;
    let end_val = (BOUNCE_END * 100.0).round() as u32;
    format!(r"{{\fscx{start}\fscy{start}\t(0,{},\fscx{peak}\fscy{peak})\t({},{},\fscx{end_val}\fscy{end_val})}}",
            BOUNCE_UP_MS, BOUNCE_UP_MS, BOUNCE_UP_MS + BOUNCE_DOWN_MS)
}

// Uppercase + sanitize tokens (keeps punctuation)
fn normalize_tokens(words: &[WordSpan]) -> Vec<String> {
    words.iter()
        .map(|w| w.text.trim())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_uppercase())
        .collect()
}

// Simple width check for karaoke - split long phrases into single-line segments
fn split_phrase_for_width(tokens: &[String], spans: &[WordSpan], frame_w: u32, font_px: u32) -> Vec<(Vec<String>, Vec<WordSpan>)> {
    let est_char_width = (font_px as f32 * 0.56).max(1.0);
    let max_chars = ((frame_w as f32 * 0.85) / est_char_width).floor() as usize; // Use 85% of width for safety

    let mut segments = Vec::new();
    let mut current_tokens = Vec::new();
    let mut current_spans = Vec::new();
    let mut current_length = 0;

    for (token, span) in tokens.iter().zip(spans.iter()) {
        let token_length = token.len() + if current_length == 0 { 0 } else { 1 }; // Add space

        if current_length > 0 && current_length + token_length > max_chars {
            // Current segment is full, start a new one
            segments.push((current_tokens.clone(), current_spans.clone()));
            current_tokens.clear();
            current_spans.clear();
            current_length = 0;
        }

        current_tokens.push(token.clone());
        current_spans.push(span.clone());
        current_length += token_length;
    }

    // Add the last segment if it has content
    if !current_tokens.is_empty() {
        segments.push((current_tokens, current_spans));
    }

    // If no segments were created (shouldn't happen), return the original as one segment
    if segments.is_empty() {
        segments.push((tokens.to_vec(), spans.to_vec()));
    }

    segments
}

// Color tags use BBGGRR (no alpha) for \1c
fn bgr_from_aa_bgrr(aa_bgrr: &str) -> String {
    aa_bgrr.trim_start_matches("&H").chars().skip(2).collect() // drop AA
}

fn assemble_colored_two_lines(
    tokens: &[String], hi: usize,
    white_bgr: &str, hi_bgr: &str,
    line1_count: usize,
    header: &str,
    font_size: u32
) -> String {
    let white = format!("{{\\1c&H{}&\\fs{}}}", white_bgr, font_size);
    // Only create bigger font style if we're actually highlighting something
    let has_highlighting = hi != usize::MAX;
    let hi_style = if has_highlighting {
        let big_font_size = (font_size as f32 * BIG_FONT_SIZE_MULTIPLIER) as u32;
        format!("{{\\1c&H{}&\\fs{}}}", hi_bgr, big_font_size)
    } else {
        format!("{{\\1c&H{}&\\fs{}}}", hi_bgr, font_size) // Same size, just different color
    };

    let mut s = String::from(header); // will include \an2 \pos \q2 and stretch
    for i in 0..tokens.len() {
        if i == line1_count { s.push_str(r"\N"); }
        // Only highlight if hi is a valid index (not usize::MAX)
        let should_highlight = has_highlighting && i == hi;
        s.push_str(if should_highlight { &hi_style } else { &white });
        let t = tokens[i].replace('\\', r"\\").replace('{', r"\{").replace('}', r"\}");
        s.push_str(&t);
        if i + 1 < tokens.len() { s.push(' '); }
    }
    s
}

struct AssStyle {
    font_name: String,
    font_size: u32,
    primary: String,     // base (white)
    secondary: String,   // unused here
    outline: String,
    outline_w: u32,
    shadow: u32,
    align: u32,    // 1..9 grid; 2 = bottom-center
    margin_v: u32, // pixels
    highlight: String,   // green for current word
}

fn pct_to_margin_v(frame_h: u32, y_pct_from_top: f32) -> u32 {
    // bottom-aligned: margin_v measured from bottom
    let y = (frame_h as f32 * (y_pct_from_top / 100.0)).round() as i32;
    let margin_from_bottom = (frame_h as i32 - y).max(0) as u32;
    margin_from_bottom
}

fn stopwords() -> &'static HashSet<&'static str> {
    use std::sync::LazyLock;
    static SW: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
        [
            "a","an","the","to","of","in","on","at","by","for","with","and","or","but",
            "i","you","he","she","we","they","be","is","are","was","were","have","has","had",
            "do","does","did","will","would","can","could","should","shall","may","might","must",
            "gonna","wanna","like","just","really","very","actually","literally","kinda","sorta",
            "um","uh","you","know"
        ].into_iter().collect()
    });
    &SW
}

fn power_words() -> &'static HashSet<&'static str> {
    use std::sync::LazyLock;
    static PW: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
        [
            "not","no","never","without","dont","can't","cant","wont","why","must","need",
            "free","new","massive","insane","huge","proof","secret","banned"
        ].into_iter().collect()
    });
    &PW
}

fn build_global_tf(segments: &[CaptionSegment]) -> HashMap<String, u32> {
    let mut tf = HashMap::new();
    for s in segments {
        for w in &s.words {
            let t = w.text.trim();
            if t.is_empty() { continue; }
            *tf.entry(t.to_lowercase()).or_insert(0) += 1;
        }
    }
    tf
}

fn original_tokens(spans: &[WordSpan]) -> Vec<String> {
    spans.iter().map(|w| w.text.trim().to_string())
        .filter(|t| !t.is_empty()).collect()
}

fn has_digit_or_currency(s: &str) -> bool {
    s.chars().any(|c| c.is_ascii_digit() || c == '$' || c == '%' || c == '#')
}

fn looks_proper_noun(token: &str, idx_in_phrase: usize) -> bool {
    if idx_in_phrase == 0 { return false; }
    let chars: Vec<char> = token.chars().collect();
    if chars.is_empty() { return false; }
    let first = chars[0];
    first.is_uppercase() && !token.chars().all(|c| c.is_uppercase())
}

fn ends_with_content_suffix(token: &str) -> bool {
    let l = token.to_lowercase();
    l.ends_with("ing") || l.ends_with("ed") || l.ends_with("ly")
}

fn mean_std(vals: &[f32]) -> (f32, f32) {
    if vals.is_empty() { return (0.0, 0.0); }
    let m = vals.iter().sum::<f32>() / vals.len() as f32;
    let v = vals.iter().map(|x| (x - m)*(x - m)).sum::<f32>() / vals.len() as f32;
    (m, v.sqrt())
}

struct HighlightState {
    tf: HashMap<String,u32>,
    recent: VecDeque<(String,u64)>,   // (token_lower, time_ms)
    last_hl_ms: Option<u64>,
    last_hl_phrase: Option<usize>,
    phrases_done: u32,
    phrases_hl: u32,
}

impl HighlightState {
    fn new(segments: &[CaptionSegment]) -> Self {
        Self {
            tf: build_global_tf(segments),
            recent: VecDeque::new(),
            last_hl_ms: None,
            last_hl_phrase: None,
            phrases_done: 0,
            phrases_hl: 0,
        }
    }

    fn push_recent_phrase(&mut self, tokens: &[String], end_ms: u64) {
        // drop old
        while let Some((_, t)) = self.recent.front().cloned() {
            if end_ms.saturating_sub(t) > HL_RECENT_WINDOW_MS { self.recent.pop_front(); }
            else { break; }
        }
        for t in tokens {
            self.recent.push_back((t.to_lowercase(), end_ms));
        }
    }

    fn recent_count(&self, token_lower: &str, now_ms: u64) -> u32 {
        self.recent.iter()
            .filter(|(w,t)| w == token_lower && now_ms.saturating_sub(*t) <= HL_RECENT_WINDOW_MS)
            .count() as u32
    }
}

fn choose_highlight_idx(
    tokens_orig: &[String],
    spans: &[WordSpan],
    phrase_idx: usize,
    st: &mut HighlightState
) -> Option<usize> {
    let sw = stopwords();
    let pw = power_words();

    // rarity controls
    let mut threshold = HL_BASE_T;
    let phrase_start = spans.first().map(|w| w.start_ms).unwrap_or(0);
    let phrase_end   = spans.last().map(|w| w.end_ms).unwrap_or(0);

    if let Some(last) = st.last_hl_ms {
        if phrase_start.saturating_sub(last) < HL_MIN_GAP_MS { threshold += 1.0; }
    }
    if st.last_hl_phrase.map(|p| p + 1 == phrase_idx).unwrap_or(false) {
        threshold += HL_HYSTERESIS; // avoid back-to-back
    }
    if st.phrases_done > 0 && (st.phrases_hl as f32) / (st.phrases_done as f32) >= HL_MAX_RATIO {
        threshold += 0.8; // too many already
    }

    // candidates
    let cand: Vec<usize> = (0..tokens_orig.len()).filter(|&i| {
        let t = tokens_orig[i].trim();
        if t.is_empty() { return false; }
        let low = t.to_lowercase();
        if sw.contains(low.as_str()) { return false; }
        t.len() >= 3 || has_digit_or_currency(t)
    }).collect();

    if cand.is_empty() {
        st.phrases_done += 1;
        st.push_recent_phrase(tokens_orig, phrase_end);
        return None;
    }

    // features needing per-phrase stats
    let lens: Vec<f32> = tokens_orig.iter().map(|t| t.len() as f32).collect();
    let mut lens_sorted = lens.clone(); lens_sorted.sort_by(|a,b| a.partial_cmp(b).unwrap());
    let med_len = lens_sorted[lens_sorted.len()/2];

    let durs: Vec<f32> = spans.iter().map(|w| (w.end_ms - w.start_ms) as f32).collect();
    let (mean_dur, std_dur) = mean_std(&durs);

    // score
    let mut best: Option<(usize,f32)> = None;
    for &i in &cand {
        let t = tokens_orig[i].trim();
        let low = t.to_lowercase();
        let mut s = 0.0;

        if has_digit_or_currency(t) { s += 3.0; }
        if st.tf.get(&low).copied().unwrap_or(0) <= 2 { s += 2.0; }
        if looks_proper_noun(t, i) { s += 1.5; }
        if pw.contains(low.as_str()) { s += 1.5; }
        if ends_with_content_suffix(t) { s += 1.0; }
        if (t.len() as f32) > med_len { s += 1.0; }

        if std_dur > 0.0 {
            let z = (durs[i] - mean_dur) / std_dur;
            s += 0.5 * z.max(0.0); // only reward longer-than-avg
        }

        // pause / phrase-final emphasis
        if i + 1 == spans.len() { s += 0.5; }
        else {
            let gap = spans[i+1].start_ms.saturating_sub(spans[i].end_ms);
            if gap >= 250 { s += 0.5; }
        }

        // penalties
        if st.recent_count(&low, phrase_end) > 3 { s -= 2.0; }
        if t.chars().all(|c| c.is_uppercase()) && !tokens_orig.iter().all(|w| w.chars().all(|c| c.is_uppercase())) {
            s -= 1.0;
        }

        // tie-breakers inline
        if s >= threshold {
            match best {
                None => best = Some((i,s)),
                Some((bi,bs)) => {
                    if (s > bs)
                        || (s == bs && i > bi)              // later in phrase
                        || (s == bs && durs[i] > durs[bi])   // longer held
                        || (s == bs && st.tf.get(&low).unwrap_or(&u32::MAX) < st.tf.get(&tokens_orig[bi].to_lowercase()).unwrap_or(&u32::MAX))
                    { best = Some((i,s)); }
                }
            }
        }
    }

    st.phrases_done += 1;
    st.push_recent_phrase(tokens_orig, phrase_end);

    if let Some((idx,_)) = best {
        st.phrases_hl += 1;
        st.last_hl_ms = Some(phrase_end);
        st.last_hl_phrase = Some(phrase_idx);
        Some(idx)
    } else {
        None
    }
}

fn build_ass_document(
    w: u32,
    h: u32,
    style: &AssStyle,
    segments: &[CaptionSegment],
    karaoke: bool,
    glow_effect: bool
) -> Result<String> {
    if segments.is_empty() {
        return Err(anyhow!("No caption segments"));
    }

    let header = format!(
r#"[Script Info]
ScriptType: v4.00+
PlayResX: {w}
PlayResY: {h}
ScaledBorderAndShadow: yes

[V4+ Styles]
Format: Name,Fontname,Fontsize,PrimaryColour,SecondaryColour,OutlineColour,BackColour,Bold,Italic,Underline,StrikeOut,ScaleX,ScaleY,Spacing,Angle,BorderStyle,Outline,Shadow,Alignment,MarginL,MarginR,MarginV,Encoding
Style: TikTok,{font},{size},{pri},{sec},{out},&H64000000,0,0,0,0,100,100,0,0,1,{ow},{sh},{al},60,60,{mv},1

[Events]
Format: Layer,Start,End,Style,Name,MarginL,MarginR,MarginV,Effect,Text
"#,
        w = w, h = h,
        font = style.font_name, size = style.font_size,
        pri = style.primary, sec = style.secondary,
        out = style.outline, ow = style.outline_w, sh = style.shadow,
        al = style.align, mv = style.margin_v
    );

    let mut lines = String::new();

    if karaoke {
        let phrases = coalesce_phrases(segments);
        let white_bgr = bgr_from_aa_bgrr(&style.primary);
        let hi_bgr    = bgr_from_aa_bgrr(&style.highlight);

        // Simple single-line karaoke: split phrases that are too wide, then process each segment
        for ph in phrases {
            let tokens_upper = normalize_tokens(&ph.spans);
            let segments = split_phrase_for_width(&tokens_upper, &ph.spans, w, style.font_size);

            // Calculate Y position based on alignment
            let y_pos = match style.align {
                5 => (h / 2) as i32, // Middle center
                _ => (h as i32 - style.margin_v as i32).max(0), // Bottom center
            };

            // Process each width-appropriate segment
            for (segment_tokens, segment_spans) in segments {
                let windows = contiguous_cs_windows(&segment_spans);

                for (i, (cs0, cs1)) in windows.iter().enumerate() {
                let dur_ms = (cs1 - cs0) * 10;
                let blur_value = if glow_effect { 6.0 } else { 2.0 };

                let header = format!(
                    "{{\\an{}\\q2\\pos({},{})\\bord{}\\blur{:.1}}}{}",
                    style.align, (w/2), y_pos,
                    style.outline_w,
                    blur_value,
                    stretch_tag_ms(dur_ms)
                );

                if glow_effect {
                    // Glow layer
                    let glow_header = format!(
                        "{{\\an{}\\q2\\pos({},{})\\1a&HFF\\bord{}\\3c&HFFFFFF&\\3a&H80\\blur{:.1}\\shad0}}{}",
                        style.align, (w/2), y_pos,
                        style.outline_w as f32 * 2.0,
                        6.0,
                        stretch_tag_ms(dur_ms)
                    );
                    let glow_text = assemble_colored_two_lines(&segment_tokens, i, &white_bgr, &hi_bgr, usize::MAX, &glow_header, style.font_size);
                    lines.push_str(&format!(
                        "Dialogue: 0,{},{},TikTok,,0,0,0,,{}\n",
                        cs_to_ass(*cs0), cs_to_ass(*cs1), glow_text
                    ));

                    // Main text layer
                    let main_header = format!(
                        "{{\\an{}\\q2\\pos({},{})\\bord{}\\blur0\\shad0}}{}",
                        style.align, (w/2), y_pos,
                        style.outline_w,
                        stretch_tag_ms(dur_ms)
                    );
                    let main_text = assemble_colored_two_lines(&segment_tokens, i, &white_bgr, &hi_bgr, usize::MAX, &main_header, style.font_size);
                    lines.push_str(&format!(
                        "Dialogue: 1,{},{},TikTok,,0,0,0,,{}\n",
                        cs_to_ass(*cs0), cs_to_ass(*cs1), main_text
                    ));
                } else {
                    // Single layer
                    let text = assemble_colored_two_lines(&segment_tokens, i, &white_bgr, &hi_bgr, usize::MAX, &header, style.font_size);
                    lines.push_str(&format!(
                        "Dialogue: 0,{},{},TikTok,,0,0,0,,{}\n",
                        cs_to_ass(*cs0), cs_to_ass(*cs1), text
                    ));
                }
            }
            }
        }
    } else {
        let white_bgr = bgr_from_aa_bgrr(&style.primary);
        let hi_bgr    = bgr_from_aa_bgrr(&style.highlight);
        let x = (w/2) as i32;
        // Calculate Y position based on alignment
        let y = match style.align {
            5 => (h / 2) as i32, // Middle center - use actual center of frame
            _ => (h as i32 - style.margin_v as i32).max(0), // Bottom center - use margin
        };

        let phrases = coalesce_phrases(segments);

        // NEW: state for smart highlighting
        let mut hl_state = HighlightState::new(segments);

        for (p_idx, phrase) in phrases.iter().enumerate() {
            let tokens_upper = normalize_tokens(&phrase.spans);

            // Split phrase into single-line segments, same as karaoke mode
            let segments = split_phrase_for_width(&tokens_upper, &phrase.spans, w, style.font_size);

            for (segment_tokens, segment_spans) in segments {
                let segment_tokens_orig = original_tokens(&segment_spans);

                let start = cs_to_ass(ms_to_cs(segment_spans.first().unwrap().start_ms));
                let end   = cs_to_ass(ms_to_cs(segment_spans.last().unwrap().end_ms));

                // Decide which single word (if any) to highlight in this segment
                let hi_opt = choose_highlight_idx(&segment_tokens_orig, &segment_spans, p_idx, &mut hl_state);
                let hi_idx = hi_opt.unwrap_or(usize::MAX); // usize::MAX => no highlight

                // Build a ONE-LINE body: only colors/sizes + entrance animation
                // (no \pos/\bord/\shad in here; those are added by the glow/stroke layers)
                let text_body = assemble_colored_two_lines(
                    &segment_tokens, hi_idx, &white_bgr, &hi_bgr,
                    usize::MAX,               // no line break
                    &bounce_tag(),            // entrance scale
                    style.font_size
                );

                // Your layered renderer (glow + black stroke + fill)
                let glow_w    = style.outline_w as f32 * 2.0;
                let glow_blur = 6.0;
                let stroke_w  = style.outline_w as f32;

                push_glow_and_stroke(
                    &mut lines, &start, &end, &text_body,
                    x, y,
                    stroke_w,
                    glow_effect,  // Use the parameter to control glow
                    glow_w, glow_blur, "&H80",  // ~50% white glow
                    style.align   // Pass the alignment from style
                );
            }
        }
    }

    Ok(header + &lines)
}

/// Calculate proportional font size that maintains consistent appearance across different aspect ratios
/// Uses 9:16 format (608x1080) as the reference size
/// Formula: font_size = reference_font_size * sqrt(current_area / reference_area)
/// This ensures captions appear the same relative size regardless of video dimensions
fn calculate_proportional_font_size(frame_w: u32, frame_h: u32) -> u32 {
    // Reference dimensions for 9:16 format at 1080p height
    let reference_width = 608.0; // 9:16 aspect ratio at 1080p height
    let reference_height = 1080.0;
    let reference_area = reference_width * reference_height;
    let reference_font_size = reference_height * 0.06; // 6% of height, same as original logic

    // Calculate current video area
    let current_area = (frame_w as f32) * (frame_h as f32);

    // Scale font size proportionally to area ratio (square root to maintain visual proportions)
    let area_ratio = current_area / reference_area;
    let font_size = reference_font_size * area_ratio.sqrt();

    // Ensure minimum font size for readability
    font_size.max(18.0) as u32
}

/// Create default ASS style for TikTok-style captions with proportional sizing
/// Uses 9:16 format as reference to maintain consistent caption size across all formats
/// Accepts optional color parameters - if None, uses defaults (white text, black outline, yellow highlight)
/// Position parameter controls vertical alignment: "bottom" (default) or "center"
fn default_ass_style(
    frame_w: u32,
    frame_h: u32,
    font_name: Option<&str>,
    text_color: Option<&str>,
    highlight_color: Option<&str>,
    outline_color: Option<&str>,
    _glow_effect: bool,
    position: Option<&str>
) -> AssStyle {
    // Convert hex colors to ASS format (AABBGGRR), use defaults if None
    let primary = text_color.map(hex_to_ass_color).unwrap_or_else(|| "&H00FFFFFF".into());
    let highlight = highlight_color.map(hex_to_ass_color).unwrap_or_else(|| "&H0000FFFE".into());
    let outline = outline_color.map(hex_to_ass_color).unwrap_or_else(|| "&H00000000".into());

    // Determine vertical position and alignment based on position parameter
    let (align, margin_v) = match position.unwrap_or("bottom") {
        "center" => (5, 0), // Alignment 5 = middle center, margin_v 0 for center
        _ => (2, pct_to_margin_v(frame_h, 88.0)), // Alignment 2 = bottom center (default)
    };

    AssStyle {
        font_name: font_name.unwrap_or("Montserrat Black").into(),
        font_size: calculate_proportional_font_size(frame_w, frame_h),
        primary: primary.clone(),
        secondary: primary,
        outline,
        outline_w: 4,
        shadow: 0,
        align,
        margin_v,
        highlight,
    }
}

/// Convert hex color string (e.g., "#ffffff") to ASS color format (e.g., "&H00FFFFFF")
fn hex_to_ass_color(hex: &str) -> String {
    let hex = hex.trim_start_matches('#');
    if hex.len() == 6 {
        // Convert RGB hex to BGR hex for ASS format
        let r = &hex[0..2];
        let g = &hex[2..4];
        let b = &hex[4..6];
        format!("&H00{}{}{}", b, g, r) // ASS uses AABBGGRR format
    } else {
        "&H00FFFFFF".into() // Default to white if invalid hex
    }
}
