use crate::rpc::RpcEvent;
use crate::types::{
    BurnCaptionsParams, CaptionSegment, CaptionedVideoResult, ExtractAudioParams,
    GenerateCaptionsParams, GenerateCaptionsResult, LoadCaptionsParams, LoadCaptionsResult,
    PositionOverride, SaveCaptionsParams, TranscribeSegmentsParams, TranscribeSegmentsResult,
    WordSpan,
};
use crate::video::probe;
use crate::{audio, whisper};
use anyhow::{anyhow, Result};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use tokio::io::AsyncBufReadExt;
use tokio::process::Command as TokioCommand;
use tokio::sync::mpsc;

#[derive(Debug)]
enum InternalUpdate {
    Progress { index: usize, value: f32 },
    Event(RpcEvent),
}

/// Best-effort removal of a per-job temp directory. Temp dirs are only
/// needed while a job runs; if we never clean them up, /tmp fills up with
/// extracted audio and JSON files after every transcription/export.
fn cleanup_temp_dir(dir: &std::path::Path) {
    if let Err(e) = fs::remove_dir_all(dir) {
        crate::debug_log!("DEBUG: failed to clean temp dir {:?}: {}", dir, e);
    }
}

/// If `path` already exists, find the next free "name (2).ext", "name
/// (3).ext", ... instead — so re-rendering the same video and format never
/// silently clobbers a previous export the way the plain deterministic
/// `{input}_{format}.mp4` name otherwise would.
fn unique_output_path(path: &str) -> String {
    let p = std::path::Path::new(path);
    if !p.exists() {
        return path.to_string();
    }

    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("output");
    let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("mp4");
    let parent = p.parent().unwrap_or_else(|| std::path::Path::new(""));

    for n in 2..10_000 {
        let candidate = parent.join(format!("{stem} ({n}).{ext}"));
        if !candidate.exists() {
            return candidate.to_string_lossy().to_string();
        }
    }
    // Astronomically unlikely, but never loop forever over a directory
    // someone has actually filled with 9999 numbered exports.
    path.to_string()
}

