use crate::rpc::RpcEvent;
use serde::{Deserialize, Serialize};
use tokio::process::Command as TokioCommand;
use std::process::Command;

/// Properly escape subtitle file paths for FFmpeg subtitle filter
/// Handles Windows paths with drive letters and special characters
pub fn escape_subtitle_path(path: &str) -> String {
    // First, escape backslashes for Windows paths
    let mut escaped = path.replace('\\', r"\\");

    // Escape colons (Windows drive letters and FFmpeg filter separators)
    escaped = escaped.replace(':', r"\:");

    // Quote the entire path to handle spaces and other special characters
    format!("'{}'", escaped)
}

#[derive(Clone, Copy)]
pub enum TargetAR {
    AR9x16,
    AR16x9,
    AR4x5,
    AR1x1
}

fn round_even(x: u32) -> u32 {
    (x & !1) + (x & 1) // ensure even for yuv420
}

fn ar_wh(ar: TargetAR) -> (u32, u32) {
    match ar {
        TargetAR::AR9x16 => (9, 16),
        TargetAR::AR16x9 => (16, 9),
        TargetAR::AR4x5  => (4, 5),
        TargetAR::AR1x1  => (1, 1),
    }
}

/// Choose a canvas that does NOT require scaling the source frame.
/// Strategy: pick the variant (keep-width or keep-height) where canvas >= source on *both* axes.
pub fn canvas_no_downscale(src_w: u32, src_h: u32, ar: TargetAR) -> (u32, u32) {
    let (aw, ah) = ar_wh(ar);
    // candidate A: keep HEIGHT (canvas_h = src_h)
    let cand_a_w = ((src_h as f32) * (aw as f32) / (ah as f32)).round() as u32;
    let cand_a_h = src_h;

    // candidate B: keep WIDTH (canvas_w = src_w)
    let cand_b_w = src_w;
    let cand_b_h = ((src_w as f32) * (ah as f32) / (aw as f32)).round() as u32;

    let (a_w, a_h) = (round_even(cand_a_w.max(2)), round_even(cand_a_h.max(2)));
    let (b_w, b_h) = (round_even(cand_b_w.max(2)), round_even(cand_b_h.max(2)));

    // Pick the one that doesn't force downscale; if both qualify, take the smaller area.
    let a_ok = a_w >= src_w && a_h >= src_h;
    let b_ok = b_w >= src_w && b_h >= src_h;

    let (out_w, out_h) = match (a_ok, b_ok) {
        (true, true) => {
            let area_a = (a_w as u64) * (a_h as u64);
            let area_b = (b_w as u64) * (b_h as u64);
            if area_a <= area_b { (a_w, a_h) } else { (b_w, b_h) }
        }
        (true, false) => (a_w, a_h),
        (false, true) => (b_w, b_h),
        // In theory one of them must be ok; fallback to A.
        (false, false) => (a_w, a_h),
    };
    (out_w, out_h)
}

/// Build a vf that keeps full source, centers it, and pads to target canvas.
/// NOTE: No scaling! (video stays native pixels)
fn vf_fit_pad_no_scale(src_w: u32, src_h: u32, ar: TargetAR, pad_color: &str) -> String {
    let (out_w, out_h) = canvas_no_downscale(src_w, src_h, ar);
    // center the source inside the canvas
    let x = (out_w as i32 - src_w as i32) / 2;
    let y = (out_h as i32 - src_h as i32) / 2;
    format!("pad={}:{}:{}:{}:{}", out_w, out_h, x.max(0), y.max(0), pad_color)
}

/// Optional scaling to a "platform standard" *after* padding.
/// Uses a sharp scaler to avoid blur; only applied if you want fixed social sizes.
fn maybe_scale_to_standard(ar: TargetAR, want_standard: bool) -> Option<(u32, u32)> {
    if !want_standard { return None; }
    match ar {
        TargetAR::AR9x16 => Some((1080, 1920)),
        TargetAR::AR16x9 => Some((1920, 1080)),
        TargetAR::AR4x5  => Some((1080, 1350)),
        TargetAR::AR1x1  => Some((1080, 1080)),
    }
}

/// Convert format string to TargetAR enum
pub fn parse_target_ar(format: &str) -> anyhow::Result<TargetAR> {
    match format {
        "9:16" => Ok(TargetAR::AR9x16),
        "16:9" => Ok(TargetAR::AR16x9),
        "4:5" => Ok(TargetAR::AR4x5),
        "1:1" => Ok(TargetAR::AR1x1),
        _ => Err(anyhow::anyhow!("Unsupported aspect ratio format: {}. Supported formats: 9:16, 16:9, 4:5, 1:1", format))
    }
}

/// Build a unified video filter for fit+pad operations with high-quality scaling
/// This creates a single filtergraph that handles scaling and padding efficiently
/// Optimized for hardware encoders (VideoToolbox prefers NV12, others use yuv420p)
pub fn build_fitpad_filter(target_w: u32, target_h: u32, subtitle_path: Option<&str>) -> String {
    build_fitpad_filter_with_format(target_w, target_h, subtitle_path, HardwareEncoder::Software)
}

/// Build optimized video filter with encoder-specific format optimization
/// VideoToolbox: ends with NV12 to avoid hidden swscale conversions
/// Others: ends with yuv420p for broad compatibility
pub fn build_fitpad_filter_with_format(
    target_w: u32,
    target_h: u32,
    subtitle_path: Option<&str>,
    encoder: HardwareEncoder
) -> String {
    // Pre-calculate approximate capacity to avoid reallocations
    let has_subtitles = subtitle_path.is_some();
    let estimated_capacity = if has_subtitles {
        200 + subtitle_path.map(|p| p.len()).unwrap_or(0) // ~200 chars + subtitle path length
    } else {
        120 // Without subtitles, much shorter
    };

    let mut result = String::with_capacity(estimated_capacity);
    let mut first = true;

    // Helper to add filter with comma separator
    let mut add_filter = |filter: &str| {
        if !first {
            result.push(',');
        }
        result.push_str(filter);
        first = false;
    };

    // Start with high-quality chroma for subtitle rendering (if needed)
    if has_subtitles {
        add_filter("format=yuv444p");
    }

    // High-quality scaling with letterboxing - BEFORE subtitles for final resolution text
    add_filter(&format!(
        "scale={}:{}:flags=lanczos:force_original_aspect_ratio=decrease",
        target_w, target_h
    ));

    // Pad to exact target dimensions with black bars - BEFORE subtitles
    add_filter(&format!(
        "pad={}:{}:(ow-iw)/2:(oh-ih)/2:black",
        target_w, target_h
    ));

    // TEMPORARILY ENABLED: Test subtitle rendering with different fonts
    if let Some(subtitle_path) = subtitle_path {
        let escaped_path = escape_subtitle_path(subtitle_path);
        // Use absolute path to fonts directory
        let fonts_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/fonts");
        add_filter(&format!("subtitles={}:fontsdir={}", escaped_path, fonts_dir.display()));
    }

    // End with encoder-optimized format to avoid hidden conversions
    let final_format = match encoder {
        HardwareEncoder::VideoToolbox => "nv12",  // VideoToolbox optimization
        HardwareEncoder::Nvenc => "nv12",        // NVENC also prefers NV12
        HardwareEncoder::Software => "yuv420p",  // libx264 broad compatibility
    };
    add_filter(&format!("format={}", final_format));

    result
}