#[allow(clippy::too_many_arguments)]
pub async fn extract_and_transcribe(
    id: &str,
    input_video: &str,
    split_by_words: bool,
    model: Option<String>,
    language: Option<String>,
    api_key: Option<String>,
    prompt: Option<String>,
    mut emit: impl FnMut(RpcEvent),
) -> Result<(crate::video::ProbeResult, String, TranscribeSegmentsResult)> {
    extract_and_transcribe_with_server(
        id,
        input_video,
        split_by_words,
        model,
        language,
        api_key,
        prompt,
        None,
        None,
        &mut emit,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn extract_and_transcribe_with_server(
    id: &str,
    input_video: &str,
    split_by_words: bool,
    model: Option<String>,
    language: Option<String>,
    api_key: Option<String>,
    prompt: Option<String>,
    whisper_base_url: Option<String>,
    audio_range: Option<crate::types::AudioRange>,
    mut emit: impl FnMut(RpcEvent),
) -> Result<(crate::video::ProbeResult, String, TranscribeSegmentsResult)> {
    let temp_dir = std::env::temp_dir().join(format!("capslap_captions_{}", id));
    if let Err(e) = fs::create_dir_all(&temp_dir) {
        return Err(anyhow!("Failed to create temp directory: {}", e));
    }

    let result = async {
        let probe_result = probe(id, input_video, &mut emit).await?;

        let audio_filename = format!("audio_{}.wav", id);
        let temp_audio_path = temp_dir.join(&audio_filename);
        let audio_params = ExtractAudioParams {
            input: input_video.to_string(),
            codec: Some("pcm_s16le".to_string()),
            out: Some(temp_audio_path.to_string_lossy().to_string()),
            range: audio_range,
        };
        let audio_result = audio::extract_audio(id, audio_params, &mut emit).await?;

        let transcribe_params = TranscribeSegmentsParams {
            audio: audio_result.audio.clone(),
            model,
            language,
            split_by_words,
            api_key,
            prompt,
            video_file: Some(input_video.to_string()),
            whisper_base_url,
        };
        let transcription = whisper::transcribe_segments_with_temp(
            id,
            transcribe_params,
            Some(&temp_dir),
            &mut emit,
        )
        .await?;

        Ok::<_, anyhow::Error>((probe_result, audio_result.audio, transcription))
    }
    .await;

    cleanup_temp_dir(&temp_dir);
    result
}

pub async fn burn_captions_with_segments(
    id: &str,
    params: BurnCaptionsParams,
    mut emit: impl FnMut(RpcEvent),
) -> Result<Vec<CaptionedVideoResult>> {
    let temp_dir = std::env::temp_dir().join(format!("capslap_captions_{}", id));
    if let Err(e) = fs::create_dir_all(&temp_dir) {
        return Err(anyhow!("Failed to create temp directory: {}", e));
    }

    let result = async {
        // We need to re-probe to get video dimensions
        let probe_result = probe(id, &params.input_video, &mut emit).await?;

        let duration_ms = probe_result
            .duration
            .map(|duration| (duration * 1000.0) as u64)
            .ok_or_else(|| anyhow!("Could not determine video duration for trimming"))?;
        let trim_start_ms = params.trim_start_ms.min(duration_ms);
        let trim_end_ms = params
            .trim_end_ms
            .min(duration_ms.saturating_sub(trim_start_ms));
        let trim_end_at_ms = duration_ms.saturating_sub(trim_end_ms);
        if trim_start_ms >= trim_end_at_ms {
            return Err(anyhow!("Trim settings must leave at least some video"));
        }
        let trimmed_segments: Vec<CaptionSegment> = params
            .segments
            .iter()
            .filter_map(|segment| {
                let start_ms = segment.start_ms.max(trim_start_ms);
                let end_ms = segment.end_ms.min(trim_end_at_ms);
                (end_ms > start_ms).then(|| CaptionSegment {
                    start_ms: start_ms - trim_start_ms,
                    end_ms: end_ms - trim_start_ms,
                    text: segment.text.clone(),
                    words: segment
                        .words
                        .iter()
                        .filter_map(|word| {
                            let start_ms = word.start_ms.max(trim_start_ms);
                            let end_ms = word.end_ms.min(trim_end_at_ms);
                            (end_ms > start_ms).then(|| crate::types::WordSpan {
                                start_ms: start_ms - trim_start_ms,
                                end_ms: end_ms - trim_start_ms,
                                text: word.text.clone(),
                                glue_to_previous: word.glue_to_previous,
                            })
                        })
                        .collect(),
                })
            })
            .collect();
        let trimmed_overrides: Vec<PositionOverride> = params
            .position_overrides
            .iter()
            .filter_map(|item| {
                let start_ms = item.start_ms.max(trim_start_ms);
                let end_ms = item.end_ms.min(trim_end_at_ms);
                (end_ms > start_ms).then(|| PositionOverride {
                    start_ms: start_ms - trim_start_ms,
                    end_ms: end_ms - trim_start_ms,
                    y_pct: item.y_pct,
                })
            })
            .collect();

        let bumpers = Bumpers {
            intro: probe_bumper(id, params.intro_video.as_deref(), "Intro", &mut emit).await?,
            outro: probe_bumper(id, params.outro_video.as_deref(), "Outro", &mut emit).await?,
            bug: check_watermark(params.watermark.clone())?,
        };

        optimized_multi_format_encode(
            id,
            &params.input_video,
            &trimmed_segments,
            &params.export_formats,
            &probe_result,
            &temp_dir,
            params.font_name,
            params.font_size,
            params.text_color,
            params.highlight_word_color,
            params.outline_color,
            params.outline_width,
            params.background_box,
            params.glow_effect,
            params.karaoke,
            params.multiline,
            params.justify_lines,
            params.position,
            params.output_size,
            params.crop_strategy,
            &trimmed_overrides,
            &params.blocked_bands,
            trim_start_ms as f64 / 1000.0,
            (trim_end_at_ms - trim_start_ms) as f64 / 1000.0,
            params.proof_short_side.filter(|&side| side > 0),
            &bumpers,
            &mut emit,
        )
        .await
    }
    .await;

    cleanup_temp_dir(&temp_dir);
    result
}

pub async fn generate_captions(
    id: &str,
    params: GenerateCaptionsParams,
    emit: impl FnMut(RpcEvent),
) -> Result<GenerateCaptionsResult> {
    generate_captions_single_pass(id, params, emit).await
}

pub async fn generate_captions_single_pass(
    id: &str,
    params: GenerateCaptionsParams,
    mut emit: impl FnMut(RpcEvent),
) -> Result<GenerateCaptionsResult> {
    let temp_dir = std::env::temp_dir().join(format!("capslap_captions_{}", id));

    let result = async {
        let (probe_result, audio_file, transcription) = extract_and_transcribe_with_server(
            id,
            &params.input_video,
            params.split_by_words,
            params.model,
            params.language,
            params.api_key,
            params.prompt,
            params.whisper_base_url,
            // The one-pass burn captions the whole file; the segments' times
            // would be off by the start of any part.
            None,
            &mut emit,
        )
        .await?;

        let captioned_videos = optimized_multi_format_encode(
            id,
            &params.input_video,
            &transcription.segments,
            &params.export_formats,
            &probe_result,
            &temp_dir,
            params.font_name,
            params.font_size,
            params.text_color,
            params.highlight_word_color,
            params.outline_color,
            params.outline_width,
            params.background_box,
            params.glow_effect,
            params.karaoke,
            params.multiline,
            params.justify_lines,
            params.position,
            params.output_size,
            params.crop_strategy,
            &params.position_overrides,
            &params.blocked_bands,
            0.0,
            probe_result.duration.unwrap_or(0.0),
            None, // the one-shot generate path makes no proof copy
            &Bumpers::default(),
            &mut emit,
        )
        .await?;

        Ok::<_, anyhow::Error>(GenerateCaptionsResult {
            probe_result,
            audio_file,
            transcription,
            captioned_videos,
        })
    }
    .await;

    cleanup_temp_dir(&temp_dir);
    result
}

/// Turn one cue's tokens into the lines the editor should draw, carrying the
/// same per-line font size the burner writes into the ASS document.
fn preview_lines(
    tokens: &[String],
    hi_idx: usize,
    justify_lines: bool,
    font_family: &str,
    frame_w: u32,
    base_px: u32,
) -> Vec<crate::types::PreviewLine> {
    let layout = cue_lines(tokens, justify_lines, font_family, frame_w, base_px);
    layout
        .iter()
        .map(|line| crate::types::PreviewLine {
            words: (line.start..line.end)
                .map(|i| crate::types::PreviewWord {
                    text: tokens[i].clone(),
                    is_highlighted: i == hi_idx,
                })
                .collect(),
            font_size_px: line.font_px,
        })
        .collect()
}

pub fn generate_preview_layout(
    params: crate::types::PreviewLayoutParams,
) -> Result<crate::types::PreviewLayoutResult> {
    // Lay the cues out on the canvas they will be burned onto. An export
    // format pads the source into a canvas of its own, and everything that
    // follows — font size, where lines break, how far up the frame the block
    // sits — is measured against that canvas, not the source frame.
    let (frame_w, frame_h) = match params.export_format.as_deref() {
        None | Some("") | Some("source") => (params.width, params.height),
        Some(format) => {
            let ar = crate::video::parse_target_ar(format)?;
            crate::video::target_dimensions(None, params.width, params.height, ar)
        }
    };

    let style = default_ass_style(
        frame_w,
        frame_h,
        params.font_name.as_deref(),
        params.text_color.as_deref(),
        params.highlight_word_color.as_deref(),
        params.outline_color.as_deref(),
        params.outline_width,
        params.background_box,
        params.glow_effect,
        params.position.as_deref(),
        params.font_size,
    );

    let mut cues = Vec::new();

    // Same anchor the burner uses, expressed as a percentage of frame height so
    // the editor can place an overlay without knowing about ASS.
    let default_y = style_anchor_y(style.align, style.margin_v, frame_h);
    let anchor = match style.align {
        5 => "center",
        8 => "top",
        _ => "bottom",
    };
    let to_pct = |y: i32| (y as f32 / frame_h as f32) * 100.0;

    if params.karaoke {
        let phrases = segment_phrases(&params.segments);

        for ph in phrases {
            let tokens_upper = normalize_tokens(&ph.spans);
            // One cue per authored segment, exactly like build_ass_document.
            let segments = vec![(tokens_upper, ph.spans.clone())];

            for (segment_tokens, segment_spans) in segments {
                let group_start_ms = segment_spans.first().map(|s| s.start_ms).unwrap_or(0);
                let group_end_ms = segment_spans.last().map(|s| s.end_ms).unwrap_or(0);
                let y_pct = to_pct(cue_anchor_y(
                    &params.position_overrides,
                    &params.blocked_bands,
                    group_start_ms,
                    group_end_ms,
                    default_y,
                    frame_h,
                    style.align,
                    cue_lines(
                        &segment_tokens,
                        params.justify_lines,
                        &style.font_name,
                        frame_w,
                        style.font_size,
                    )
                    .len(),
                    style.font_size,
                ));

                let windows = contiguous_cs_windows(&segment_spans);

                for (i, (cs0, cs1)) in windows.iter().enumerate() {
                    let start_ms = (*cs0 as u64) * 10;
                    let end_ms = (*cs1 as u64) * 10;

                    // In karaoke each window highlights one word (index i);
                    // segment_tokens and segment_spans are 1:1.
                    cues.push(crate::types::PreviewCue {
                        start_ms,
                        end_ms,
                        lines: preview_lines(
                            &segment_tokens,
                            i,
                            params.justify_lines,
                            &style.font_name,
                            frame_w,
                            style.font_size,
                        ),
                        y_pct,
                        group_start_ms,
                        group_end_ms,
                        anchor: anchor.to_string(),
                    });
                }
            }
        }
    } else {
        let phrases = segment_phrases(&params.segments);
        let mut hl_state = HighlightState::new(&params.segments);

        for (p_idx, phrase) in phrases.iter().enumerate() {
            let tokens_upper = normalize_tokens(&phrase.spans);
            let segments = vec![(tokens_upper, phrase.spans.clone())];

            for (segment_tokens, segment_spans) in segments {
                let segment_tokens_orig = original_tokens(&segment_spans);
                let start_ms = segment_spans.first().unwrap().start_ms;
                let end_ms = segment_spans.last().unwrap().end_ms;
                let hi_opt = choose_highlight_idx(
                    &segment_tokens_orig,
                    &segment_spans,
                    p_idx,
                    &mut hl_state,
                );
                let hi_idx = hi_opt.unwrap_or(usize::MAX);

                // Without justification the burner emits the cue as one
                // flowing run and lets libass wrap it, so the layout is one
                // line here too and the client wraps it with the same margins.
                let lines_structure = preview_lines(
                    &segment_tokens,
                    hi_idx,
                    params.justify_lines,
                    &style.font_name,
                    frame_w,
                    style.font_size,
                );

                let y_pct = to_pct(cue_anchor_y(
                    &params.position_overrides,
                    &params.blocked_bands,
                    start_ms,
                    end_ms,
                    default_y,
                    frame_h,
                    style.align,
                    lines_structure.len(),
                    style.font_size,
                ));

                cues.push(crate::types::PreviewCue {
                    start_ms,
                    end_ms,
                    lines: lines_structure,
                    y_pct,
                    // Non-karaoke cues are already one block each.
                    group_start_ms: start_ms,
                    group_end_ms: end_ms,
                    anchor: anchor.to_string(),
                });
            }
        }
    }

    Ok(crate::types::PreviewLayoutResult {
        cues,
        font_size_px: style.font_size,
        frame_width: frame_w,
        frame_height: frame_h,
    })
}

pub fn save_captions(params: SaveCaptionsParams) -> Result<()> {
    // Sidecar captions file lives next to the video: video.mp4 -> video.mp4.capslap.json
    // (appended, not extension-replaced, so video.mp4 and video.avi never clash)
    let json_path = format!("{}.capslap.json", params.video_path);

    let file = crate::types::CaptionsFile {
        segments: params.segments,
        position_overrides: params.position_overrides,
    };
    let json = serde_json::to_string_pretty(&file)?;
    fs::write(&json_path, json)?;

    Ok(())
}

pub fn load_captions(params: LoadCaptionsParams) -> Result<LoadCaptionsResult> {
    let json_path = format!("{}.capslap.json", params.video_path);
    let path = std::path::Path::new(&json_path);

    if path.exists() {
        let content = fs::read_to_string(path)?;
        // Current format is an object; sidecars written by older builds are a
        // bare array of segments.
        match serde_json::from_str::<crate::types::CaptionsFile>(&content) {
            Ok(file) => Ok(LoadCaptionsResult {
                segments: Some(file.segments),
                position_overrides: file.position_overrides,
            }),
            Err(_) => {
                let segments: Vec<CaptionSegment> = serde_json::from_str(&content)?;
                Ok(LoadCaptionsResult {
                    segments: Some(segments),
                    position_overrides: Vec::new(),
                })
            }
        }
    } else {
        Ok(LoadCaptionsResult {
            segments: None,
            position_overrides: Vec::new(),
        })
    }
}

/// Render the captions alone as an RGBA PNG the editor can drag over a
/// caption-free frame.
///
/// FFmpeg gives us the same subtitles drawn on black and on white, stacked
/// vertically. For a pixel where the caption covers a fraction `a` of the area,
/// the two passes are `k = c*a` and `w = c*a + 255*(1-a)`, so the difference
/// recovers the coverage and dividing it out of `k` recovers the colour.
async fn render_caption_layer(
    ffmpeg_path: &str,
    frame_w: u32,
    frame_h: u32,
    ass_path: &str,
    time_sec: f64,
    scale_height: Option<u32>,
) -> Result<Vec<u8>> {
    let filter = crate::video::build_caption_layer_filter(ass_path, time_sec, scale_height);

    let output = TokioCommand::new(ffmpeg_path)
        .arg("-threads")
        .arg("2")
        .arg("-f")
        .arg("lavfi")
        .arg("-i")
        .arg(format!("color=c=black:s={}x{}:d=1", frame_w, frame_h))
        .arg("-f")
        .arg("lavfi")
        .arg("-i")
        .arg(format!("color=c=white:s={}x{}:d=1", frame_w, frame_h))
        .arg("-filter_complex")
        .arg(&filter)
        .arg("-map")
        .arg("[out]")
        .arg("-frames:v")
        .arg("1")
        .arg("-f")
        .arg("rawvideo")
        .arg("-pix_fmt")
        .arg("rgb24")
        .arg("-")
        .output()
        .await
        .map_err(|e| anyhow!("Failed to run ffmpeg: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("FFmpeg caption layer render failed: {}", stderr));
    }

    let raw = &output.stdout;
    let h = scale_height.unwrap_or(frame_h);
    let stacked_h = h * 2;
    if stacked_h == 0 {
        return Err(anyhow!("Caption layer render returned an empty image"));
    }
    let h_bytes = stacked_h as usize * 3;
    if h_bytes == 0 || raw.len() % h_bytes != 0 {
        return Err(anyhow!("Unexpected caption layer image buffer size"));
    }
    let w = (raw.len() / h_bytes) as u32;

    let total_pixels = (w * h) as usize;
    if raw.len() < total_pixels * 6 {
        return Err(anyhow!("Unexpected caption layer image buffer size"));
    }

    let (black_slice, white_slice) = raw.split_at(total_pixels * 3);
    let mut out_buffer = vec![0u8; total_pixels * 4];

    for (out_px, (b_px, w_px)) in out_buffer.as_chunks_mut::<4>().0.iter_mut().zip(
        black_slice
            .as_chunks::<3>()
            .0
            .iter()
            .zip(white_slice.as_chunks::<3>().0.iter()),
    ) {
        let diff0 = w_px[0].saturating_sub(b_px[0]);
        let diff1 = w_px[1].saturating_sub(b_px[1]);
        let diff2 = w_px[2].saturating_sub(b_px[2]);
        let showing_through = diff0.max(diff1).max(diff2);
        let alpha = 255 - showing_through;

        if alpha > 0 {
            let inv_a = 255.0 / alpha as f32;
            out_px[0] = ((b_px[0] as f32 * inv_a).min(255.0)) as u8;
            out_px[1] = ((b_px[1] as f32 * inv_a).min(255.0)) as u8;
            out_px[2] = ((b_px[2] as f32 * inv_a).min(255.0)) as u8;
            out_px[3] = alpha;
        }
    }

    let layer = image::RgbaImage::from_raw(w, h, out_buffer)
        .ok_or_else(|| anyhow!("Failed to create RGBA caption layer buffer"))?;

    let mut png = Vec::new();
    layer
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| anyhow!("Failed to encode caption layer: {}", e))?;

    Ok(png)
}

pub async fn generate_preview_frame(
    params: crate::types::PreviewFrameParams,
) -> Result<crate::types::PreviewFrameResult> {
    let temp_dir = std::env::temp_dir().join(format!("capslap_preview_{}", uuid::Uuid::new_v4()));
    if let Err(e) = fs::create_dir_all(&temp_dir) {
        return Err(anyhow!("Failed to create temp directory: {}", e));
    }

    let result = async {
        // We need to probe to get video dimensions
        // We don't have an ID for logs here, so we use a placeholder
        let probe_id = "preview_probe";
        let probe_result = probe(probe_id, &params.input_video, |e| {
            // Ignore logs for preview
            let _ = e;
        })
        .await?;

        // Determine target dimensions
        let src_w = probe_result.width.unwrap_or(1920) as u32;
        let src_h = probe_result.height.unwrap_or(1080) as u32;

        // "source" lays the captions out on the video's own frame, which is
        // what the editor shows. Every other format is an export canvas the
        // video gets fitted into, and the captions follow that canvas.
        let (target_w, target_h) = if params.export_format == "source" {
            (src_w, src_h)
        } else {
            let target_ar = crate::video::parse_target_ar(&params.export_format)?;
            crate::video::target_dimensions(params.output_size.as_deref(), src_w, src_h, target_ar)
        };

        // Calculate crop strategy
        let crop_strategy = params.crop_strategy.as_deref().unwrap_or("fit");

        let render_mode = params.render_mode.as_deref().unwrap_or("video");
        let wants_captions = render_mode != "clean";

        // The caption layer is drawn on transparent black, so it must stay PNG.
        // Opaque frames (full previews & thumbnails) are photos — fast JPEG keeps latency and data URIs small.
        let transparent = render_mode == "captions";
        let as_jpeg = !transparent;

        let ass_str = if wants_captions {
            let style = default_ass_style(
                target_w,
                target_h,
                params.font_name.as_deref(),
                params.text_color.as_deref(),
                params.highlight_word_color.as_deref(),
                params.outline_color.as_deref(),
                params.outline_width,
                params.background_box,
                params.glow_effect,
                params.position.as_deref(),
                params.font_size,
            );

            let ass_doc = build_ass_document(
                target_w,
                target_h,
                &style,
                &params.segments,
                params.karaoke,
                params.multiline,
                params.justify_lines,
                params.glow_effect,
                &params.position_overrides,
                &params.blocked_bands,
            )?;

            let ass_path = temp_dir.join("preview.ass");
            fs::write(&ass_path, &ass_doc)?;
            Some(ass_path.to_string_lossy().to_string())
        } else {
            None
        };

        let ffmpeg_path = crate::video::get_ffmpeg_path_sync();
        let time_sec = params.timestamp_ms as f64 / 1000.0;

        let image_bytes = if transparent {
            render_caption_layer(
                &ffmpeg_path,
                target_w,
                target_h,
                ass_str.as_deref().unwrap(),
                time_sec,
                params.thumbnail_height,
            )
            .await?
        } else {
            // Construct filter graph
            let is_hdr = crate::video::is_hdr(&probe_result);
            let mut vf = crate::video::build_fitpad_filter_with_options(
                target_w,
                target_h,
                ass_str.as_deref(),
                crate::video::HardwareEncoder::Software, // Use software mode for compatibility
                crop_strategy,
                is_hdr,
            );

            if let Some(thumb_h) = params.thumbnail_height {
                vf.push_str(&format!(",scale=-2:{}:flags=bilinear", thumb_h.max(2)));
            }

            // Restore the seeked-to moment before libass looks for a cue.
            if ass_str.is_some() {
                vf = format!("{},{}", crate::video::seek_pts_filter(time_sec), vf);
            }

            let output = TokioCommand::new(&ffmpeg_path)
                .arg("-ss")
                .arg(time_sec.to_string())
                .arg("-i")
                .arg(&params.input_video)
                .arg("-threads")
                .arg("2")
                .arg("-vf")
                .arg(&vf)
                .arg("-frames:v")
                .arg("1")
                .arg("-f")
                .arg("image2")
                .arg("-c:v")
                .arg(if as_jpeg { "mjpeg" } else { "png" })
                .args(if as_jpeg {
                    if params.thumbnail_height.is_some() {
                        vec!["-q:v", "4"]
                    } else {
                        vec!["-q:v", "2"]
                    }
                } else {
                    vec![]
                })
                .arg("-") // Output to stdout
                .output()
                .await
                .map_err(|e| anyhow!("Failed to run ffmpeg: {}", e))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(anyhow!("FFmpeg preview failed: {}", stderr));
            }

            output.stdout
        };

        use base64::{engine::general_purpose, Engine as _};
        let encoded = general_purpose::STANDARD.encode(&image_bytes);
        let mime = if as_jpeg { "jpeg" } else { "png" };
        let data_uri = format!("data:image/{};base64,{}", mime, encoded);

        Ok::<_, anyhow::Error>(crate::types::PreviewFrameResult {
            image_data: data_uri,
        })
    }
    .await;

    // Always clean up the temp dir, including on error
    cleanup_temp_dir(&temp_dir);
    result
}

#[cfg(test)]
mod tests_persistence {
    use super::*;
    use std::fs;
    use tempfile::NamedTempFile;

    #[test]
    fn test_save_and_load_captions() -> Result<()> {
        let video_file = NamedTempFile::new()?;
        let video_path = video_file.path().to_string_lossy().to_string();

        let segments = vec![
            CaptionSegment {
                start_ms: 1000,
                end_ms: 2000,
                text: "Hello world".to_string(),
                words: vec![],
            },
            CaptionSegment {
                start_ms: 2500,
                end_ms: 3500,
                text: "Testing save load".to_string(),
                words: vec![],
            },
        ];

        // Save
        save_captions(SaveCaptionsParams {
            video_path: video_path.clone(),
            segments: segments.clone(),
            position_overrides: vec![PositionOverride {
                start_ms: 1000,
                end_ms: 2000,
                y_pct: 35.0,
            }],
        })?;

        // Check file exists
        let json_path = format!("{}.capslap.json", video_path);
        assert!(std::path::Path::new(&json_path).exists());

        // Load
        let loaded = load_captions(LoadCaptionsParams {
            video_path: video_path.clone(),
        })?;

        assert!(loaded.segments.is_some());
        let loaded_segments = loaded.segments.unwrap();
        assert_eq!(loaded_segments.len(), 2);
        assert_eq!(loaded_segments[0].text, "Hello world");
        assert_eq!(loaded_segments[1].end_ms, 3500);
        assert_eq!(loaded.position_overrides.len(), 1);
        assert_eq!(loaded.position_overrides[0].y_pct, 35.0);

        // Cleanup
        let _ = fs::remove_file(json_path);

        Ok(())
    }

    #[test]
    fn test_load_legacy_bare_array_sidecar() -> Result<()> {
        // Sidecars written before position overrides existed are a bare array.
        let video_file = NamedTempFile::new()?;
        let video_path = video_file.path().to_string_lossy().to_string();
        let json_path = format!("{}.capslap.json", video_path);

        fs::write(
            &json_path,
            r#"[{"startMs":0,"endMs":500,"text":"Legacy","words":[]}]"#,
        )?;

        let loaded = load_captions(LoadCaptionsParams {
            video_path: video_path.clone(),
        })?;

        let loaded_segments = loaded.segments.expect("legacy sidecar should load");
        assert_eq!(loaded_segments.len(), 1);
        assert_eq!(loaded_segments[0].text, "Legacy");
        assert!(loaded.position_overrides.is_empty());

        let _ = fs::remove_file(json_path);

        Ok(())
    }

    #[test]
    fn a_word_split_across_a_segment_boundary_is_put_back_together() {
        // Exactly what whisper does with long Finnish words: it emits "korke"
        // and "alla" as separate pieces and breaks the segment between them.
        let segments = vec![
            CaptionSegment {
                start_ms: 0,
                end_ms: 1000,
                text: "tarpeeksi korke".to_string(),
                words: vec![
                    WordSpan {
                        start_ms: 0,
                        end_ms: 500,
                        text: "tarpeeksi".to_string(),
                        glue_to_previous: false,
                    },
                    WordSpan {
                        start_ms: 500,
                        end_ms: 1000,
                        text: "korke".to_string(),
                        glue_to_previous: false,
                    },
                ],
            },
            CaptionSegment {
                start_ms: 1000,
                end_ms: 2000,
                text: "alla birtsille.".to_string(),
                words: vec![
                    WordSpan {
                        start_ms: 1000,
                        end_ms: 1400,
                        text: "alla".to_string(),
                        glue_to_previous: true,
                    },
                    WordSpan {
                        start_ms: 1400,
                        end_ms: 2000,
                        text: "birtsille.".to_string(),
                        glue_to_previous: false,
                    },
                ],
            },
        ];

        let phrases = segment_phrases(&segments);
        let words: Vec<&str> = phrases
            .iter()
            .flat_map(|p| p.spans.iter())
            .map(|s| s.text.as_str())
            .collect();

        assert_eq!(words, vec!["tarpeeksi", "korkealla", "birtsille."]);

        // The rejoined word has to cover both halves, or karaoke would stop
        // highlighting it halfway through.
        let joined = phrases
            .iter()
            .flat_map(|p| p.spans.iter())
            .find(|s| s.text == "korkealla")
            .expect("the halves should have been joined");
        assert_eq!(joined.start_ms, 500);
        assert_eq!(joined.end_ms, 1400);
    }

    #[test]
    fn test_segment_phrases_honors_edited_segment_text() {
        // When s.text is edited and differs from s.words, coalesce_phrases must use s.text
        let segments = vec![CaptionSegment {
            start_ms: 0,
            end_ms: 2000,
            text: "Uusi muokattu teksti".to_string(), // edited by user
            words: vec![
                WordSpan {
                    start_ms: 0,
                    end_ms: 1000,
                    text: "Vanha".to_string(),
                    glue_to_previous: false,
                },
                WordSpan {
                    start_ms: 1000,
                    end_ms: 2000,
                    text: "teksti".to_string(),
                    glue_to_previous: false,
                },
            ],
        }];

        let phrases = segment_phrases(&segments);
        let words: Vec<&str> = phrases
            .iter()
            .flat_map(|p| p.spans.iter())
            .map(|s| s.text.as_str())
            .collect();

        assert_eq!(words, vec!["Uusi", "muokattu", "teksti"]);
    }

    #[test]
    fn each_authored_segment_stays_its_own_cue() {
        // Sentence-ending punctuation, sub-second gaps and short cues used to
        // be re-chunked here, which discarded every split the user had made in
        // the editor. One segment in, one cue out.
        let segments = vec![
            CaptionSegment {
                start_ms: 0,
                end_ms: 800,
                text: "Eka lause.".to_string(),
                words: vec![
                    WordSpan {
                        start_ms: 0,
                        end_ms: 400,
                        text: "Eka".to_string(),
                        glue_to_previous: false,
                    },
                    WordSpan {
                        start_ms: 400,
                        end_ms: 800,
                        text: "lause.".to_string(),
                        glue_to_previous: false,
                    },
                ],
            },
            CaptionSegment {
                start_ms: 800,
                end_ms: 1600,
                text: "Toka".to_string(),
                words: vec![WordSpan {
                    start_ms: 800,
                    end_ms: 1600,
                    text: "Toka".to_string(),
                    glue_to_previous: false,
                }],
            },
            CaptionSegment {
                start_ms: 1600,
                end_ms: 2400,
                text: "kolmas".to_string(),
                words: vec![WordSpan {
                    start_ms: 1600,
                    end_ms: 2400,
                    text: "kolmas".to_string(),
                    glue_to_previous: false,
                }],
            },
        ];

        let phrases = segment_phrases(&segments);

        assert_eq!(phrases.len(), 3);
        assert_eq!(phrases[0].tokens, vec!["Eka", "lause."]);
        assert_eq!(phrases[1].tokens, vec!["Toka"]);
        assert_eq!(phrases[2].tokens, vec!["kolmas"]);
        assert_eq!(phrases[1].start_ms, 800);
        assert_eq!(phrases[1].end_ms, 1600);
    }

    #[test]
    fn test_resolve_anchor_y_uses_override_for_matching_cue() {
        let overrides = vec![PositionOverride {
            start_ms: 1000,
            end_ms: 2000,
            y_pct: 25.0,
        }];

        // Cue midpoint 1500ms falls inside the override.
        assert_eq!(resolve_anchor_y(&overrides, 1000, 2000, 900, 1000), 250);
        // Cue midpoint 2500ms does not, so the style default stands.
        assert_eq!(resolve_anchor_y(&overrides, 2000, 3000, 900, 1000), 900);
        // No overrides at all.
        assert_eq!(resolve_anchor_y(&[], 1000, 2000, 900, 1000), 900);
    }

    #[test]
    fn test_style_anchor_y_per_alignment() {
        // Bottom-aligned keeps its margin from the bottom edge...
        assert_eq!(style_anchor_y(2, 120, 1000), 880);
        // ...center ignores the margin...
        assert_eq!(style_anchor_y(5, 120, 1000), 500);
        // ...and top measures its margin from the top.
        assert_eq!(style_anchor_y(8, 120, 1000), 120);
    }
}

#[allow(clippy::too_many_arguments)]
async fn optimized_multi_format_encode(
    id: &str,
    input_video: &str,
    segments: &[CaptionSegment],
    export_formats: &[String],
    probe_result: &crate::video::ProbeResult,
    temp_dir: &std::path::Path,
    font_name: Option<String>,
    font_size: Option<u32>,
    text_color: Option<String>,
    highlight_word_color: Option<String>,
    outline_color: Option<String>,
    outline_width: Option<u32>,
    background_box: bool,
    glow_effect: bool,
    karaoke: bool,
    multiline: bool,
    justify_lines: bool,
    position: Option<String>,
    output_size: Option<String>,
    crop_strategy: Option<String>,
    position_overrides: &[PositionOverride],
    blocked_bands: &[(f32, f32)],
    trim_start_seconds: f64,
    output_duration_seconds: f64,
    proof_short_side: Option<u32>,
    bumpers: &Bumpers,
    emit: &mut impl FnMut(RpcEvent),
) -> Result<Vec<CaptionedVideoResult>> {
    // Fail fast if libass is not available (required for burning subtitles)
    if !crate::video::is_libass_available().await {
        return Err(anyhow!("The installed FFmpeg version does not support burning subtitles (missing 'ass' filter). Please install a version of FFmpeg with libass support (e.g. via homebrew: 'brew install ffmpeg')."));
    }

    if export_formats.is_empty() {
        return Err(anyhow!("No export formats specified"));
    }

    emit(RpcEvent::Log {
        id: id.into(),
        message: format!(
            "Export params: OutputSize={:?}, CropStrategy={:?}, Formats={:?}",
            output_size, crop_strategy, export_formats
        ),
    });

    let input_path = std::path::Path::new(input_video)
        .with_extension("")
        .to_string_lossy()
        .to_string();

    // Pre-generate shared ASS files for each format (avoiding redundant subtitle processing)
    let mut format_ass_files = Vec::new();
    for format in export_formats {
        emit(RpcEvent::Log {
            id: id.into(),
            message: format!("Processing format loop for: {}", format),
        });
        let src_w = probe_result.width.unwrap_or(1920) as u32;
        let src_h = probe_result.height.unwrap_or(1080) as u32;

        // Determine target dimensions based on output_size or aspect ratio.
        // "source" keeps the video's own frame, which is what the editor
        // previews when no reframing was asked for.
        let (target_w, target_h) = if format == "source" {
            (src_w, src_h)
        } else {
            let target_ar = crate::video::parse_target_ar(format)?;
            crate::video::target_dimensions(output_size.as_deref(), src_w, src_h, target_ar)
        };

        // Build ASS subtitle file optimized for this format
        emit(RpcEvent::Log {
            id: id.into(),
            message: format!("Building ASS style for format: {}", format),
        });
        let style = default_ass_style(
            target_w,
            target_h,
            font_name.as_deref(),
            text_color.as_deref(),
            highlight_word_color.as_deref(),
            outline_color.as_deref(),
            outline_width,
            background_box,
            glow_effect,
            position.as_deref(),
            font_size,
        );
        emit(RpcEvent::Log {
            id: id.into(),
            message: format!("Building ASS document for format: {}", format),
        });
        let ass_doc = build_ass_document(
            target_w,
            target_h,
            &style,
            segments,
            karaoke,
            multiline,
            justify_lines,
            glow_effect,
            position_overrides,
            blocked_bands,
        )?;
        emit(RpcEvent::Log {
            id: id.into(),
            message: format!("ASS document built for format: {}", format),
        });

        let safe_format = format.replace(':', "x");
        let ass_filename = format!("captions_{}_{}.ass", id, safe_format);
        let ass_path = temp_dir.join(&ass_filename);
        fs::write(&ass_path, ass_doc)?;
        emit(RpcEvent::Log {
            id: id.into(),
            message: format!("ASS file written to: {:?}", ass_path),
        });

        format_ass_files.push((format.clone(), ass_path, target_w, target_h));
    }

    // Process formats with limited concurrency (2 at a time for optimal resource usage)
    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(2));
    let mut tasks = tokio::task::JoinSet::new();
    let (tx, mut rx) = mpsc::unbounded_channel::<InternalUpdate>();

    // Track progress of each task
    let mut task_progress = HashMap::new();
    let total_tasks = format_ass_files.len();

    // Initial progress event
    emit(RpcEvent::Progress {
        id: id.to_string(),
        status: "Starting export...".to_string(),
        progress: 0.0,
    });

    for (idx, (format, ass_path, target_w, target_h)) in format_ass_files.into_iter().enumerate() {
        let format = format.clone();
        let input_video = input_video.to_string();
        let probe_result = probe_result.clone();
        let semaphore = semaphore.clone();
        let task_id = format!("{}_{}", id, idx);
        let input_path = input_path.clone();
        let crop_strat = crop_strategy.clone().unwrap_or_else(|| "fit".to_string());
        let tx = tx.clone();
        let bumpers = bumpers.clone();

        tasks.spawn(async move {
            // Acquire semaphore permit for bounded concurrency
            let _permit = semaphore.acquire().await.unwrap();

            let safe_format = format.replace(':', "x");
            let captioned_path = unique_output_path(&format!("{}_{}.mp4", input_path, safe_format));
            // Only when the video is bigger than the proof; a copy the same size is waste.
            let proof = proof_short_side
                .filter(|&side| side < target_w.min(target_h))
                .map(|side| {
                    (
                        side,
                        unique_output_path(&proof_output_path(&captioned_path, side)),
                    )
                });

            // Single-pass format conversion + caption burning with hardware acceleration
            optimized_single_format_encode(
                &task_id,
                &input_video,
                &ass_path,
                &captioned_path,
                target_w,
                target_h,
                &crop_strat,
                &probe_result,
                trim_start_seconds,
                output_duration_seconds,
                proof.as_ref().map(|(side, path)| (*side, path.as_str())),
                &bumpers,
                tx,
                idx,
            )
            .await?;

            Ok::<CaptionedVideoResult, anyhow::Error>(CaptionedVideoResult {
                format,
                raw_video: "".to_string(),
                captioned_video: captioned_path,
                width: target_w,
                height: target_h,
                proof_video: proof.map(|(_, path)| path),
            })
        });
    }

    // Drop original sender so receiver knows when all senders are done (after tasks finish)
    drop(tx);

    let mut captioned_videos = Vec::new();
    let mut active = true;

    // Collect results and handle progress
    while active || !tasks.is_empty() {
        tokio::select! {
            Some(res) = tasks.join_next() => {
                match res {
                    Ok(Ok(result)) => captioned_videos.push(result),
                    Ok(Err(e)) => return Err(e), // Task failed
                    Err(e) => return Err(anyhow!("Task join error: {}", e)),
                }
            }
            Some(update) = rx.recv() => {
                match update {
                    InternalUpdate::Progress { index, value } => {
                        task_progress.insert(index, value);

                        let sum: f32 = task_progress.values().sum();
                        let avg_progress = sum / total_tasks as f32;

                        emit(RpcEvent::Progress {
                            id: id.to_string(),
                            status: format!("Exporting... ({:.0}%)", avg_progress * 100.0),
                            progress: avg_progress,
                        });
                    },
                    InternalUpdate::Event(e) => emit(e),
                }
            }
            else => {
                // Channel closed and tasks empty
                active = false;
            }
        }
    }

    // Final 100% progress
    emit(RpcEvent::Progress {
        id: id.to_string(),
        status: "Export complete".to_string(),
        progress: 1.0,
    });

    // Sort results to match input order if needed, but for now just returning collected results
    Ok(captioned_videos)
}