/// Determine the best audio codec and settings based on input analysis
/// Returns (codec, additional_args) tuple
pub fn determine_audio_codec(probe_result: Option<&crate::video::ProbeResult>) -> (&'static str, Vec<&'static str>) {
    let Some(probe) = probe_result else { return ("aac", vec!["-q:a", "2"]); };

    // No audio track
    if !probe.audio { return ("aac", vec!["-q:a", "2"]); }

    // If we couldn't detect the codec, re-encode with quality settings
    let Some(codec) = &probe.audio_codec else { return ("aac", vec!["-q:a", "2"]); };

    let codec_lower = codec.to_lowercase();

    // Check for codec patterns that should be re-encoded
    if codec_lower.starts_with("pcm_") || codec_lower.starts_with("adpcm_") {
        return ("aac", vec!["-q:a", "2"]); // Uncompressed/old formats - re-encode with VBR
    }

    // AAC bitrate-based decision
    if codec_lower == "aac" {
        if let Some(bitrate) = probe.audio_bitrate {
            // If AAC bitrate is ≤ 160kbps, copy it (good quality, small size)
            if bitrate <= 160_000 {
                return ("copy", vec![]);
            }
        } else {
            // Unknown bitrate AAC - copy to be safe
            return ("copy", vec![]);
        }
    }

    // Codec-specific decisions
    match codec_lower.as_str() {
        // Good modern codecs - copy these
        "mp3" | "opus" | "vorbis" => ("copy", vec![]),

        // Less common but still good codecs - copy
        "ac3" | "eac3" | "dts" | "mp2" => ("copy", vec![]),

        // Lossless formats - re-encode for better compatibility and smaller size
        "flac" | "alac" | "ape" | "wavpack" => ("aac", vec!["-q:a", "2"]),

        // Very old or unusual codecs - re-encode for compatibility
        "gsm" | "speex" => ("aac", vec!["-q:a", "2"]),

        // High-bitrate AAC or unknown codec - re-encode with VBR for quality parity
        _ => {
            eprintln!("Codec '{}' - re-encoding with VBR for optimal quality/size", codec);
            ("aac", vec!["-q:a", "2"])
        }
    }
}


/// Check if the current platform is macOS
pub fn is_macos() -> bool {
    cfg!(target_os = "macos")
}

/// Check if VideoToolbox H.264 encoder is available on macOS
/// This function tests if ffmpeg supports h264_videotoolbox encoder
pub async fn is_videotoolbox_available() -> bool {
    if !is_macos() {
        return false;
    }

    // Test if ffmpeg has h264_videotoolbox encoder available
    let result = Command::new("ffmpeg")
        .args(["-hide_banner", "-encoders"])
        .output();

    match result {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            stdout.contains("h264_videotoolbox")
        }
        Err(_) => false,
    }
}

/// Check if NVIDIA NVENC H.264 encoder is available
/// This function tests if ffmpeg supports h264_nvenc encoder
pub async fn is_nvenc_available() -> bool {
    // Test if ffmpeg has h264_nvenc encoder available
    let result = Command::new("ffmpeg")
        .args(["-hide_banner", "-encoders"])
        .output();

    match result {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            stdout.contains("h264_nvenc")
        }
        Err(_) => false,
    }
}

/// Determine the best available hardware encoder
pub async fn get_best_hardware_encoder() -> HardwareEncoder {
    if is_videotoolbox_available().await {
        HardwareEncoder::VideoToolbox
    } else if is_nvenc_available().await {
        HardwareEncoder::Nvenc
    } else {
        HardwareEncoder::Software
    }
}

#[derive(Debug, Clone, Copy)]
pub enum HardwareEncoder {
    VideoToolbox,
    Nvenc,
    Software,
}

/// Configure hardware encoder arguments based on available hardware (for TokioCommand)
/// Includes VideoToolbox optimizations and color metadata locking
pub fn configure_hardware_encoder_args(
    cmd: &mut TokioCommand,
    encoder: HardwareEncoder,
    crf: &str,
    gop_size_str: &str,
    preset: &str
) {
    match encoder {
        HardwareEncoder::VideoToolbox => {
            cmd.arg("-c:v").arg("h264_videotoolbox")
               .arg("-b:v").arg("0")                    // Use CRF mode (constant quality)
               .arg("-crf").arg(crf)                    // Quality setting (lower = better)
               .arg("-allow_sw").arg("1")               // Allow software fallback if needed
               .arg("-g").arg(gop_size_str)             // GOP size for seeking
               .arg("-pix_fmt").arg("nv12");            // VideoToolbox prefers NV12
        },
        HardwareEncoder::Nvenc => {
            // NVIDIA NVENC with enhanced settings for quality
            cmd.arg("-c:v").arg("h264_nvenc")
               .arg("-cq").arg(crf)                     // Constant quality mode (19 = good quality)
               .arg("-preset").arg("p5")                // High quality preset (p1=fast, p7=slow)
               .arg("-tune").arg("hq")                  // High quality tuning
               .arg("-rc").arg("vbr")                   // Variable bitrate for quality
               .arg("-g").arg(gop_size_str)             // GOP size for seeking
               .arg("-pix_fmt").arg("nv12");            // NVENC also prefers NV12
        },
        HardwareEncoder::Software => {
            cmd.arg("-c:v").arg("libx264")
               .arg("-preset").arg(preset)              // Configurable preset
               .arg("-crf").arg(crf)                    // Quality setting
               .arg("-g").arg(gop_size_str)             // GOP size for seeking
               .arg("-pix_fmt").arg("yuv420p");         // Broad compatibility
        }
    }

    // Add color metadata locking to prevent unnecessary conversions
    cmd.arg("-color_range").arg("tv")                   // TV range (16-235)
       .arg("-colorspace").arg("bt709")                 // Rec. 709 color space
       .arg("-color_primaries").arg("bt709")            // Rec. 709 primaries
       .arg("-color_trc").arg("bt709")                  // Rec. 709 transfer characteristics
       .arg("-benchmark")                               // Show overall timing
       .arg("-stats");                                  // Show per-filter timings
}