/// Optimized single format encoding with hardware acceleration and modern FFmpeg flags
#[allow(clippy::too_many_arguments)]
async fn optimized_single_format_encode(
    id: &str,
    input_video: &str,
    ass_path: &std::path::Path,
    output_path: &str,
    target_w: u32,
    target_h: u32,
    crop_strategy: &str,
    probe_result: &crate::video::ProbeResult,
    trim_start_seconds: f64,
    output_duration_seconds: f64,
    proof: Option<(u32, &str)>,
    bumpers: &Bumpers,
    tx: mpsc::UnboundedSender<InternalUpdate>,
    index: usize,
) -> Result<()> {
    // Determine the best available hardware encoder for H.264 first (for filter optimization)
    let hardware_encoder = crate::video::get_best_hardware_encoder().await;

    // Best first: the whole GPU pipeline, then GPU decode only, then just the
    // hardware encoder, then software. Each one that fails hands over to the next.
    let attempts = crate::video::encode_attempts(
        hardware_encoder,
        crate::video::vt_pipeline_available(),
        crate::video::is_hdr(probe_result),
    );
    let last = attempts.len() - 1;
    let mut result = Err(anyhow!("no encoder attempt was made"));
    for (n, (encoder, accel)) in attempts.into_iter().enumerate() {
        result = try_encode_with_encoder(
            id,
            input_video,
            ass_path,
            output_path,
            target_w,
            target_h,
            crop_strategy,
            probe_result,
            trim_start_seconds,
            output_duration_seconds,
            proof,
            bumpers,
            encoder,
            accel,
            tx.clone(),
            index,
        )
        .await;
        if result.is_ok() {
            break;
        }
        if n < last {
            let _ = tx.send(InternalUpdate::Event(RpcEvent::Log {
                id: id.into(),
                message: format!("{encoder:?} with {accel:?} failed, trying the next way"),
            }));
        }
    }
    result
}

/// Helper function to try encoding with a specific encoder
#[allow(clippy::too_many_arguments)]
async fn try_encode_with_encoder(
    id: &str,
    input_video: &str,
    ass_path: &std::path::Path,
    output_path: &str,
    target_w: u32,
    target_h: u32,
    crop_strategy: &str,
    probe_result: &crate::video::ProbeResult,
    trim_start_seconds: f64,
    output_duration_seconds: f64,
    proof: Option<(u32, &str)>,
    bumpers: &Bumpers,
    hardware_encoder: crate::video::HardwareEncoder,
    accel: crate::video::Accel,
    tx: mpsc::UnboundedSender<InternalUpdate>,
    index: usize,
) -> Result<()> {
    // The GPU pipeline needs the source's size to scale it exactly; without it
    // the decode still runs on the GPU and the filters stay as they were.
    let source_size = probe_result
        .width
        .zip(probe_result.height)
        .filter(|(w, h)| *w > 0 && *h > 0)
        .map(|(w, h)| (w as u32, h as u32));
    let accel = match (accel, source_size) {
        (crate::video::Accel::Pipeline, None) => crate::video::Accel::Decode,
        (a, _) => a,
    };
    // Build optimized filter with format conversion AND subtitles in one pass
    // Use encoder-specific format optimization (NV12 for VideoToolbox/NVENC, yuv420p for software)
    let ass = ass_path.to_string_lossy().to_string();
    let is_hdr = crate::video::is_hdr(probe_result);
    let vf = match (accel, source_size) {
        (crate::video::Accel::Pipeline, Some((src_w, src_h))) => {
            crate::video::build_gpu_fitpad_filter(
                src_w,
                src_h,
                target_w,
                target_h,
                Some(&ass),
                crop_strategy,
            )
        }
        _ => crate::video::build_fitpad_filter_with_options(
            target_w,
            target_h,
            Some(&ass),
            hardware_encoder,
            crop_strategy,
            is_hdr,
        ),
    };

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

    let mut cmd = TokioCommand::new(&ffmpeg_path);
    cmd.kill_on_drop(true);

    // With a proof copy the burned frame is split in two: the full-size output as
    // before, and a scaled-down copy encoded alongside it, so the captions are drawn
    // and the source decoded only once. With an intro or outro, the clips are joined
    // to the burned video in the same graph, and the audio comes out of it too.
    let proof_scale = proof.map(|(side, _)| {
        if accel == crate::video::Accel::Pipeline {
            crate::video::gpu_proof_scale_filter(target_w, target_h, side)
        } else {
            proof_scale_filter(target_w, target_h, side)
        }
    });
    let main = main_video_chain(
        &vf,
        bumpers,
        target_w,
        target_h,
        pixel_format(hardware_encoder),
    );
    let filter_complex = if bumpers.is_empty() {
        match (&proof_scale, &bumpers.bug) {
            (Some(scale), _) => Some(format!("{main},split=2[full][small];[small]{scale}[proof]")),
            (None, Some(_)) => Some(format!("{main}[full]")),
            (None, None) => None,
        }
    } else {
        Some(bumper_graph(
            &main,
            target_w,
            target_h,
            probe_result.fps.filter(|f| *f > 0.0).unwrap_or(30.0),
            pixel_format(hardware_encoder),
            probe_result.audio,
            output_duration_seconds,
            bumpers,
            proof_scale.as_deref(),
        ))
    };
    // Audio of the joined video comes from the graph, of a plain one from the source.
    let (full_audio, proof_audio) = if bumpers.is_empty() {
        ("0:a?", "0:a?")
    } else {
        ("[afull]", "[aproof]")
    };

    let total_duration_seconds = output_duration_seconds + bumpers.duration();
    let duration_us = Some((total_duration_seconds * 1_000_000.0) as u64);
    let trim_start_arg = format!("{trim_start_seconds:.3}");
    let output_duration_arg = format!("{output_duration_seconds:.3}");

    cmd.args({
        let mut args = vec!["-y"];
        // The proof copy goes up to the GPU for its scaling and needs a device to do it.
        if accel == crate::video::Accel::Pipeline && proof.is_some() {
            args.extend_from_slice(&crate::video::VT_FILTER_DEVICE_ARGS);
        }
        if trim_start_seconds > 0.0 {
            args.extend_from_slice(&["-ss", &trim_start_arg]);
        }
        // -t before -i limits this input only, so the clips joined to it keep theirs.
        args.extend_from_slice(&["-t", &output_duration_arg]);
        if accel != crate::video::Accel::Off {
            args.extend(crate::video::vt_decode_args(
                accel == crate::video::Accel::Pipeline,
            ));
        }
        args.extend_from_slice(&["-i", input_video]);
        for clip in bumpers.clips() {
            // Intro and outro are decoded on the GPU too; they are scaled on the
            // CPU with the rest of the joining, so their frames come back as ever.
            if accel != crate::video::Accel::Off {
                args.extend(crate::video::vt_decode_args(false));
            }
            args.extend_from_slice(&["-i", &clip.path]);
        }
        if let Some(bug) = &bumpers.bug {
            args.extend_from_slice(&["-i", &bug.path]);
        }
        args.extend_from_slice(&["-progress", "pipe:1"]); // Enable progress reporting
        match &filter_complex {
            Some(graph) => args.extend_from_slice(&["-filter_complex", graph, "-map", "[full]"]),
            // Map first video stream
            None => args.extend_from_slice(&["-vf", &vf, "-map", "0:v:0"]),
        }
        args.extend_from_slice(&[
            "-fps_mode",
            "passthrough", // Modern replacement for -vsync
            "-threads",
            "0", // Use all available CPU cores
            "-map",
            full_audio, // Audio if present (optional)
        ]);

        // Add hardware-optimized encoding parameters
        match hardware_encoder {
            crate::video::HardwareEncoder::VideoToolbox => {
                // VideoToolbox in this ffmpeg build doesn't support -q:v
                // We use -b:v (bitrate) instead.
                // CRF 16 equivalent is roughly usually 10-12Mbps for 1080p, scaling accordingly.
                // Since specific bitrate control is robust, we use a high bitrate.
                args.extend_from_slice(&[
                    "-c:v",
                    "h264_videotoolbox",
                    "-b:v",
                    "12M", // High quality target (matching software CRF 16 intent)
                    "-allow_sw",
                    "1", // Allow software fallback
                    "-g",
                    &gop_size_str,
                ]);
            }
            crate::video::HardwareEncoder::Nvenc => {
                // Note: pix_fmt is already set in the filter (format=nv12), no need to duplicate
                args.extend_from_slice(&[
                    "-c:v",
                    "h264_nvenc",
                    "-cq",
                    "16",
                    "-preset",
                    "p5",
                    "-tune",
                    "hq",
                    "-rc",
                    "vbr",
                    "-g",
                    &gop_size_str,
                ]);
            }
            crate::video::HardwareEncoder::Software => {
                // Note: pix_fmt is already set in the filter (format=yuv420p), no need to duplicate
                args.extend_from_slice(&[
                    "-c:v",
                    "libx264",
                    "-preset",
                    "medium",
                    "-crf",
                    "16",
                    "-g",
                    &gop_size_str,
                ]);
            }
        }

        if bumpers.is_empty() {
            args.push("-c:a");
            args.push(audio_codec);

            // Add audio-specific args
            args.extend(audio_args.iter().copied());

            // Add explicit bitrate for re-encoded audio if not using copy
            if audio_codec != "copy" && audio_codec == "aac" && audio_args.is_empty() {
                args.extend_from_slice(&["-b:a", "160k"]);
            }
        } else {
            // Joined audio is new audio: nothing to copy.
            args.extend_from_slice(&["-c:a", "aac", "-b:a", "160k"]);
        }

        args.extend_from_slice(&[
            "-movflags",
            "+faststart", // Fast web playback
            output_path,
        ]);

        // Second output: the proof copy, same encoder family at a proofing bitrate.
        if let Some((_, proof_path)) = proof {
            args.extend_from_slice(&["-map", "[proof]", "-map", proof_audio]);
            args.extend(proof_video_args(hardware_encoder).iter().copied());
            args.extend_from_slice(&["-g", &gop_size_str, "-c:a", "aac", "-b:a", "128k"]);
            args.extend_from_slice(&["-movflags", "+faststart", proof_path]);
        }
        args
    });

    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(if crate::debug_enabled() {
        std::process::Stdio::inherit()
    } else {
        std::process::Stdio::null()
    });

    // Log intent
    let _ = tx.send(InternalUpdate::Event(RpcEvent::Log {
        id: id.into(),
        message: format!(
            "Starting encoder: {:?}, GPU use: {:?}",
            hardware_encoder, accel
        ),
    }));

    let mut child = cmd.spawn()?;

    // Process stdout for progress
    if let Some(stdout) = child.stdout.take() {
        let reader = tokio::io::BufReader::new(stdout);
        let mut lines = reader.lines();

        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(stripped) = line.strip_prefix("out_time_us=") {
                if let Ok(us) = stripped.trim().parse::<u64>() {
                    let progress = if let Some(total) = duration_us {
                        if total > 0 {
                            (us as f64 / total as f64).min(0.99) as f32
                        } else {
                            0.0
                        }
                    } else {
                        0.0
                    };

                    // Send progress update
                    let _ = tx.send(InternalUpdate::Progress {
                        index,
                        value: progress,
                    });
                }
            }
        }
    }

    let status = child.wait().await?;

    if !status.success() {
        let encoder_name = match hardware_encoder {
            crate::video::HardwareEncoder::VideoToolbox => "h264_videotoolbox",
            crate::video::HardwareEncoder::Nvenc => "h264_nvenc",
            crate::video::HardwareEncoder::Software => "libx264",
        };
        return Err(anyhow!(
            "FFmpeg failed to encode format for {} with encoder {}",
            id,
            encoder_name
        ));
    }

    Ok(())
}

/// A ready-made clip joined before or after the captioned video.
#[derive(Clone, Debug)]
struct Bumper {
    path: String,
    duration: f64,
    has_audio: bool,
}

/// Ready-made media added to a render: clips around the captioned video, and a
/// logo over it.
#[derive(Clone, Debug, Default)]
struct Bumpers {
    intro: Option<Bumper>,
    outro: Option<Bumper>,
    bug: Option<crate::types::WatermarkParams>,
}

impl Bumpers {
    /// No intro and no outro (a bug alone joins nothing).
    fn is_empty(&self) -> bool {
        self.intro.is_none() && self.outro.is_none()
    }

    /// The bug's input number: after the source and the clips.
    fn bug_input(&self) -> usize {
        1 + self.clips().count()
    }

    /// In input order: the intro is input 1 when there is one, the outro after it.
    fn clips(&self) -> impl Iterator<Item = &Bumper> {
        self.intro.iter().chain(self.outro.iter())
    }

    fn duration(&self) -> f64 {
        self.clips().map(|c| c.duration).sum()
    }
}

async fn probe_bumper(
    id: &str,
    path: Option<&str>,
    what: &str,
    emit: &mut impl FnMut(RpcEvent),
) -> Result<Option<Bumper>> {
    let Some(path) = path.filter(|p| !p.trim().is_empty()) else {
        return Ok(None);
    };
    if !std::path::Path::new(path).is_file() {
        return Err(anyhow!("{what} clip not found: {path}"));
    }
    let probed = probe(id, path, &mut *emit).await?;
    let duration = probed.duration.filter(|d| *d > 0.0);
    match (probed.video, duration) {
        (true, Some(duration)) => Ok(Some(Bumper {
            path: path.to_string(),
            duration,
            has_audio: probed.audio,
        })),
        _ => Err(anyhow!("{what} clip has no video to show: {path}")),
    }
}

fn pixel_format(encoder: crate::video::HardwareEncoder) -> &'static str {
    match encoder {
        crate::video::HardwareEncoder::Software => "yuv420p",
        _ => "nv12",
    }
}

fn check_watermark(
    watermark: Option<crate::types::WatermarkParams>,
) -> Result<Option<crate::types::WatermarkParams>> {
    let Some(mut wm) = watermark.filter(|w| !w.path.trim().is_empty()) else {
        return Ok(None);
    };
    if !std::path::Path::new(&wm.path).is_file() {
        return Err(anyhow!("Logo not found: {}", wm.path));
    }
    wm.size_pct = wm.size_pct.clamp(1.0, 60.0);
    wm.margin_pct = wm.margin_pct.clamp(0.0, 30.0);
    wm.opacity = wm.opacity.clamp(0.0, 1.0);
    Ok(Some(wm))
}

/// The logo's width and edge distance in pixels on a `w`×`h` frame. The editor
/// preview (video_canvas.watermark_rect) sizes it the same way.
fn watermark_geometry(wm: &crate::types::WatermarkParams, w: u32, h: u32) -> (u32, u32) {
    let short = w.min(h) as f32;
    let width = crate::video::round_even(((short * wm.size_pct / 100.0).round() as u32).max(2));
    let margin = (short * wm.margin_pct / 100.0).round() as u32;
    (width, margin)
}