/// Get hardware encoder arguments as string slices (for std::process::Command)
/// Includes VideoToolbox optimizations, color metadata locking, and benchmark flags
pub fn get_hardware_encoder_args(
    encoder: HardwareEncoder,
    crf: &str,
    gop_size_str: &str,
    preset: &str
) -> Vec<String> {
    let mut args = match encoder {
        HardwareEncoder::VideoToolbox => vec![
            "-c:v".to_string(), "h264_videotoolbox".to_string(),
            "-b:v".to_string(), "0".to_string(),
            "-crf".to_string(), crf.to_string(),
            "-allow_sw".to_string(), "1".to_string(),
            "-g".to_string(), gop_size_str.to_string(),
            "-pix_fmt".to_string(), "nv12".to_string(),           // VideoToolbox prefers NV12
        ],
        HardwareEncoder::Nvenc => vec![
            "-c:v".to_string(), "h264_nvenc".to_string(),
            "-cq".to_string(), crf.to_string(),
            "-preset".to_string(), "p5".to_string(),
            "-tune".to_string(), "hq".to_string(),
            "-rc".to_string(), "vbr".to_string(),
            "-g".to_string(), gop_size_str.to_string(),
            "-pix_fmt".to_string(), "nv12".to_string(),           // NVENC also prefers NV12
        ],
        HardwareEncoder::Software => vec![
            "-c:v".to_string(), "libx264".to_string(),
            "-preset".to_string(), preset.to_string(),
            "-crf".to_string(), crf.to_string(),
            "-g".to_string(), gop_size_str.to_string(),
            "-pix_fmt".to_string(), "yuv420p".to_string(),        // Broad compatibility
        ],
    };

    // Add color metadata locking to prevent unnecessary conversions
    args.extend(vec![
        "-color_range".to_string(), "tv".to_string(),             // TV range (16-235)
        "-colorspace".to_string(), "bt709".to_string(),           // Rec. 709 color space
        "-color_primaries".to_string(), "bt709".to_string(),      // Rec. 709 primaries
        "-color_trc".to_string(), "bt709".to_string(),            // Rec. 709 transfer characteristics
    ]);

    // Add benchmark flags for performance monitoring
    args.extend(vec![
        "-benchmark".to_string(),                                 // Show overall timing
        "-stats".to_string(),                                     // Show per-filter timings
    ]);

    args
}