/// The captioned source as the start of a filter chain, left open for the caller
/// to finish (`[v0]`, `split`, ...): the caption filter `vf`, and the logo laid
/// over it when there is one.
fn main_video_chain(vf: &str, bumpers: &Bumpers, w: u32, h: u32, pix_fmt: &str) -> String {
    let Some(wm) = &bumpers.bug else {
        return format!("[0:v:0]{vf}");
    };
    let (width, m) = watermark_geometry(wm, w, h);
    let x = if wm.corner.ends_with("left") {
        format!("{m}")
    } else {
        format!("W-w-{m}")
    };
    let y = if wm.corner.starts_with("bottom") {
        format!("H-h-{m}")
    } else {
        format!("{m}")
    };
    let input = bumpers.bug_input();
    format!(
        "[{input}:v:0]scale={width}:-2,format=rgba,colorchannelmixer=aa={op:.3}[bug];\
         [0:v:0]{vf}[captioned];[captioned][bug]overlay={x}:{y},format={pix_fmt}",
        op = wm.opacity
    )
}

/// The filter graph for a captioned video with an intro and/or outro: every part
/// brought to the output's frame, rate and pixel format, audio to 48 kHz stereo
/// (silence for a part without any), then concatenated. Outputs `[full]` and
/// `[afull]`, plus `[proof]` and `[aproof]` when `proof_scale` is given.
#[allow(clippy::too_many_arguments)]
fn bumper_graph(
    main: &str,
    target_w: u32,
    target_h: u32,
    fps: f64,
    pix_fmt: &str,
    main_has_audio: bool,
    main_duration: f64,
    bumpers: &Bumpers,
    proof_scale: Option<&str>,
) -> String {
    const AUDIO: &str = "aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo";
    let silence =
        |secs: f64| format!("anullsrc=r=48000:cl=stereo,atrim=duration={secs:.3},{AUDIO}");
    let mut chains = vec![
        format!("{main},setsar=1,fps={fps:.3}[v0]"),
        if main_has_audio {
            format!("[0:a:0]{AUDIO}[a0]")
        } else {
            format!("{}[a0]", silence(main_duration))
        },
    ];
    // Intro, main, outro, by input number.
    let mut order = Vec::new();
    let mut input = 1;
    for (clip, is_intro) in [(&bumpers.intro, true), (&bumpers.outro, false)] {
        let Some(clip) = clip else { continue };
        chains.push(format!(
            "[{input}:v:0]scale={target_w}:{target_h}:flags=lanczos:force_original_aspect_ratio=decrease,\
             pad={target_w}:{target_h}:(ow-iw)/2:(oh-ih)/2:black,setsar=1,fps={fps:.3},format={pix_fmt}[v{input}]"
        ));
        chains.push(if clip.has_audio {
            format!("[{input}:a:0]{AUDIO}[a{input}]")
        } else {
            format!("{}[a{input}]", silence(clip.duration))
        });
        if is_intro {
            order.push(input);
        }
        input += 1;
    }
    order.push(0);
    if bumpers.outro.is_some() {
        order.push(input - 1);
    }
    let parts: String = order.iter().map(|i| format!("[v{i}][a{i}]")).collect();
    let n = order.len();
    match proof_scale {
        None => chains.push(format!("{parts}concat=n={n}:v=1:a=1[full][afull]")),
        Some(scale) => {
            chains.push(format!("{parts}concat=n={n}:v=1:a=1[joined][ajoined]"));
            chains.push(format!(
                "[joined]split=2[full][small];[small]{scale}[proof]"
            ));
            chains.push("[ajoined]asplit=2[afull][aproof]".to_string());
        }
    }
    chains.join(";")
}

/// Scales a proof copy so its short side is `short_side`, keeping the aspect ratio
/// (the other side rounded to even, as H.264 needs).
fn proof_scale_filter(target_w: u32, target_h: u32, short_side: u32) -> String {
    if target_w < target_h {
        format!("scale={short_side}:-2")
    } else {
        format!("scale=-2:{short_side}")
    }
}

/// `clip_9x16.mp4` → `clip_9x16_proof720p.mp4`
fn proof_output_path(captioned_path: &str, short_side: u32) -> String {
    match captioned_path.strip_suffix(".mp4") {
        Some(stem) => format!("{stem}_proof{short_side}p.mp4"),
        None => format!("{captioned_path}_proof{short_side}p.mp4"),
    }
}

/// Encoder settings for a proof copy: small files meant for watching, not publishing.
fn proof_video_args(encoder: crate::video::HardwareEncoder) -> &'static [&'static str] {
    match encoder {
        crate::video::HardwareEncoder::VideoToolbox => &[
            "-c:v",
            "h264_videotoolbox",
            "-b:v",
            "2500k",
            "-allow_sw",
            "1",
        ],
        crate::video::HardwareEncoder::Nvenc => &[
            "-c:v",
            "h264_nvenc",
            "-cq",
            "26",
            "-preset",
            "p5",
            "-rc",
            "vbr",
        ],
        crate::video::HardwareEncoder::Software => {
            &["-c:v", "libx264", "-preset", "veryfast", "-crf", "26"]
        }
    }
}

// ---- Constants for horizontal stretch animation ----
const STRETCH_X_PEAK: f32 = 1.03; // 1.08–1.15 looks right
const STRETCH_UP_MIN_MS: i64 = 0;
const STRETCH_UP_MAX_MS: i64 = 150;
const BIG_FONT_SIZE_MULTIPLIER: f32 = 1.1;

// ---- Constants for bounce animation (non-karaoke) ----
const BOUNCE_START: f32 = 0.85; // 95%
const BOUNCE_PEAK: f32 = 1.05; // 103%
const BOUNCE_END: f32 = 1.0; // 100%
const BOUNCE_UP_MS: i64 = 100; // Time to reach peak
const BOUNCE_DOWN_MS: i64 = 66; // Time to settle

// ---- Smart highlight tuning (non-karaoke) ----
const HL_BASE_T: f32 = 2.5; // base threshold
const HL_HYSTERESIS: f32 = 0.7; // make back-to-back highlights harder
const HL_MIN_GAP_MS: u64 = 1200; // min time between highlights
const HL_MAX_RATIO: f32 = 0.35; // cap ~35% of phrases highlighted
const HL_RECENT_WINDOW_MS: u64 = 5000; // window for repetition penalty

#[allow(clippy::too_many_arguments)]
fn push_glow_and_stroke(
    lines: &mut String,
    start: &str,
    end: &str,
    text_body: &str, // ONLY \1c, \fs, \t(...). No \bord/\blur/\shad here.
    x: i32,
    y: i32,
    stroke_w: f32,     // black outline width
    enable_glow: bool, // whether to apply glow effect
    glow_w: f32,
    glow_blur: f32,
    glow_alpha_hex: &str, // e.g. "&H80" ~ 50% opacity
    alignment: u32,       // ASS alignment value (2 = bottom center, 5 = middle center)
    wrap_tag: &str,       // \q0 to let libass wrap, \q2 when we set the breaks
) {
    let common = format!("{{\\an{}{}\\pos({},{})\\be0}}", alignment, wrap_tag, x, y);

    // LAYER 0 — soft WHITE GLOW (outline only) - only if enabled
    if enable_glow {
        // hide fill (\1a&HFF), set white outline (\3c), set opacity (\3a), add blur
        let glow = format!(
            "{}{{\\1a&HFF\\bord{:.2}\\3c&HFFFFFF&\\3a{}\\blur{:.2}\\shad0}}",
            common, glow_w, glow_alpha_hex, glow_blur
        );
        lines.push_str(&format!(
            "Dialogue: 0,{},{},TikTok,,0,0,0,,{}{}\n",
            start, end, glow, text_body
        ));
    }

    // LAYER 1 (or 0 if no glow) — sharp black stroke + visible fill
    let layer = if enable_glow { 1 } else { 0 };
    let stroke_fill = format!(
        "{}{{\\1a&H00\\bord{:.2}\\3c&H000000&\\3a&H00\\blur0\\shad0}}",
        common, stroke_w
    );
    lines.push_str(&format!(
        "Dialogue: {},{},{},TikTok,,0,0,0,,{}{}\n",
        layer, start, end, stroke_fill, text_body
    ));
}

#[derive(Clone)]
#[allow(dead_code)]
struct Phrase {
    start_ms: u64,
    end_ms: u64,
    tokens: Vec<String>,  // plain words for layout
    spans: Vec<WordSpan>, // timings per token (same length as tokens)
}

/// One phrase per caption segment: the cue boundaries the user authored in the
/// editor are what gets burned in. Re-chunking here used to throw away every
/// split, merge and syllable fix made in the panel, which is why the burn never
/// matched the preview.
fn segment_phrases(segments: &[CaptionSegment]) -> Vec<Phrase> {
    crate::debug_log!(
        "DEBUG: Entering segment_phrases with {} segments",
        segments.len()
    );
    let mut out: Vec<Phrase> = Vec::new();
    for s in segments {
        let mut spans = segment_words(s);
        if spans.is_empty() {
            continue;
        }

        // A syllable glued to the previous cue belongs to the word that ends
        // that cue ("kaup" | "pa"), so move it there instead of showing half a
        // word on screen. Only the cue boundary moves; the split stays the
        // user's to make.
        if spans[0].glue_to_previous {
            if let Some(previous) = out.last_mut() {
                let syllable = spans.remove(0);
                if let Some(tail) = previous.spans.last_mut() {
                    tail.text.push_str(&syllable.text);
                    tail.end_ms = tail.end_ms.max(syllable.end_ms);
                }
                previous.end_ms = previous
                    .spans
                    .last()
                    .map(|w| w.end_ms)
                    .unwrap_or(previous.end_ms);
                previous.tokens = previous.spans.iter().map(|x| x.text.clone()).collect();
                if spans.is_empty() {
                    continue;
                }
            }
        }

        out.push(Phrase {
            start_ms: spans.first().unwrap().start_ms,
            end_ms: spans.last().unwrap().end_ms,
            tokens: spans.iter().map(|x| x.text.clone()).collect(),
            spans,
        });
    }
    out
}

/// The words of one segment, with syllables glued back into whole words.
fn segment_words(s: &CaptionSegment) -> Vec<WordSpan> {
    let mut all: Vec<WordSpan> = Vec::new();
    {
        let words_joined = s
            .words
            .iter()
            .map(|w| w.text.trim())
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join("");
        let seg_joined = s.text.split_whitespace().collect::<Vec<_>>().join("");
        let use_words = !s.words.is_empty() && (words_joined == seg_joined);

        if use_words {
            for w in &s.words {
                let t = w.text.trim();
                if t.is_empty() {
                    continue;
                }

                // Put a word back together before anything measures or wraps it.
                // Doing it here means line breaking, highlighting and karaoke all
                // see one word, with no special cases of their own.
                if w.glue_to_previous {
                    if let Some(previous) = all.last_mut() {
                        previous.text.push_str(t);
                        previous.end_ms = w.end_ms.max(previous.end_ms);
                        continue;
                    }
                }

                all.push(WordSpan {
                    start_ms: w.start_ms,
                    end_ms: w.end_ms,
                    text: t.to_string(),
                    // Kept on the leading word so the caller can glue it to the
                    // cue before this one.
                    glue_to_previous: w.glue_to_previous,
                });
            }
        } else if !s.text.trim().is_empty() {
            // User edited segment text (or words was empty) — always honor edited text
            let toks: Vec<_> = s.text.split_whitespace().collect();
            let total = (s.end_ms - s.start_ms).max(1);
            let n_toks = toks.len();
            let per = total / (n_toks.max(1) as u64);
            let mut t = s.start_ms;
            for (idx, tok) in toks.iter().enumerate() {
                let s0 = t;
                let e0 = if idx == n_toks - 1 {
                    s.end_ms
                } else {
                    (t + per).min(s.end_ms)
                };
                t = e0;
                all.push(WordSpan {
                    start_ms: s0,
                    end_ms: e0,
                    text: tok.to_string(),
                    glue_to_previous: false,
                });
            }
        }
    }

    all
}

/// Left/right margin the wrapper keeps free, as libass measures line width
/// against PlayResX minus these. 7% a side leaves captions 86% of the frame,
/// which is what the editor preview lays out against.
pub(crate) fn horizontal_margin(frame_w: u32) -> u32 {
    ((frame_w as f32) * CAPTION_SIDE_MARGIN_PCT / 100.0).round() as u32
}

/// Percentage of frame width kept free on each side.
pub(crate) const CAPTION_SIDE_MARGIN_PCT: f32 = 7.0;

// ---- time quantization (ASS is 1/100s) ----
fn ms_to_cs(ms: u64) -> i64 {
    (ms / 10) as i64
}
fn cs_to_ass(cs: i64) -> String {
    let total = cs.max(0);
    let h = total / 360000; // 3600*100
    let m = (total % 360000) / 6000;
    let s = (total % 6000) / 100;
    let c = total % 100;
    format!("{:01}:{:02}:{:02}.{:02}", h, m, s, c)
}

// Contiguous, non-overlapping windows in cs
fn contiguous_cs_windows(words: &[WordSpan]) -> Vec<(i64, i64)> {
    let mut out = Vec::with_capacity(words.len());
    for (i, w) in words.iter().enumerate() {
        let s = ms_to_cs(w.start_ms);
        let e = if i + 1 < words.len() {
            ms_to_cs(words[i + 1].start_ms) // [s, next_s)
        } else {
            ms_to_cs(w.end_ms) // last word keeps its end
        };
        out.push((s, (e.max(s + 1)))); // at least 1 cs
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
    format!(
        r"{{\fscx{start}\fscy{start}\t(0,{},\fscx{peak}\fscy{peak})\t({},{},\fscx{end_val}\fscy{end_val})}}",
        BOUNCE_UP_MS,
        BOUNCE_UP_MS,
        BOUNCE_UP_MS + BOUNCE_DOWN_MS
    )
}

// Uppercase + sanitize tokens (keeps punctuation)
fn normalize_tokens(words: &[WordSpan]) -> Vec<String> {
    words
        .iter()
        .map(|w| w.text.trim())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_uppercase())
        .collect()
}

// Color tags use BBGGRR (no alpha) for \1c
fn bgr_from_aa_bgrr(aa_bgrr: &str) -> String {
    aa_bgrr.trim_start_matches("&H").chars().skip(2).collect() // drop AA
}

/// Width available to caption text, both sides' margins removed.
pub(crate) fn caption_box_width(frame_w: u32) -> f32 {
    (frame_w as f32) - 2.0 * horizontal_margin(frame_w) as f32
}

/// Lay a cue out as justified lines, or as one plain line when justification
/// is off, so both callers below share one notion of "the lines of this cue".
fn cue_lines(
    tokens: &[String],
    justify_lines: bool,
    font_family: &str,
    frame_w: u32,
    base_px: u32,
) -> Vec<crate::justify::JustifiedLine> {
    if justify_lines {
        let lines = crate::justify::justify_two_lines(
            tokens,
            font_family,
            caption_box_width(frame_w),
            base_px,
        );
        if !lines.is_empty() {
            return lines;
        }
    }
    // Breaks decided here, for the whole cue: the highlighted word is set
    // bigger and each karaoke window opens with a stretch, and letting libass
    // wrap every window on its own made the lines jump from word to word.
    crate::justify::steady_lines(
        tokens,
        font_family,
        caption_box_width(frame_w) * STEADY_WRAP_SLACK,
        base_px,
        BIG_FONT_SIZE_MULTIPLIER,
        // Karaoke windows open stretched, plain cues bounce in: room for either.
        STRETCH_X_PEAK.max(BOUNCE_PEAK),
    )
}

/// Our advance-width sum ignores kerning and outline; leave libass a little room.
const STEADY_WRAP_SLACK: f32 = 0.97;

/// Whether cues carry their own line breaks (`\q2`) or libass wraps them (`\q0`):
/// only a font we can't measure is left to libass.
fn breaks_are_ours(font_family: &str, justify_lines: bool) -> bool {
    justify_lines || crate::text_metrics::metrics_for(font_family).is_some()
}

/// Render one cue's text body, justified or free-flowing.
#[allow(clippy::too_many_arguments)]
fn assemble_cue(
    tokens: &[String],
    layout: &[crate::justify::JustifiedLine],
    hi: usize,
    white_bgr: &str,
    hi_bgr: &str,
    header: &str,
    justify_lines: bool,
    font_size: u32,
) -> String {
    if justify_lines {
        assemble_justified(tokens, layout, hi, white_bgr, hi_bgr, header)
    } else {
        assemble_colored_lines(tokens, layout, hi, white_bgr, hi_bgr, header, font_size)
    }
}

/// Emit one cue's words with an explicit break and font size per line.
///
/// Used for the justified style, where every line carries its own `\fs` and no
/// automatic wrapping may interfere with the widths we computed.
fn assemble_justified(
    tokens: &[String],
    lines: &[crate::justify::JustifiedLine],
    hi: usize,
    white_bgr: &str,
    hi_bgr: &str,
    header: &str,
) -> String {
    let mut s = String::from(header);
    for (line_idx, line) in lines.iter().enumerate() {
        if line_idx > 0 {
            s.push_str(r"\N");
        }
        for (i, token) in tokens.iter().enumerate().take(line.end).skip(line.start) {
            let color = if hi != usize::MAX && i == hi {
                hi_bgr
            } else {
                white_bgr
            };
            // Size stays put even on the highlighted word: the line is flush
            // to the box, so growing one word would break the justification.
            s.push_str(&format!("{{\\1c&H{}&\\fs{}}}", color, line.font_px));
            s.push_str(
                &token
                    .replace('\\', r"\\")
                    .replace('{', r"\{")
                    .replace('}', r"\}"),
            );
            if i + 1 < line.end && !(token.ends_with('-') && token.len() > 1) {
                s.push(' ');
            }
        }
    }
    s
}

/// A cue's words at the style's size, the highlighted one bigger, broken into
/// `lines` (one line: libass wraps it).
fn assemble_colored_lines(
    tokens: &[String],
    lines: &[crate::justify::JustifiedLine],
    hi: usize,
    white_bgr: &str,
    hi_bgr: &str,
    header: &str,
    font_size: u32,
) -> String {
    let white = format!("{{\\1c&H{}&\\fs{}}}", white_bgr, font_size);
    let has_highlighting = hi != usize::MAX;
    let hi_style = if has_highlighting {
        let big_font_size = (font_size as f32 * BIG_FONT_SIZE_MULTIPLIER) as u32;
        format!("{{\\1c&H{}&\\fs{}}}", hi_bgr, big_font_size)
    } else {
        format!("{{\\1c&H{}&\\fs{}}}", hi_bgr, font_size)
    };
    let line_starts: Vec<usize> = lines.iter().skip(1).map(|l| l.start).collect();

    let mut s = String::from(header);
    for i in 0..tokens.len() {
        if line_starts.contains(&i) {
            s.push_str(r"\N");
        }
        s.push_str(if has_highlighting && i == hi {
            &hi_style
        } else {
            &white
        });
        s.push_str(
            &tokens[i]
                .replace('\\', r"\\")
                .replace('{', r"\{")
                .replace('}', r"\}"),
        );
        let ends_line = line_starts.contains(&(i + 1));
        let ends_with_hyphen = tokens[i].ends_with('-') && tokens[i].len() > 1;
        if i + 1 < tokens.len() && !ends_line && !ends_with_hyphen {
            s.push(' ');
        }
    }
    s
}

struct AssStyle {
    font_name: String,
    font_size: u32,
    primary: String,   // base (white)
    secondary: String, // unused here
    outline: String,
    outline_w: u32,
    /// ASS BorderStyle: 1 = outline + shadow around glyphs (no fill behind
    /// the line), 3 = an opaque/semi-transparent box behind the whole line.
    /// `outline_w` doubles as the per-glyph stroke width in mode 1, or the
    /// box padding in mode 3 — that's how libass itself overloads the
    /// `Outline` style field depending on BorderStyle.
    border_style: u32,
    shadow: u32,
    align: u32,        // 1..9 grid; 2 = bottom-center
    margin_v: u32,     // pixels
    highlight: String, // green for current word
}

/// The Y coordinate we hand to ASS `\pos` for a style with no manual override.
///
/// Which edge of the text block lands on that coordinate depends on the
/// alignment: `\an8` anchors the top, `\an5` the middle, `\an2` the bottom.
fn style_anchor_y(align: u32, margin_v: u32, frame_h: u32) -> i32 {
    match align {
        5 => (frame_h / 2) as i32,                      // Middle center
        8 => margin_v as i32,                           // Top center, margin from the top
        _ => (frame_h as i32 - margin_v as i32).max(0), // Bottom center, margin from the bottom
    }
}

/// Which edge of the text block sits on the anchor, for a given ASS alignment.
fn anchor_name(align: u32) -> &'static str {
    match align {
        5 => "center",
        8 => "top",
        _ => "bottom",
    }
}

/// Rough height of a caption block as a percentage of frame height. ASS lines
/// sit about 1.2 line heights apart; the outline adds a little.
fn block_height_pct(line_count: usize, font_size: u32, frame_h: u32) -> f32 {
    if frame_h == 0 {
        return 0.0;
    }
    (line_count.max(1) as f32) * (font_size as f32 * 1.25) / (frame_h as f32) * 100.0
}

/// Anchor Y for one cue, in pixels.
///
/// A manual placement wins outright — the user dragged it there. Otherwise the
/// style's own position is used, nudged clear of any interface the target
/// platforms draw over the video.
#[allow(clippy::too_many_arguments)]
fn cue_anchor_y(
    overrides: &[PositionOverride],
    blocked: &[(f32, f32)],
    start_ms: u64,
    end_ms: u64,
    default_y: i32,
    frame_h: u32,
    align: u32,
    line_count: usize,
    font_size: u32,
) -> i32 {
    let overridden = resolve_anchor_y(overrides, start_ms, end_ms, default_y, frame_h);
    if overridden != default_y || blocked.is_empty() || frame_h == 0 {
        return overridden;
    }

    let y_pct = (default_y as f32 / frame_h as f32) * 100.0;
    let dodged = crate::placement::dodge_blocked(
        y_pct,
        anchor_name(align),
        block_height_pct(line_count, font_size, frame_h),
        blocked,
    );
    ((dodged / 100.0) * frame_h as f32).round() as i32
}

/// Anchor Y for one cue: a manual override if the cue's midpoint falls inside
/// one, otherwise the style default.
fn resolve_anchor_y(
    overrides: &[PositionOverride],
    start_ms: u64,
    end_ms: u64,
    default_y: i32,
    frame_h: u32,
) -> i32 {
    if overrides.is_empty() {
        return default_y;
    }
    let mid_ms = start_ms + end_ms.saturating_sub(start_ms) / 2;
    overrides
        .iter()
        .find(|o| mid_ms >= o.start_ms && mid_ms <= o.end_ms)
        .map(|o| (((frame_h as f32) * (o.y_pct / 100.0)).round() as i32).clamp(0, frame_h as i32))
        .unwrap_or(default_y)
}

fn _pct_to_margin_v(frame_h: u32, y_pct_from_top: f32) -> u32 {
    // bottom-aligned: margin_v measured from bottom
    let y = (frame_h as f32 * (y_pct_from_top / 100.0)).round() as i32;
    (frame_h as i32 - y).max(0) as u32
}

fn stopwords() -> &'static HashSet<&'static str> {
    use std::sync::LazyLock;
    static SW: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
        [
            "a",
            "an",
            "the",
            "to",
            "of",
            "in",
            "on",
            "at",
            "by",
            "for",
            "with",
            "and",
            "or",
            "but",
            "i",
            "you",
            "he",
            "she",
            "we",
            "they",
            "be",
            "is",
            "are",
            "was",
            "were",
            "have",
            "has",
            "had",
            "do",
            "does",
            "did",
            "will",
            "would",
            "can",
            "could",
            "should",
            "shall",
            "may",
            "might",
            "must",
            "gonna",
            "wanna",
            "like",
            "just",
            "really",
            "very",
            "actually",
            "literally",
            "kinda",
            "sorta",
            "um",
            "uh",
            "you",
            "know",
        ]
        .into_iter()
        .collect()
    });
    &SW
}

fn power_words() -> &'static HashSet<&'static str> {
    use std::sync::LazyLock;
    static PW: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
        [
            "not", "no", "never", "without", "dont", "can't", "cant", "wont", "why", "must",
            "need", "free", "new", "massive", "insane", "huge", "proof", "secret", "banned",
        ]
        .into_iter()
        .collect()
    });
    &PW
}

fn build_global_tf(segments: &[CaptionSegment]) -> HashMap<String, u32> {
    let mut tf = HashMap::new();
    for s in segments {
        for w in &s.words {
            let t = w.text.trim();
            if t.is_empty() {
                continue;
            }
            *tf.entry(t.to_lowercase()).or_insert(0) += 1;
        }
    }
    tf
}

fn original_tokens(spans: &[WordSpan]) -> Vec<String> {
    spans
        .iter()
        .map(|w| w.text.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

fn has_digit_or_currency(s: &str) -> bool {
    s.chars()
        .any(|c| c.is_ascii_digit() || c == '$' || c == '%' || c == '#')
}

fn looks_proper_noun(token: &str, idx_in_phrase: usize) -> bool {
    if idx_in_phrase == 0 {
        return false;
    }
    let chars: Vec<char> = token.chars().collect();
    if chars.is_empty() {
        return false;
    }
    let first = chars[0];
    first.is_uppercase() && !token.chars().all(|c| c.is_uppercase())
}

fn ends_with_content_suffix(token: &str) -> bool {
    let l = token.to_lowercase();
    l.ends_with("ing") || l.ends_with("ed") || l.ends_with("ly")
}

fn mean_std(vals: &[f32]) -> (f32, f32) {
    if vals.is_empty() {
        return (0.0, 0.0);
    }
    let m = vals.iter().sum::<f32>() / vals.len() as f32;
    let v = vals.iter().map(|x| (x - m) * (x - m)).sum::<f32>() / vals.len() as f32;
    (m, v.sqrt())
}

struct HighlightState {
    tf: HashMap<String, u32>,
    recent: VecDeque<(String, u64)>, // (token_lower, time_ms)
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
            if end_ms.saturating_sub(t) > HL_RECENT_WINDOW_MS {
                self.recent.pop_front();
            } else {
                break;
            }
        }
        for t in tokens {
            self.recent.push_back((t.to_lowercase(), end_ms));
        }
    }

    fn recent_count(&self, token_lower: &str, now_ms: u64) -> u32 {
        self.recent
            .iter()
            .filter(|(w, t)| w == token_lower && now_ms.saturating_sub(*t) <= HL_RECENT_WINDOW_MS)
            .count() as u32
    }
}