#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub video: String             // Path to the exported video file
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ExportParams {
    pub input: String,                    // Path to input video
    pub codec: String,                    // Output codec ("h264", "hevc", "prores")
    pub crf: Option<i32>,                 // Quality setting (lower = better quality, default: 18)
    pub preset: Option<String>,           // Encoding preset (default: "slow" for final, "medium" for preview)
    pub tune: Option<String>,             // Tuning (default: "film" for live-action, "animation" for synthetic)
    pub width: Option<i32>,               // Output width (exact dimensions, will letterbox to fit)
    pub height: Option<i32>,              // Output height (exact dimensions, will letterbox to fit)
    pub format: Option<String>,           // Aspect ratio format ("16:9", "9:16", "1:1", "4:5")
    pub use_standard_sizes: Option<bool>, // Whether to scale to standard social media sizes after padding
    pub out: String                       // Path for output video
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProbeResult {
    pub duration: Option<f64>,    // Length in seconds (None if unknown)
    pub width: Option<i32>,       // Video width in pixels (None if no video)
    pub height: Option<i32>,      // Video height in pixels (None if no video)
    pub fps: Option<f64>,         // Frames per second (None if no video/unknown)
    pub audio: bool,              // True if file has audio track
    pub video: bool,              // True if file has video track
    pub audio_codec: Option<String>, // Audio codec name (e.g., "aac", "mp3", "pcm_s16le")
    pub audio_bitrate: Option<i32>,  // Audio bitrate in bits/sec (e.g., 128000)
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ExtractThumbnailParams {
    pub input: String,            // Path to input video
    pub timestamp: Option<f64>,   // Time in seconds to extract frame from (default: 0.5)
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ThumbnailResult {
    pub image_data: String,       // Base64 encoded image data
    pub width: i32,               // Width of the thumbnail
    pub height: i32,              // Height of the thumbnail
}


/// Detect content type for tuning parameter
fn detect_content_type(probe_result: Option<&ProbeResult>) -> &'static str {
    // Simple heuristic: if frame rate is very consistent (30fps, 60fps), likely synthetic
    // Otherwise assume live-action content
    if let Some(probe) = probe_result {
        if let Some(fps) = probe.fps {
            // Perfect frame rates suggest synthetic content (games, animations, screen recordings)
            if (fps - 30.0).abs() < 0.01 || (fps - 60.0).abs() < 0.01 || (fps - 24.0).abs() < 0.01 {
                return "animation";
            }
        }
    }
    "film" // Default to film tuning for live-action
}

pub async fn export_video(id: &str, p: ExportParams, mut emit: impl FnMut(RpcEvent)) -> anyhow::Result<ExportResult> {
    let pr = probe(id, &p.input, &mut emit).await.ok();
    let crf = p.crf.unwrap_or(18).to_string(); // Default to CRF 18 for balanced quality/size
    let preset = p.preset.as_deref().unwrap_or("slow"); // Default to slow for final exports
    let tune = p.tune.as_deref().unwrap_or_else(|| detect_content_type(pr.as_ref()));
    let use_standard_sizes = p.use_standard_sizes.unwrap_or(false);

    // Determine the best available hardware encoder for H.264
    let hardware_encoder = if p.codec == "h264" {
        get_best_hardware_encoder().await
    } else {
        HardwareEncoder::Software
    };

    let mut cmd = TokioCommand::new("ffmpeg");
    cmd.arg("-y").arg("-i").arg(&p.input);

    // High-quality scaler settings
    cmd.arg("-sws_flags").arg("lanczos+accurate_rnd+full_chroma_int");

    // Build video filter for high-quality export
    let mut vf_parts = Vec::new();

    // Handle video scaling/letterboxing with new high-quality approach
    if let (Some(width), Some(height)) = (p.width, p.height) {
        // exact dimensions specified - use old behavior for backward compatibility
        let filter = format!("scale={}:{}:force_original_aspect_ratio=decrease,pad={}:{}:(ow-iw)/2:(oh-ih)/2:black",
                           width, height, width, height);
        vf_parts.push(filter);

        emit(RpcEvent::Log {
            id: id.into(),
            message: format!("Scaling to {}x{} with letterboxing", width, height)
        });
    } else if let Some(format) = &p.format {
        // New high-quality aspect ratio conversion
        if let Some(probe_result) = &pr {
            if let (Some(orig_width), Some(orig_height)) = (probe_result.width, probe_result.height) {
                let target_ar = parse_target_ar(format)?;
                let src_w = orig_width as u32;
                let src_h = orig_height as u32;

                // Build pad filter (no scaling)
                let pad_filter = vf_fit_pad_no_scale(src_w, src_h, target_ar, "black");
                vf_parts.push(pad_filter);

                // Optional scaling to standard social media sizes
                if let Some((std_w, std_h)) = maybe_scale_to_standard(target_ar, use_standard_sizes) {
                    let scale_filter = format!("scale={}:{}:flags=lanczos", std_w, std_h);
                    vf_parts.push(scale_filter);

                    emit(RpcEvent::Log {
                        id: id.into(),
                        message: format!("High-quality conversion to {} format ({}x{}) with padding and scaling to {}x{}",
                                       format, src_w, src_h, std_w, std_h)
                    });
                } else {
                    let (canvas_w, canvas_h) = canvas_no_downscale(src_w, src_h, target_ar);
                    emit(RpcEvent::Log {
                        id: id.into(),
                        message: format!("High-quality conversion to {} format ({}x{}) with padding to {}x{} - no scaling",
                                       format, src_w, src_h, canvas_w, canvas_h)
                    });
                }
            } else {
                emit(RpcEvent::Log {
                    id: id.into(),
                    message: "Warning: Could not determine video dimensions for format conversion".into()
                });
            }
        }
    }

    // Apply video filters if any
    if !vf_parts.is_empty() {
        cmd.arg("-vf").arg(vf_parts.join(","));
    }

    // High-quality encoding settings with cadence preservation
    cmd.arg("-fps_mode").arg("passthrough") // Preserve original frame timing (modern replacement for -vsync)
       .arg("-threads").arg("0");            // Use all available CPU cores

    // Calculate GOP size based on frame rate (2x fps for good seeking)
    let gop_size = if let Some(fps) = pr.as_ref().and_then(|p| p.fps) {
        (fps * 2.0).round() as u32
    } else {
        48 // Default for 24fps content
    };

        match p.codec.as_str() {
        "h264" => {
            let encoder_name = match hardware_encoder {
                HardwareEncoder::VideoToolbox => "VideoToolbox (GPU) + NV12 optimization",
                HardwareEncoder::Nvenc => "NVENC (GPU) + NV12 optimization",
                HardwareEncoder::Software => "libx264 (CPU)",
            };

            emit(RpcEvent::Log {
                id: id.into(),
                message: format!("Using {} for H.264 encoding", encoder_name)
            });

            configure_hardware_encoder_args(&mut cmd, hardware_encoder, &crf, &gop_size.to_string(), preset);

            // Add tune parameter for software encoding only (hardware encoders have built-in tuning)
            if matches!(hardware_encoder, HardwareEncoder::Software) {
                cmd.arg("-tune").arg(tune);
            }
        },
        "hevc" | "h265" => {
            cmd.arg("-c:v").arg("libx265")
               .arg("-preset").arg(preset)          // Configurable preset
               .arg("-tune").arg(tune)              // Content-aware tuning (if supported)
               .arg("-crf").arg(&crf)
               .arg("-g").arg(gop_size.to_string()) // GOP size for seeking
               .arg("-pix_fmt").arg("yuv420p");     // Broad compatibility
        },
        "prores" => {
            cmd.arg("-c:v").arg("prores_ks")
               .arg("-profile:v").arg("3");
        },
        other => {
            emit(RpcEvent::Log {
                id: id.into(),
                message: format!("Unknown codec '{}', using stream copy", other)
            });
            cmd.arg("-c:v").arg("copy");
        }
    }

    // Determine optimal audio codec and settings
    let (audio_codec, audio_args) = determine_audio_codec(pr.as_ref());

    // High-quality audio handling and metadata preservation
    cmd.arg("-c:a").arg(audio_codec);             // Optimal audio codec

    // Add explicit bitrate for re-encoded audio if not using copy
    if audio_codec != "copy" && audio_codec == "aac" && audio_args.is_empty() {
        cmd.arg("-b:a").arg("160k");              // Explicit AAC bitrate for quality
    }

    for arg in audio_args {
        cmd.arg(arg);                             // Additional audio encoding args
    }

    cmd
       .arg("-map_metadata").arg("0")              // Copy timing/metadata (colors, primaries, etc.)
       .arg("-map").arg("0:v:0")                   // Map first video stream
       .arg("-map").arg("0:a?")                    // Map audio if present (? makes it optional)
       .arg("-movflags").arg("+faststart")         // Fast start for web playback
       .arg(&p.out);

    let encoder_info = match hardware_encoder {
        HardwareEncoder::VideoToolbox => "h264_videotoolbox (GPU)",
        HardwareEncoder::Nvenc => "h264_nvenc (GPU)",
        HardwareEncoder::Software => "libx264 (CPU)",
    };
    emit(RpcEvent::Log {
        id: id.into(),
        message: format!("Starting export with CRF {}, encoder: {}, preset '{}', tune '{}', audio: {}",
                        crf, encoder_info, preset, tune, audio_codec)
    });

    let status = cmd.status().await?;
    if !status.success() {
        return Err(anyhow::anyhow!("ffmpeg export failed"));
    }

    emit(RpcEvent::Log {
        id: id.into(),
        message: "High-quality export completed successfully".into()
    });

    Ok(ExportResult { video: p.out })
}

// PROBE OPERATION - Analyze media file to get technical information
// This is typically the first operation run on any video/audio file
// Uses ffprobe (part of ffmpeg) to extract metadata without processing the file
pub async fn probe(id: &str, input: &str, mut emit: impl FnMut(RpcEvent)) -> anyhow::Result<ProbeResult> {
    emit(RpcEvent::Progress { id: id.into(), status: "Probing…".into(), progress: 0.05 });

    // Run ffprobe command to get file information as JSON
    let child = TokioCommand::new("ffprobe")
        .arg("-v").arg("error")              // Only show errors, suppress info messages
        .arg("-print_format").arg("json")    // Output as JSON for easy parsing
        .arg("-show_streams")                // Include information about audio/video streams
        .arg("-show_format")                 // Include information about file format
        .arg(input)                          // The file to analyze
        .stdout(std::process::Stdio::piped()) // Capture the output
        .spawn()?;

    // Wait for ffprobe to finish and get the output
    let out = child.wait_with_output().await?;
    if !out.status.success() {
        return Err(anyhow::anyhow!("ffprobe failed"));
    }

    // Parse the JSON output from ffprobe
    let v: serde_json::Value = serde_json::from_slice(&out.stdout)?;

    // Extract duration from format metadata (container level)
    let mut duration = v.get("format")
        .and_then(|f| f.get("duration"))
        .and_then(|d| d.as_str())
        .and_then(|s| s.parse::<f64>().ok());

    // Initialize stream-specific information
    let mut width = None;
    let mut height = None;
    let mut fps = None;
    let mut audio = false;
    let mut video = false;
    let mut audio_codec = None;
    let mut audio_bitrate = None;

    // Analyze each stream in the file
    if let Some(arr) = v.get("streams").and_then(|s| s.as_array()) {
        for st in arr {
            if let Some(codec_type) = st.get("codec_type").and_then(|x| x.as_str()) {
                match codec_type {
                    "video" => {
                        video = true;
                        // Extract video dimensions
                        width = st.get("width").and_then(|x| x.as_i64()).map(|x| x as i32);
                        height = st.get("height").and_then(|x| x.as_i64()).map(|x| x as i32);

                        // Extract frame rate (can be in fraction format)
                        if let Some(fr) = st.get("avg_frame_rate").and_then(|x| x.as_str()) {
                            fps = parse_fps(fr).or(fps);
                        }

                        // Fallback: try to get duration from video stream if format didn't have it
                        if duration.is_none() {
                            duration = st.get("duration")
                                .and_then(|x| x.as_str())
                                .and_then(|s| s.parse::<f64>().ok());
                        }
                    },
                    "audio" => {
                        audio = true;
                        // Extract audio codec name
                        audio_codec = st.get("codec_name").and_then(|x| x.as_str()).map(|s| s.to_string());
                        // Extract audio bitrate (can be in stream or format)
                        audio_bitrate = st.get("bit_rate")
                            .and_then(|x| x.as_str())
                            .and_then(|s| s.parse::<i32>().ok());
                    }
                    _ => {} // Ignore other stream types (subtitles, data, etc.)
                }
            }
        }
    }

    emit(RpcEvent::Progress { id: id.into(), status: "Probe complete".into(), progress: 1.0 });
    Ok(ProbeResult { duration, width, height, fps, audio, video, audio_codec, audio_bitrate })
}



// ffmpeg sometimes reports frame rates as fractions (e.g., "30000/1001" for 29.97 fps)
// This function handles both fraction and decimal formats
fn parse_fps(s: &str) -> Option<f64> {
    if s.contains('/') {
        // Handle fraction format like "30000/1001"
        let mut sp = s.split('/');
        let num: f64 = sp.next()?.parse().ok()?;  // Numerator
        let den: f64 = sp.next()?.parse().ok()?;  // Denominator
        if den == 0.0 { return None; }            // Avoid division by zero
        Some(num/den)                             // Calculate the actual fps
    } else {
        // Handle decimal format like "29.97"
        s.parse().ok()
    }
}