fn choose_highlight_idx(
    tokens_orig: &[String],
    spans: &[WordSpan],
    phrase_idx: usize,
    st: &mut HighlightState,
) -> Option<usize> {
    let sw = stopwords();
    let pw = power_words();

    // rarity controls
    let mut threshold = HL_BASE_T;
    let phrase_start = spans.first().map(|w| w.start_ms).unwrap_or(0);
    let phrase_end = spans.last().map(|w| w.end_ms).unwrap_or(0);

    if let Some(last) = st.last_hl_ms {
        if phrase_start.saturating_sub(last) < HL_MIN_GAP_MS {
            threshold += 1.0;
        }
    }
    if st
        .last_hl_phrase
        .map(|p| p + 1 == phrase_idx)
        .unwrap_or(false)
    {
        threshold += HL_HYSTERESIS; // avoid back-to-back
    }
    if st.phrases_done > 0 && (st.phrases_hl as f32) / (st.phrases_done as f32) >= HL_MAX_RATIO {
        threshold += 0.8; // too many already
    }

    // candidates
    let cand: Vec<usize> = (0..tokens_orig.len())
        .filter(|&i| {
            let t = tokens_orig[i].trim();
            if t.is_empty() {
                return false;
            }
            let low = t.to_lowercase();
            if sw.contains(low.as_str()) {
                return false;
            }
            t.len() >= 3 || has_digit_or_currency(t)
        })
        .collect();

    if cand.is_empty() {
        st.phrases_done += 1;
        st.push_recent_phrase(tokens_orig, phrase_end);
        return None;
    }

    // features needing per-phrase stats
    let lens: Vec<f32> = tokens_orig.iter().map(|t| t.len() as f32).collect();
    let mut lens_sorted = lens.clone();
    lens_sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let med_len = lens_sorted[lens_sorted.len() / 2];

    let durs: Vec<f32> = spans
        .iter()
        .map(|w| (w.end_ms - w.start_ms) as f32)
        .collect();
    let (mean_dur, std_dur) = mean_std(&durs);

    // score
    let mut best: Option<(usize, f32)> = None;
    for &i in &cand {
        let t = tokens_orig[i].trim();
        let low = t.to_lowercase();
        let mut s = 0.0;

        if has_digit_or_currency(t) {
            s += 3.0;
        }
        if st.tf.get(&low).copied().unwrap_or(0) <= 2 {
            s += 2.0;
        }
        if looks_proper_noun(t, i) {
            s += 1.5;
        }
        if pw.contains(low.as_str()) {
            s += 1.5;
        }
        if ends_with_content_suffix(t) {
            s += 1.0;
        }
        if (t.len() as f32) > med_len {
            s += 1.0;
        }

        if std_dur > 0.0 {
            let z = (durs[i] - mean_dur) / std_dur;
            s += 0.5 * z.max(0.0); // only reward longer-than-avg
        }

        // pause / phrase-final emphasis
        if i + 1 == spans.len() {
            s += 0.5;
        } else {
            let gap = spans[i + 1].start_ms.saturating_sub(spans[i].end_ms);
            if gap >= 250 {
                s += 0.5;
            }
        }

        // penalties
        if st.recent_count(&low, phrase_end) > 3 {
            s -= 2.0;
        }
        if t.chars().all(|c| c.is_uppercase())
            && !tokens_orig
                .iter()
                .all(|w| w.chars().all(|c| c.is_uppercase()))
        {
            s -= 1.0;
        }

        // tie-breakers inline
        if s >= threshold {
            match best {
                None => best = Some((i, s)),
                Some((bi, bs)) => {
                    if (s > bs)
                        || (s == bs && i > bi)              // later in phrase
                        || (s == bs && durs[i] > durs[bi])   // longer held
                        || (s == bs && st.tf.get(&low).unwrap_or(&u32::MAX) < st.tf.get(&tokens_orig[bi].to_lowercase()).unwrap_or(&u32::MAX))
                    {
                        best = Some((i, s));
                    }
                }
            }
        }
    }

    st.phrases_done += 1;
    st.push_recent_phrase(tokens_orig, phrase_end);

    if let Some((idx, _)) = best {
        st.phrases_hl += 1;
        st.last_hl_ms = Some(phrase_end);
        st.last_hl_phrase = Some(phrase_idx);
        Some(idx)
    } else {
        None
    }
}

#[allow(clippy::too_many_arguments)]
fn build_ass_document(
    w: u32,
    h: u32,
    style: &AssStyle,
    segments: &[CaptionSegment],
    karaoke: bool,
    multiline: bool,
    justify_lines: bool,
    glow_effect: bool,
    position_overrides: &[PositionOverride],
    blocked_bands: &[(f32, f32)],
) -> Result<String> {
    if segments.is_empty() {
        return Err(anyhow!("No caption segments"));
    }
    crate::debug_log!(
        "DEBUG: build_ass_document start. karaoke={}, multiline={}, glow={}",
        karaoke,
        multiline,
        glow_effect
    );

    let header = format!(
        r#"[Script Info]
ScriptType: v4.00+
PlayResX: {w}
PlayResY: {h}
ScaledBorderAndShadow: yes

[V4+ Styles]
Format: Name,Fontname,Fontsize,PrimaryColour,SecondaryColour,OutlineColour,BackColour,Bold,Italic,Underline,StrikeOut,ScaleX,ScaleY,Spacing,Angle,BorderStyle,Outline,Shadow,Alignment,MarginL,MarginR,MarginV,Encoding
Style: TikTok,{font},{size},{pri},{sec},{out},&H64000000,0,0,0,0,100,100,0,0,{bs},{ow},{sh},{al},{mh},{mh},{mv},1

[Events]
Format: Layer,Start,End,Style,Name,MarginL,MarginR,MarginV,Effect,Text
"#,
        w = w,
        h = h,
        font = style.font_name,
        size = style.font_size,
        pri = style.primary,
        sec = style.secondary,
        out = style.outline,
        bs = style.border_style,
        ow = style.outline_w,
        sh = style.shadow,
        al = style.align,
        mh = horizontal_margin(w),
        mv = style.margin_v
    );

    let mut lines = String::new();
    // Cues carry their own breaks (see cue_lines), so libass must not re-wrap
    // them; only a font we can't measure is left to libass's wrapping.
    let wrap_tag = if breaks_are_ours(&style.font_name, justify_lines) {
        r"\q2"
    } else {
        r"\q0"
    };

    if karaoke {
        let phrases = segment_phrases(segments);
        let white_bgr = bgr_from_aa_bgrr(&style.primary);
        let hi_bgr = bgr_from_aa_bgrr(&style.highlight);

        // One cue per authored segment, its lines decided once for the whole
        // cue (cue_lines) so every karaoke window below breaks them the same.
        for ph in phrases {
            let tokens_upper = normalize_tokens(&ph.spans);
            let cue_layout = cue_lines(
                &tokens_upper,
                justify_lines,
                &style.font_name,
                w,
                style.font_size,
            );
            let segments = vec![(tokens_upper, ph.spans.clone(), usize::MAX)];

            let default_y = style_anchor_y(style.align, style.margin_v, h);

            // Process each width-appropriate segment
            for (segment_tokens, segment_spans, split_idx) in segments {
                // Every karaoke window of this block shares one Y, so a caption
                // the user dragged moves as a whole instead of word by word.
                let y_pos = cue_anchor_y(
                    position_overrides,
                    blocked_bands,
                    segment_spans.first().map(|s| s.start_ms).unwrap_or(0),
                    segment_spans.last().map(|s| s.end_ms).unwrap_or(0),
                    default_y,
                    h,
                    style.align,
                    cue_layout.len(),
                    style.font_size,
                );
                let _ = split_idx;

                let windows = contiguous_cs_windows(&segment_spans);

                for (i, (cs0, cs1)) in windows.iter().enumerate() {
                    let dur_ms = (cs1 - cs0) * 10;
                    let blur_value = if glow_effect { 6.0 } else { 2.0 };

                    let header = format!(
                        "{{\\an{}{}\\pos({},{})\\bord{}\\blur{:.1}}}{}",
                        style.align,
                        wrap_tag,
                        (w / 2),
                        y_pos,
                        style.outline_w,
                        blur_value,
                        stretch_tag_ms(dur_ms)
                    );

                    if glow_effect {
                        // Glow layer
                        let glow_header = format!(
                        "{{\\an{}{}\\pos({},{})\\1a&HFF\\bord{}\\3c&HFFFFFF&\\3a&H80\\blur{:.1}\\shad0}}{}",
                        style.align, wrap_tag, (w/2), y_pos,
                        style.outline_w as f32 * 2.0,
                        6.0,
                        stretch_tag_ms(dur_ms)
                    );
                        let glow_text = assemble_cue(
                            &segment_tokens,
                            &cue_layout,
                            i,
                            &white_bgr,
                            &hi_bgr,
                            &glow_header,
                            justify_lines,
                            style.font_size,
                        );
                        lines.push_str(&format!(
                            "Dialogue: 0,{},{},TikTok,,0,0,0,,{}\n",
                            cs_to_ass(*cs0),
                            cs_to_ass(*cs1),
                            glow_text
                        ));

                        // Main text layer
                        let main_header = format!(
                            "{{\\an{}{}\\pos({},{})\\bord{}\\blur0\\shad0}}{}",
                            style.align,
                            wrap_tag,
                            (w / 2),
                            y_pos,
                            style.outline_w,
                            stretch_tag_ms(dur_ms)
                        );
                        let main_text = assemble_cue(
                            &segment_tokens,
                            &cue_layout,
                            i,
                            &white_bgr,
                            &hi_bgr,
                            &main_header,
                            justify_lines,
                            style.font_size,
                        );
                        lines.push_str(&format!(
                            "Dialogue: 1,{},{},TikTok,,0,0,0,,{}\n",
                            cs_to_ass(*cs0),
                            cs_to_ass(*cs1),
                            main_text
                        ));
                    } else {
                        // Single layer
                        let text = assemble_cue(
                            &segment_tokens,
                            &cue_layout,
                            i,
                            &white_bgr,
                            &hi_bgr,
                            &header,
                            justify_lines,
                            style.font_size,
                        );
                        lines.push_str(&format!(
                            "Dialogue: 0,{},{},TikTok,,0,0,0,,{}\n",
                            cs_to_ass(*cs0),
                            cs_to_ass(*cs1),
                            text
                        ));
                    }
                }
            }
        }
    } else {
        let white_bgr = bgr_from_aa_bgrr(&style.primary);
        let hi_bgr = bgr_from_aa_bgrr(&style.highlight);
        let x = (w / 2) as i32;
        let default_y = style_anchor_y(style.align, style.margin_v, h);

        let phrases = segment_phrases(segments);

        // NEW: state for smart highlighting
        let mut hl_state = HighlightState::new(segments);

        for (p_idx, phrase) in phrases.iter().enumerate() {
            crate::debug_log!("DEBUG: Processing phrase {}/{}", p_idx, phrases.len());
            let tokens_upper = normalize_tokens(&phrase.spans);

            // One cue per authored segment, broken into lines by cue_lines.
            let segments = vec![(tokens_upper, phrase.spans.clone())];

            for (segment_tokens, segment_spans) in segments {
                let segment_tokens_orig = original_tokens(&segment_spans);

                let start = cs_to_ass(ms_to_cs(segment_spans.first().unwrap().start_ms));
                let end = cs_to_ass(ms_to_cs(segment_spans.last().unwrap().end_ms));

                // Decide which single word (if any) to highlight in this segment
                let hi_opt = choose_highlight_idx(
                    &segment_tokens_orig,
                    &segment_spans,
                    p_idx,
                    &mut hl_state,
                );
                let hi_idx = hi_opt.unwrap_or(usize::MAX); // usize::MAX => no highlight

                // One assembly for every mode: the cue flows as authored and
                // libass breaks it where it no longer fits. Storyteller differs
                // only in its centered alignment (style.align == 5).
                let cue_layout = cue_lines(
                    &segment_tokens,
                    justify_lines,
                    &style.font_name,
                    w,
                    style.font_size,
                );
                let text_body = assemble_cue(
                    &segment_tokens,
                    &cue_layout,
                    hi_idx,
                    &white_bgr,
                    &hi_bgr,
                    &bounce_tag(), // entrance scale
                    justify_lines,
                    style.font_size,
                );

                // Now that the text is assembled we know how many lines it
                // has, which is what deciding whether it clears the platform
                // interface depends on.
                let y = cue_anchor_y(
                    position_overrides,
                    blocked_bands,
                    segment_spans.first().unwrap().start_ms,
                    segment_spans.last().unwrap().end_ms,
                    default_y,
                    h,
                    style.align,
                    1 + text_body.matches(r"\N").count(),
                    style.font_size,
                );

                // Your layered renderer (glow + black stroke + fill)
                let glow_w = style.outline_w as f32 * 2.0;
                let glow_blur = 6.0;
                let stroke_w = style.outline_w as f32;

                push_glow_and_stroke(
                    &mut lines,
                    &start,
                    &end,
                    &text_body,
                    x,
                    y,
                    stroke_w,
                    glow_effect, // Use the parameter to control glow
                    glow_w,
                    glow_blur,
                    "&H80",      // ~50% white glow
                    style.align, // Pass the alignment from style
                    wrap_tag,
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
fn calculate_proportional_font_size(
    frame_w: u32,
    frame_h: u32,
    base_font_size: Option<u32>,
) -> u32 {
    // Reference dimensions for 9:16 format at 1080p height
    let reference_width = 608.0; // 9:16 aspect ratio at 1080p height
    let reference_height = 1080.0;
    let reference_area = reference_width * reference_height;
    // Use provided base font size or default to 6% of height (approx 65px at 1080p)
    let reference_font_size = base_font_size.unwrap_or((reference_height * 0.06) as u32) as f32;

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
#[allow(clippy::too_many_arguments)]
fn default_ass_style(
    frame_w: u32,
    frame_h: u32,
    font_name: Option<&str>,
    text_color: Option<&str>,
    highlight_color: Option<&str>,
    outline_color: Option<&str>,
    outline_width: Option<u32>,
    background_box: bool,
    _glow_effect: bool,
    position: Option<&str>,
    font_size: Option<u32>,
) -> AssStyle {
    // Convert hex colors to ASS format (AABBGGRR), use defaults if None
    let primary = text_color
        .map(hex_to_ass_color)
        .unwrap_or_else(|| "&H00FFFFFF".into());
    let highlight = highlight_color
        .map(hex_to_ass_color)
        .unwrap_or_else(|| "&H0000FFFE".into());
    let outline = outline_color
        .map(hex_to_ass_color)
        .unwrap_or_else(|| "&H00000000".into());

    // Helper for percentage of height
    let pct_h = |p: f32| -> u32 { (frame_h as f32 * (p / 100.0)).round() as u32 };

    // Determine vertical position and alignment based on position parameter
    let (align, margin_v) = match position.unwrap_or("bottom") {
        "top" => (8, pct_h(12.0)),            // Top center, 12% from top
        "top-quarter" => (8, pct_h(25.0)),    // Top center, 25% from top
        "center" => (5, 0),                   // Middle center
        "bottom-quarter" => (2, pct_h(25.0)), // Bottom center, 25% from bottom
        "safe-center" => (5, 0),              // Legacy support: Middle center
        _ => (2, pct_h(12.0)),                // Bottom center, 12% from bottom (default)
    };

    let resolved_font_size = calculate_proportional_font_size(frame_w, frame_h, font_size);

    // In box mode there's no per-glyph stroke — the box itself separates
    // text from background — so `outline_w` here is repurposed as box
    // padding instead, scaled with font size to stay proportional like the
    // editor's own preview pill does.
    let (border_style, outline_w) = if background_box {
        let padding = ((resolved_font_size as f32) * 0.22).round().max(6.0) as u32;
        (3, padding)
    } else {
        (1, outline_width.unwrap_or(4))
    };

    AssStyle {
        font_name: font_name.unwrap_or("Montserrat Black").into(),
        font_size: resolved_font_size,
        primary: primary.clone(),
        secondary: primary,
        outline,
        outline_w,
        border_style,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_assemble_colored_lines_hyphenation() {
        let tokens = vec!["SAKSALAIS-".to_string(), "ROOMALAINEN".to_string()];
        let one_line = [crate::justify::JustifiedLine {
            start: 0,
            end: 2,
            font_px: 20,
        }];
        let result = assemble_colored_lines(
            &tokens,
            &one_line,
            usize::MAX,
            "FFFFFF",
            "0000FF",
            "{\\an2}",
            20,
        );
        assert!(
            !result.contains("SAKSALAIS- "),
            "Should not contain space after hyphen"
        );
    }

    fn karaoke_block(words: &[&str]) -> Vec<CaptionSegment> {
        let spans: Vec<WordSpan> = words
            .iter()
            .enumerate()
            .map(|(i, w)| WordSpan {
                start_ms: i as u64 * 400,
                end_ms: (i as u64 + 1) * 400,
                text: (*w).into(),
                glue_to_previous: false,
            })
            .collect();
        vec![CaptionSegment {
            start_ms: 0,
            end_ms: spans.len() as u64 * 400,
            text: words.join(" "),
            words: spans,
        }]
    }

    #[test]
    fn a_karaoke_block_keeps_its_line_breaks_on_every_word() {
        let segments = karaoke_block(&[
            "agentit,",
            "olivat",
            "löytäneet",
            "tavan",
            "kommunikoida",
            "keskenään.",
        ]);
        let style = default_ass_style(
            1080,
            1920,
            Some("Montserrat Black"),
            None,
            None,
            None,
            None,
            false,
            false,
            None,
            Some(80),
        );
        let doc = build_ass_document(
            1080,
            1920,
            &style,
            &segments,
            true,
            false,
            false,
            false,
            &[],
            &[],
        )
        .expect("document builds");
        // Where each window breaks its lines, as the words before each \N.
        let layouts: Vec<Vec<String>> = doc
            .lines()
            .filter(|l| l.starts_with("Dialogue:"))
            .map(|l| {
                let plain = regex_free_text(l);
                plain
                    .split(r"\N")
                    .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
                    .collect()
            })
            .collect();
        assert_eq!(layouts.len(), 6, "one window per word");
        assert!(
            layouts[0].len() >= 2,
            "a long block needs lines: {:?}",
            layouts[0]
        );
        assert!(
            layouts.iter().all(|l| l == &layouts[0]),
            "lines moved between words: {layouts:#?}"
        );
        // libass must not wrap on top of our breaks.
        assert!(doc
            .lines()
            .filter(|l| l.starts_with("Dialogue:"))
            .all(|l| l.contains(r"\q2")));
    }

    /// A dialogue line's text with the {override} blocks taken out.
    fn regex_free_text(line: &str) -> String {
        let text = line.splitn(10, ',').nth(9).unwrap_or("");
        let mut out = String::new();
        let mut depth = 0;
        for c in text.chars() {
            match c {
                '{' => depth += 1,
                '}' => depth -= 1,
                _ if depth == 0 => out.push(c),
                _ => {}
            }
        }
        out
    }

    #[test]
    fn justified_cues_carry_one_font_size_per_line() {
        let segments = vec![CaptionSegment {
            start_ms: 0,
            end_ms: 2000,
            text: "vähän niinku huonosta miesvalinnasta".to_string(),
            words: vec![
                WordSpan {
                    start_ms: 0,
                    end_ms: 500,
                    text: "vähän".into(),
                    glue_to_previous: false,
                },
                WordSpan {
                    start_ms: 500,
                    end_ms: 1000,
                    text: "niinku".into(),
                    glue_to_previous: false,
                },
                WordSpan {
                    start_ms: 1000,
                    end_ms: 1500,
                    text: "huonosta".into(),
                    glue_to_previous: false,
                },
                WordSpan {
                    start_ms: 1500,
                    end_ms: 2000,
                    text: "miesvalinnasta".into(),
                    glue_to_previous: false,
                },
            ],
        }];

        let style = default_ass_style(
            1080,
            1920,
            Some("Montserrat Black"),
            None,
            None,
            None,
            None,
            false,
            false,
            None,
            Some(80),
        );
        let doc = build_ass_document(
            1080,
            1920,
            &style,
            &segments,
            false, // karaoke
            false, // multiline
            true,  // justify_lines
            false, // glow
            &[],
            &[],
        )
        .expect("document builds");

        let dialogue = doc
            .lines()
            .find(|l| l.starts_with("Dialogue:"))
            .expect("a dialogue line");

        // Two lines, and the shorter one is set larger so both fill the box.
        assert!(
            dialogue.contains(r"\N"),
            "expected a line break in {dialogue}"
        );
        let sizes: Vec<u32> = dialogue
            .match_indices(r"\fs")
            .filter_map(|(i, _)| {
                // Skip \fscx / \fscy from the bounce animation.
                let digits: String = dialogue[i + 3..]
                    .chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect();
                digits.parse().ok()
            })
            .collect();
        assert!(sizes.len() >= 4, "one size tag per word: {sizes:?}");
        assert!(
            sizes[0] > *sizes.last().unwrap(),
            "first line should be the big one: {sizes:?}"
        );
        // Justified cues set their own breaks, so libass must not re-wrap.
        assert!(dialogue.contains(r"\q2"), "expected \\q2 in {dialogue}");
    }

    #[test]
    fn unjustified_short_cues_stay_on_one_line_at_one_size() {
        let segments = vec![CaptionSegment {
            start_ms: 0,
            end_ms: 1000,
            text: "ja se".to_string(),
            words: vec![
                WordSpan {
                    start_ms: 0,
                    end_ms: 500,
                    text: "ja".into(),
                    glue_to_previous: false,
                },
                WordSpan {
                    start_ms: 500,
                    end_ms: 1000,
                    text: "se".into(),
                    glue_to_previous: false,
                },
            ],
        }];

        let style = default_ass_style(
            1080,
            1920,
            Some("Montserrat Black"),
            None,
            None,
            None,
            None,
            false,
            false,
            None,
            Some(80),
        );
        let doc = build_ass_document(
            1080,
            1920,
            &style,
            &segments,
            false,
            false,
            false,
            false,
            &[],
            &[],
        )
        .expect("document builds");

        let dialogue = doc
            .lines()
            .find(|l| l.starts_with("Dialogue:"))
            .expect("a dialogue line");
        // Our breaks, not libass's: a short cue simply has none.
        assert!(dialogue.contains(r"\q2"), "expected \\q2 in {dialogue}");
        assert!(
            !dialogue.contains(r"\N"),
            "no forced break expected in {dialogue}"
        );
    }
}

#[cfg(test)]
mod tests_proof_copy {
    use super::*;

    #[test]
    fn proof_scales_the_short_side() {
        assert_eq!(proof_scale_filter(1920, 1080, 720), "scale=-2:720");
        assert_eq!(proof_scale_filter(1080, 1920, 720), "scale=720:-2");
        assert_eq!(proof_scale_filter(1080, 1080, 540), "scale=-2:540");
    }

    #[test]
    fn proof_sits_next_to_the_full_size_video() {
        assert_eq!(
            proof_output_path("/v/talk.mp4_9x16.mp4", 720),
            "/v/talk.mp4_9x16_proof720p.mp4"
        );
        assert_eq!(proof_output_path("/v/raw", 540), "/v/raw_proof540p.mp4");
    }

    fn clip(path: &str, duration: f64, has_audio: bool) -> Bumper {
        Bumper {
            path: path.into(),
            duration,
            has_audio,
        }
    }

    #[test]
    fn intro_and_outro_play_around_the_video() {
        let bumpers = Bumpers {
            intro: Some(clip("intro.mp4", 3.0, true)),
            outro: Some(clip("outro.mov", 5.5, false)),
            ..Default::default()
        };
        assert_eq!(bumpers.duration(), 8.5);
        let paths: Vec<_> = bumpers.clips().map(|c| c.path.as_str()).collect();
        assert_eq!(paths, ["intro.mp4", "outro.mov"]);
        let graph = bumper_graph(
            "[0:v:0]VF",
            1080,
            1920,
            25.0,
            "nv12",
            true,
            60.0,
            &bumpers,
            None,
        );
        assert!(graph.contains("[0:v:0]VF,setsar=1,fps=25.000[v0]"));
        assert!(graph.contains("[1:v:0]scale=1080:1920:"));
        assert!(graph.contains(
            "pad=1080:1920:(ow-iw)/2:(oh-ih)/2:black,setsar=1,fps=25.000,format=nv12[v1]"
        ));
        assert!(graph.contains("[1:a:0]aresample=48000"));
        // The outro has no sound of its own: silence as long as it is.
        assert!(graph.contains("anullsrc=r=48000:cl=stereo,atrim=duration=5.500,"));
        assert!(graph.ends_with("[v1][a1][v0][a0][v2][a2]concat=n=3:v=1:a=1[full][afull]"));
    }

    #[test]
    fn an_outro_alone_is_input_one_and_comes_last() {
        let bumpers = Bumpers {
            intro: None,
            outro: Some(clip("outro.mp4", 4.0, true)),
            ..Default::default()
        };
        let graph = bumper_graph(
            "[0:v:0]VF",
            1920,
            1080,
            30.0,
            "yuv420p",
            false,
            12.0,
            &bumpers,
            None,
        );
        assert!(graph.contains("atrim=duration=12.000,")); // silent main video
        assert!(graph.ends_with("[v0][a0][v1][a1]concat=n=2:v=1:a=1[full][afull]"));
    }

    #[test]
    fn the_proof_copy_is_split_after_the_join() {
        let bumpers = Bumpers {
            intro: Some(clip("i.mp4", 2.0, true)),
            outro: None,
            ..Default::default()
        };
        let graph = bumper_graph(
            "VF",
            1920,
            1080,
            30.0,
            "nv12",
            true,
            10.0,
            &bumpers,
            Some("scale=-2:720"),
        );
        assert!(graph.contains("concat=n=2:v=1:a=1[joined][ajoined]"));
        assert!(graph.contains("[joined]split=2[full][small];[small]scale=-2:720[proof]"));
        assert!(graph.ends_with("[ajoined]asplit=2[afull][aproof]"));
    }

    fn bug(corner: &str) -> crate::types::WatermarkParams {
        crate::types::WatermarkParams {
            path: "logo.png".into(),
            corner: corner.into(),
            size_pct: 12.0,
            margin_pct: 4.0,
            opacity: 0.8,
        }
    }

    #[test]
    fn the_bug_sits_in_its_corner_sized_by_the_short_side() {
        // 1080 is the short side of both frames: 12 % is 130 px, 4 % is 43 px.
        assert_eq!(watermark_geometry(&bug("top-right"), 1080, 1920), (130, 43));
        assert_eq!(watermark_geometry(&bug("top-right"), 1920, 1080), (130, 43));
        let only_bug = Bumpers {
            bug: Some(bug("top-right")),
            ..Default::default()
        };
        let chain = main_video_chain("VF", &only_bug, 1080, 1920, "nv12");
        assert_eq!(
            chain,
            "[1:v:0]scale=130:-2,format=rgba,colorchannelmixer=aa=0.800[bug];\
             [0:v:0]VF[captioned];[captioned][bug]overlay=W-w-43:43,format=nv12"
        );
        let left = Bumpers {
            bug: Some(bug("bottom-left")),
            ..Default::default()
        };
        assert!(main_video_chain("VF", &left, 1080, 1920, "nv12").contains("overlay=43:H-h-43"));
        let plain = Bumpers::default();
        assert_eq!(
            main_video_chain("VF", &plain, 1080, 1920, "nv12"),
            "[0:v:0]VF"
        );
    }

    #[test]
    fn the_bug_comes_after_the_clips_and_stays_off_them() {
        let all = Bumpers {
            intro: Some(clip("i.mp4", 2.0, true)),
            outro: Some(clip("o.mp4", 2.0, true)),
            bug: Some(bug("top-left")),
        };
        assert_eq!(all.bug_input(), 3);
        let main = main_video_chain("VF", &all, 1920, 1080, "nv12");
        let graph = bumper_graph(&main, 1920, 1080, 30.0, "nv12", true, 10.0, &all, None);
        assert!(graph.starts_with("[3:v:0]scale="));
        // Only the source goes under the logo; the clips are scaled on their own.
        assert!(graph.contains("[captioned][bug]overlay=43:43,format=nv12,setsar=1,fps=30.000[v0]"));
        assert!(graph.contains("[1:v:0]scale=1920:1080:"));
    }

    #[test]
    fn a_missing_logo_is_an_error_and_an_empty_one_is_none() {
        assert!(check_watermark(Some(bug("top-right"))).is_err());
        let mut empty = bug("top-right");
        empty.path = " ".into();
        assert_eq!(check_watermark(Some(empty)).unwrap(), None);
        let parsed: crate::types::WatermarkParams =
            serde_json::from_value(serde_json::json!({"path": "x.png"})).unwrap();
        assert_eq!(
            (parsed.corner.as_str(), parsed.size_pct),
            ("top-right", 12.0)
        );
    }

    #[test]
    fn intro_and_outro_are_optional_in_burn_params() {
        let base = serde_json::json!({
            "inputVideo": "in.mp4", "segments": [], "exportFormats": ["16:9"],
            "karaoke": false, "fontName": null
        });
        let without: BurnCaptionsParams = serde_json::from_value(base.clone()).unwrap();
        assert_eq!((without.intro_video, without.outro_video), (None, None));
        let mut with = base;
        with["outroVideo"] = serde_json::json!("/clips/outro.mp4");
        let with: BurnCaptionsParams = serde_json::from_value(with).unwrap();
        assert_eq!(with.outro_video.as_deref(), Some("/clips/outro.mp4"));
    }

    #[test]
    fn proof_copy_is_optional_in_burn_params() {
        let base = serde_json::json!({
            "inputVideo": "in.mp4",
            "segments": [],
            "exportFormats": ["16:9"],
            "karaoke": false,
            "fontName": null
        });
        let without: BurnCaptionsParams = serde_json::from_value(base.clone()).unwrap();
        assert_eq!(without.proof_short_side, None);
        let mut with = base;
        with["proofShortSide"] = serde_json::json!(720);
        let with: BurnCaptionsParams = serde_json::from_value(with).unwrap();
        assert_eq!(with.proof_short_side, Some(720));
    }
}
