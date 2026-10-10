use crate::rpc::RpcEvent;
use crate::whisper::{find_ffmpeg_binary, find_ffprobe_binary};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::Command;
use std::sync::{OnceLock, RwLock};
use tokio::io::AsyncBufReadExt;
use tokio::process::Command as TokioCommand;

static VIDEOTOOLBOX_AVAIL: OnceLock<bool> = OnceLock::new();
static NVENC_AVAIL: OnceLock<bool> = OnceLock::new();
static LIBASS_AVAIL: OnceLock<bool> = OnceLock::new();
static FFMPEG_WHISPER_AVAIL: OnceLock<bool> = OnceLock::new();
static FFMPEG_VERSION: OnceLock<Option<String>> = OnceLock::new();
static BEST_HW_ENCODER: OnceLock<HardwareEncoder> = OnceLock::new();

/// Thread-safe friendly bounded cache with FIFO eviction to prevent unbounded memory growth.
#[derive(Debug)]
pub struct BoundedCache<K, V> {
    capacity: usize,
    map: HashMap<K, V>,
    order: std::collections::VecDeque<K>,
}

impl<K: std::hash::Hash + Eq + Clone, V> BoundedCache<K, V> {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            map: HashMap::new(),
            order: std::collections::VecDeque::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: std::borrow::Borrow<Q>,
        Q: std::hash::Hash + Eq + ?Sized,
    {
        self.map.get(key)
    }

    pub fn insert(&mut self, key: K, value: V) {
        if let std::collections::hash_map::Entry::Occupied(mut e) = self.map.entry(key.clone()) {
            e.insert(value);
            return;
        }

        while self.map.len() >= self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.map.remove(&oldest);
            } else {
                break;
            }
        }

        self.order.push_back(key.clone());
        self.map.insert(key, value);
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.order.clear();
    }
}

type ProbeCacheEntry = (Option<std::time::SystemTime>, u64, ProbeResult);
type ProbeCache = BoundedCache<String, ProbeCacheEntry>;

type FrameCacheEntry = (Option<std::time::SystemTime>, u64, std::sync::Arc<str>);
type FrameCache = BoundedCache<String, FrameCacheEntry>;

const PROBE_CACHE_CAPACITY: usize = 64;
const FRAME_CACHE_CAPACITY: usize = 16;

// In-memory ProbeResult cache keyed by file path and (mtime, size)
static PROBE_CACHE: OnceLock<RwLock<ProbeCache>> = OnceLock::new();
static FIRST_FRAME_CACHE: OnceLock<RwLock<FrameCache>> = OnceLock::new();

fn get_probe_cache() -> &'static RwLock<ProbeCache> {
    PROBE_CACHE.get_or_init(|| RwLock::new(BoundedCache::new(PROBE_CACHE_CAPACITY)))
}

fn get_frame_cache() -> &'static RwLock<FrameCache> {
    FIRST_FRAME_CACHE.get_or_init(|| RwLock::new(BoundedCache::new(FRAME_CACHE_CAPACITY)))
}

static FFMPEG_SYNC_PATH: OnceLock<String> = OnceLock::new();

/// Get FFmpeg binary path synchronously (for use in sync functions)
pub fn get_ffmpeg_path_sync() -> String {
    FFMPEG_SYNC_PATH
        .get_or_init(|| {
            // Helper that respects architecture on macOS: if a candidate exists but
            // can't run on this machine (e.g. x86_64 binary on arm64 without Rosetta),
            // skip it and try the next candidate.
            fn usable(path: &std::path::Path) -> bool {
                path.exists() && crate::whisper::binary_runnable(path)
            }

            // Try to use cached path or default to "ffmpeg"
            // In sync context, we can't use the full async detection
            if let Ok(ffmpeg_path) = std::env::var("FFMPEG_PATH") {
                crate::debug_log!("DEBUG: FFMPEG_PATH set to {}", ffmpeg_path);
                let path = std::path::Path::new(&ffmpeg_path);
                if usable(path) {
                    return ffmpeg_path;
                } else {
                    crate::debug_log!(
                        "DEBUG: FFMPEG_PATH points to non-existent or unrunnable file, falling back to auto-detection"
                    );
                }
            }

            // PRIORITY 1: Bundled binary in rust/bin (Development / bundled script)
            let project_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            let bin_dir = project_root.join("bin");
            let bundled_rust = bin_dir.join(if cfg!(target_os = "windows") {
                "ffmpeg.exe"
            } else {
                "ffmpeg"
            });

            if usable(&bundled_rust) {
                crate::debug_log!(
                    "DEBUG: Found bundled ffmpeg at rust/bin: {:?}",
                    bundled_rust
                );
                return bundled_rust.to_string_lossy().to_string();
            }

            // PRIORITY 2: Bundled path next to the executable (Production app bundle)
            if let Ok(exe_path) = std::env::current_exe() {
                if let Some(exe_dir) = exe_path.parent() {
                    let bundled = exe_dir.join(if cfg!(target_os = "windows") {
                        "bin/ffmpeg.exe"
                    } else {
                        "bin/ffmpeg"
                    });
                    if usable(&bundled) {
                        crate::debug_log!("DEBUG: Found bundled ffmpeg at {:?}", bundled);
                        return bundled.to_string_lossy().to_string();
                    }
                }
            }

            // Try common paths
            let paths = vec![
                "/opt/homebrew/bin/ffmpeg",
                "/usr/local/bin/ffmpeg",
                "/usr/bin/ffmpeg",
                "ffmpeg",
            ];

            for path in paths {
                if let Ok(which_path) = which::which(path) {
                    if usable(&which_path) {
                        crate::debug_log!("DEBUG: Found ffmpeg at {}", which_path.display());
                        return which_path.to_string_lossy().to_string();
                    }
                }
                let p = std::path::Path::new(path);
                if p.is_file() && usable(p) {
                    crate::debug_log!("DEBUG: Found ffmpeg at {}", path);
                    return path.to_string();
                }
            }

            crate::debug_log!("DEBUG: No ffmpeg found, falling back to 'ffmpeg'");
            "ffmpeg".to_string() // Fallback
        })
        .clone()
}

/// Get the fonts directory path for subtitle rendering
/// Returns None if fonts directory cannot be found (libass will use system fonts)
fn _get_fonts_dir() -> Option<std::path::PathBuf> {
    // Priority 0: Explicit override — what a packaged app's launcher sets,
    // pointing at wherever it actually bundled the fonts, rather than this
    // process guessing a layout from its own exe path.
    if let Ok(path) = std::env::var("CAPSLAP_FONTS_DIR") {
        let p = std::path::PathBuf::from(path);
        if p.exists() && p.is_dir() {
            return Some(p);
        }
    }

    // Priority 1: Development environment
    let dev_fonts = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/fonts");
    if dev_fonts.exists() && dev_fonts.is_dir() {
        return Some(dev_fonts);
    }

    // Priority 2: Bundled fonts in app bundle (macOS)
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            // For macOS app bundle: Contents/MacOS/../Resources/fonts/
            if let Some(contents_dir) = exe_dir.parent() {
                let bundled_fonts = contents_dir.join("Resources/fonts");
                if bundled_fonts.exists() {
                    return Some(bundled_fonts);
                }
            }
            // Also try next to executable
            let exe_fonts = exe_dir.join("fonts");
            if exe_fonts.exists() {
                return Some(exe_fonts);
            }
        }
    }

    // No custom fonts directory found - libass will use system fonts
    None
}

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
    AR1x1,
}

/// Longest edge H.264 encoders accept. VideoToolbox refuses to open a session
/// beyond this, and the export then falls back to software — which on a canvas
/// that large crawls badly enough to look like a hang.
const MAX_ENCODE_EDGE: u32 = 4096;

/// Shrink a canvas, keeping its shape, until an encoder will take it.
///
/// Bites when a source with an aspect ratio far from the target is exported at
/// its own resolution: fitting a 4032x3024 phone video into 9:16 without
/// downscaling calls for a 4032x7168 canvas, most of it black bars, at fourteen
/// times the pixels of 1080p.
fn clamp_to_encodable(w: u32, h: u32) -> (u32, u32) {
    let longest = w.max(h);
    if longest <= MAX_ENCODE_EDGE {
        return (w, h);
    }
    let scale = MAX_ENCODE_EDGE as f64 / longest as f64;
    // Round down to even: rounding up could land back over the limit.
    let shrink = |v: u32| (((v as f64) * scale).round() as u32 & !1).max(2);
    (shrink(w), shrink(h))
}

/// Frame size an export lands on, given the requested output size.
///
/// A named size fixes the short edge and derives the other from the aspect
/// ratio; anything else keeps the source resolution where it can.
pub fn target_dimensions(
    output_size: Option<&str>,
    src_w: u32,
    src_h: u32,
    target_ar: TargetAR,
) -> (u32, u32) {
    let Some(size) = output_size else {
        let (w, h) = canvas_no_downscale(src_w, src_h, target_ar);
        return clamp_to_encodable(w, h);
    };

    let edge: f64 = match size {
        "1080p" => 1080.0,
        "720p" => 720.0,
        "4k" | "2160p" => 2160.0,
        _ => {
            let (w, h) = canvas_no_downscale(src_w, src_h, target_ar);
            return clamp_to_encodable(w, h);
        }
    };

    let (base_w, base_h) = ar_wh(target_ar);
    let ar = base_w as f64 / base_h as f64;
    let (w, h) = if base_w > base_h {
        (round_even((edge * ar).round() as u32), edge as u32)
    } else {
        (edge as u32, round_even((edge / ar).round() as u32))
    };
    clamp_to_encodable(w, h)
}

pub fn round_even(x: u32) -> u32 {
    // Round up to next even number for yuv420 compatibility
    (x + 1) & !1
}

pub fn ar_wh(ar: TargetAR) -> (u32, u32) {
    match ar {
        TargetAR::AR9x16 => (9, 16),
        TargetAR::AR16x9 => (16, 9),
        TargetAR::AR4x5 => (4, 5),
        TargetAR::AR1x1 => (1, 1),
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
            if area_a <= area_b {
                (a_w, a_h)
            } else {
                (b_w, b_h)
            }
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
    format!(
        "pad={}:{}:{}:{}:{}",
        out_w,
        out_h,
        x.max(0),
        y.max(0),
        pad_color
    )
}

/// Optional scaling to a "platform standard" *after* padding.
/// Uses a sharp scaler to avoid blur; only applied if you want fixed social sizes.
fn maybe_scale_to_standard(ar: TargetAR, want_standard: bool) -> Option<(u32, u32)> {
    if !want_standard {
        return None;
    }
    match ar {
        TargetAR::AR9x16 => Some((1080, 1920)),
        TargetAR::AR16x9 => Some((1920, 1080)),
        TargetAR::AR4x5 => Some((1080, 1350)),
        TargetAR::AR1x1 => Some((1080, 1080)),
    }
}

/// Convert format string to TargetAR enum
pub fn parse_target_ar(format: &str) -> anyhow::Result<TargetAR> {
    match format {
        "9:16" => Ok(TargetAR::AR9x16),
        "16:9" => Ok(TargetAR::AR16x9),
        "4:5" => Ok(TargetAR::AR4x5),
        "1:1" => Ok(TargetAR::AR1x1),
        _ => Err(anyhow::anyhow!(
            "Unsupported aspect ratio format: {}. Supported formats: 9:16, 16:9, 4:5, 1:1",
            format
        )),
    }
}

/// Build a unified video filter for fit+pad operations with high-quality scaling
/// This creates a single filtergraph that handles scaling and padding efficiently
/// Optimized for hardware encoders (VideoToolbox prefers NV12, others use yuv420p)
pub fn build_fitpad_filter(
    target_w: u32,
    target_h: u32,
    subtitle_path: Option<&str>,
    is_hdr: bool,
) -> String {
    build_fitpad_filter_with_format(
        target_w,
        target_h,
        subtitle_path,
        HardwareEncoder::Software,
        is_hdr,
    )
}

// / Build optimized video filter with encoder-specific format optimization
// / VideoToolbox: ends with NV12 to avoid hidden swscale conversions
// / Others: ends with yuv420p for broad compatibility
pub fn build_fitpad_filter_with_format(
    target_w: u32,
    target_h: u32,
    subtitle_path: Option<&str>,
    encoder: HardwareEncoder,
    is_hdr: bool,
) -> String {
    // Legacy support wrapper
    build_fitpad_filter_with_options(target_w, target_h, subtitle_path, encoder, "fit", is_hdr)
}

/// The `ass` filter clause for a subtitle file, pointed at our bundled fonts
/// when we can find them.
fn ass_filter(subtitle_path: &str) -> String {
    let escaped_path = escape_subtitle_path(subtitle_path);

    match _get_fonts_dir() {
        Some(fonts_dir) => {
            let fonts_path_str = fonts_dir.to_string_lossy().to_string();
            let escaped_fonts_path = escape_subtitle_path(&fonts_path_str);
            // escape_subtitle_path wraps in single quotes; strip them so we can
            // compose the ass='...':fontsdir='...' form.
            let clean_path = escaped_path.trim_matches('\'');
            let clean_fonts = escaped_fonts_path.trim_matches('\'');
            format!("ass='{}':fontsdir='{}'", clean_path, clean_fonts)
        }
        None => format!("ass={}", escaped_path),
    }
}

/// Draw the subtitles twice with no video in the graph — once on black, once on
/// white — and stack the results.
///
/// libass leaves the alpha channel alone, so rendering straight onto a
/// transparent canvas yields an invisible image. Rendering against two known
/// backgrounds instead lets the caller recover exact per-pixel coverage:
/// `white - black` is the background showing through, so `alpha = 255 - that`,
/// and the black pass is the colour already premultiplied by it.
pub fn build_caption_layer_filter(
    subtitle_path: &str,
    time_sec: f64,
    scale_height: Option<u32>,
) -> String {
    let ass = ass_filter(subtitle_path);
    let seek = seek_pts_filter(time_sec);
    let scale = match scale_height {
        Some(h) => format!(",scale=-2:{}:flags=bilinear", h.max(2)),
        None => String::new(),
    };
    // Deliberately no explicit `format` conversion here: forcing one (gbrp in
    // particular) makes swscale leave the trailing `width % 16` columns
    // untouched, which reads as fully opaque once the two passes are compared.
    format!(
        "[0]{seek},{ass}{scale}[k];[1]{seek},{ass}{scale}[w];[k][w]vstack=inputs=2[out]",
        seek = seek,
        ass = ass,
        scale = scale
    )
}

/// Stamp a frame with an absolute presentation time.
///
/// Input seeking (`-ss` before `-i`) rebases timestamps to zero, so libass would
/// look for a cue at t=0 no matter which moment we asked for — and draw nothing
/// unless a caption happens to start the video. Since preview renders emit a
/// single frame, pinning its PTS to the requested moment is both correct and
/// independent of how the demuxer chose to seek.
pub fn seek_pts_filter(time_sec: f64) -> String {
    format!("setpts={:.3}/TB", time_sec.max(0.0))
}

// / Extended filter builder with crop strategy support
pub fn build_fitpad_filter_with_options(
    target_w: u32,
    target_h: u32,
    subtitle_path: Option<&str>,
    encoder: HardwareEncoder,
    crop_strategy: &str,
    is_hdr: bool,
) -> String {
    let mut filters = Vec::new();

    // 1. Scaling Strategy
    if crop_strategy == "fill" {
        // "Fill" / "Center Crop" strategy:
        // Scale input so it COVERS the target area (keeping aspect ratio), then crop the center.
        // Formula: scale=w=TARGET_W:h=TARGET_H:force_original_aspect_ratio=increase,crop=TARGET_W:TARGET_H
        filters.push(format!(
            "scale=w={}:h={}:force_original_aspect_ratio=increase",
            target_w, target_h
        ));
        filters.push(format!(
            "crop={}:{}:(iw-ow)/2:(ih-oh)/2",
            target_w, target_h
        ));
    } else {
        // "Fit" / "Letterbox" strategy (default):
        // Scale input so it FITS within the target area (keeping aspect ratio), then pad with black bars.
        // Formula: scale=w=TARGET_W:h=TARGET_H:force_original_aspect_ratio=decrease,pad=TARGET_W:TARGET_H:(ow-iw)/2:(oh-ih)/2:black
        filters.push(format!(
            "scale={}:{}:flags=lanczos:force_original_aspect_ratio=decrease",
            target_w, target_h
        ));
        filters.push(format!(
            "pad={}:{}:(ow-iw)/2:(oh-ih)/2:black",
            target_w, target_h
        ));
    }

    // 2. Tonemapping (HDR -> SDR)
    // Only apply if input is actually HDR
    if is_hdr {
        // High-quality zscale tone mapping chain
        // 1. Linearize: transfer=linear, npl=100 (assume 100 nits target for SDR)
        // 2. Convert to GBR float for processing
        // 3. Map primaries to BT.709
        // 4. Tone map (hable or mobius)
        // 5. Convert back to BT.709 transfer/primaries/range
        // Note: We use a simplified robust chain that works well for most content
        filters.push("zscale=t=linear:npl=100,format=gbrpf32le,zscale=p=bt709,tonemap=tonemap=hable:desat=0,zscale=t=bt709:m=bt709:r=tv,format=yuv420p".to_string());
    }
    // For SDR content, we do NOTHING (preserving original colors)
    // The previous code was unconditionally applying tonemap which washed out SDR colors

    // 3. Subtitles
    if let Some(path) = subtitle_path {
        filters.push(ass_filter(path));
    }

    // 3. Encoder-specific format optimization
    match encoder {
        HardwareEncoder::VideoToolbox => {
            // VideoToolbox prefers NV12
            filters.push("format=nv12".to_string());
        }
        HardwareEncoder::Nvenc => {
            // NVENC also prefers NV12
            filters.push("format=nv12".to_string());
        }
        HardwareEncoder::Software => {
            // Software (x264) generic compatibility
            filters.push("format=yuv420p".to_string());
        }
    }

    filters.join(",")
}

/// Determine the best audio codec and settings based on input analysis
/// Returns (codec, additional_args) tuple
pub fn determine_audio_codec(
    probe_result: Option<&crate::video::ProbeResult>,
) -> (&'static str, Vec<&'static str>) {
    let Some(probe) = probe_result else {
        return ("aac", vec!["-q:a", "2"]);
    };

    // No audio track
    if !probe.audio {
        return ("aac", vec!["-q:a", "2"]);
    }

    // If we couldn't detect the codec, re-encode with quality settings
    let Some(codec) = &probe.audio_codec else {
        return ("aac", vec!["-q:a", "2"]);
    };

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
        _ => ("aac", vec!["-q:a", "2"]),
    }
}

/// Check if the current platform is macOS
pub fn is_macos() -> bool {
    cfg!(target_os = "macos")
}

/// Check if VideoToolbox H.264 encoder is available on macOS
/// This function tests if ffmpeg supports h264_videotoolbox encoder
pub async fn is_videotoolbox_available() -> bool {
    *VIDEOTOOLBOX_AVAIL.get_or_init(|| {
        if !is_macos() {
            return false;
        }

        let result = Command::new(get_ffmpeg_path_sync())
            .args(["-hide_banner", "-encoders"])
            .output();

        match result {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                stdout.contains("h264_videotoolbox")
            }
            Err(_) => false,
        }
    })
}

/// Check if NVIDIA NVENC H.264 encoder is available
/// This function tests if ffmpeg supports h264_nvenc encoder
pub async fn is_nvenc_available() -> bool {
    *NVENC_AVAIL.get_or_init(|| {
        let result = Command::new(get_ffmpeg_path_sync())
            .args(["-hide_banner", "-encoders"])
            .output();

        match result {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                stdout.contains("h264_nvenc")
            }
            Err(_) => false,
        }
    })
}

/// Check if FFmpeg has built-in Whisper support (requires FFmpeg 8.0+)
/// This function tests if ffmpeg supports the whisper audio filter
pub async fn is_ffmpeg_whisper_available() -> bool {
    *FFMPEG_WHISPER_AVAIL.get_or_init(|| {
        let result = Command::new(get_ffmpeg_path_sync())
            .args(["-hide_banner", "-filters"])
            .output();

        match result {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                stdout.contains("whisper")
            }
            Err(_) => false,
        }
    })
}

/// Check if whisper.cpp CLI is available (preferred method)
pub async fn is_whisper_cpp_available() -> bool {
    // Use the new cross-platform whisper binary detection from whisper.rs
    crate::whisper::find_whisper_binary().await.is_ok()
}

/// Check if FFmpeg has libass support for subtitles
pub async fn is_libass_available() -> bool {
    *LIBASS_AVAIL.get_or_init(|| {
        let result = Command::new(get_ffmpeg_path_sync())
            .args(["-hide_banner", "-filters"])
            .output();

        match result {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                stdout.contains(" ass ")
            }
            Err(_) => false,
        }
    })
}

/// Get FFmpeg version to check if it's 8.0+ for Whisper support
pub async fn get_ffmpeg_version() -> Option<String> {
    FFMPEG_VERSION
        .get_or_init(|| {
            let result = Command::new(get_ffmpeg_path_sync())
                .args(["-version"])
                .output();

            match result {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    stdout.lines().next().and_then(|line| {
                        let words: Vec<&str> = line.split_whitespace().collect();
                        if let Some(pos) = words.iter().position(|&word| word == "version") {
                            words.get(pos + 1).map(|v| v.to_string())
                        } else {
                            None
                        }
                    })
                }
                Err(_) => None,
            }
        })
        .clone()
}

/// Determine the best available hardware encoder
pub async fn get_best_hardware_encoder() -> HardwareEncoder {
    *BEST_HW_ENCODER.get_or_init(|| {
        if !is_macos() {
            if is_nvenc_sync() {
                return HardwareEncoder::Nvenc;
            }
            return HardwareEncoder::Software;
        }

        let result = Command::new(get_ffmpeg_path_sync())
            .args(["-hide_banner", "-encoders"])
            .output();

        if let Ok(output) = result {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.contains("h264_videotoolbox") {
                return HardwareEncoder::VideoToolbox;
            }
        }
        HardwareEncoder::Software
    })
}

fn is_nvenc_sync() -> bool {
    let result = Command::new(get_ffmpeg_path_sync())
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HardwareEncoder {
    VideoToolbox,
    Nvenc,
    Software,
}

/// How much of the job a VideoToolbox encode also hands to the GPU.
///
/// `Pipeline` decodes into VideoToolbox frames, scales them with `scale_vt` and
/// only then brings them to the CPU, because the filters after that (`ass` for
/// the captions, `crop`, `pad`, `overlay`) exist only for system memory.
/// `Decode` leaves the filters as they were and just decodes on the GPU, and
/// `Off` is the plain path every machine can run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accel {
    Pipeline,
    Decode,
    Off,
}

/// The attempts to make, best first. A VideoToolbox build can lack `scale_vt`,
/// and 10-bit or odd sources can refuse the GPU path, so every attempt but the
/// last is allowed to fail at start-up and hand over to the next one.
/// HDR keeps its CPU tone-mapping chain, so it goes straight to the encoder-only path.
pub fn encode_attempts(
    encoder: HardwareEncoder,
    pipeline_available: bool,
    is_hdr: bool,
) -> Vec<(HardwareEncoder, Accel)> {
    let mut out = Vec::new();
    match encoder {
        HardwareEncoder::VideoToolbox => {
            if !is_hdr {
                if pipeline_available {
                    out.push((encoder, Accel::Pipeline));
                }
                out.push((encoder, Accel::Decode));
            }
            out.push((encoder, Accel::Off));
        }
        HardwareEncoder::Nvenc => out.push((encoder, Accel::Off)),
        HardwareEncoder::Software => {}
    }
    out.push((HardwareEncoder::Software, Accel::Off));
    out
}

/// Input options that put decoding on VideoToolbox. With `keep_on_gpu` the
/// frames stay there for `scale_vt`; without, ffmpeg copies them back to system
/// memory itself, so ordinary CPU filters work unchanged.
pub fn vt_decode_args(keep_on_gpu: bool) -> Vec<&'static str> {
    if keep_on_gpu {
        vec![
            "-hwaccel",
            "videotoolbox",
            "-hwaccel_output_format",
            "videotoolbox_vld",
        ]
    } else {
        vec!["-hwaccel", "videotoolbox"]
    }
}

/// Is the GPU pipeline there to try: a macOS ffmpeg that knows both the
/// VideoToolbox decoder and the `scale_vt` filter. Checked once.
pub fn vt_pipeline_available() -> bool {
    static AVAIL: OnceLock<bool> = OnceLock::new();
    *AVAIL.get_or_init(|| {
        if !is_macos() {
            return false;
        }
        let ffmpeg = get_ffmpeg_path_sync();
        let run = |arg: &str, needle: &str| {
            Command::new(&ffmpeg)
                .args(["-hide_banner", arg])
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).contains(needle))
                .unwrap_or(false)
        };
        run("-hwaccels", "videotoolbox") && run("-filters", "scale_vt")
    })
}

/// The size to scale a `src_w`x`src_h` picture to before the crop (`fill`) or
/// pad (`fit`) that finishes the job, so the GPU does all the resampling and
/// the CPU only cuts or pads. Both sides even, as H.264 needs: a fill never
/// comes out smaller than the target, a fit never larger.
pub fn gpu_scale_dims(
    src_w: u32,
    src_h: u32,
    target_w: u32,
    target_h: u32,
    fill: bool,
) -> (u32, u32) {
    let (sw, sh) = (src_w.max(2) as f64, src_h.max(2) as f64);
    let (tw, th) = (target_w as f64, target_h as f64);
    if fill {
        let s = (tw / sw).max(th / sh);
        let up = |v: f64| round_even((v.ceil() as u32).max(2));
        (up(sw * s).max(target_w), up(sh * s).max(target_h))
    } else {
        let s = (tw / sw).min(th / sh);
        let down = |v: f64| ((v.round() as u32).max(2)) & !1;
        (down(sw * s).min(target_w), down(sh * s).min(target_h))
    }
}

/// `build_fitpad_filter_with_options` for a VideoToolbox frame: `scale_vt` does
/// the resampling on the GPU, `hwdownload` brings the result to the CPU, and the
/// crop or pad, the captions and the pixel format follow as before. SDR only.
pub fn build_gpu_fitpad_filter(
    src_w: u32,
    src_h: u32,
    target_w: u32,
    target_h: u32,
    subtitle_path: Option<&str>,
    crop_strategy: &str,
) -> String {
    let fill = crop_strategy == "fill";
    let (w, h) = gpu_scale_dims(src_w, src_h, target_w, target_h, fill);
    let mut filters = vec![format!("scale_vt=w={w}:h={h}"), "hwdownload".to_string()];
    // The decoder's own pixel format is NV12 for 8-bit video; anything else
    // fails here at start-up and the next attempt takes over.
    filters.push("format=nv12".to_string());
    if fill {
        filters.push(format!("crop={target_w}:{target_h}:(iw-ow)/2:(ih-oh)/2"));
    } else {
        filters.push(format!(
            "pad={target_w}:{target_h}:(ow-iw)/2:(oh-ih)/2:black"
        ));
    }
    if let Some(path) = subtitle_path {
        filters.push(ass_filter(path));
    }
    filters.push("format=nv12".to_string());
    filters.join(",")
}

/// A proof copy's size: the short side as asked, the other rounded to even.
pub fn proof_dims(target_w: u32, target_h: u32, short_side: u32) -> (u32, u32) {
    if target_w < target_h {
        (
            short_side,
            round_even((short_side as f64 * target_h as f64 / target_w as f64).round() as u32),
        )
    } else {
        (
            round_even((short_side as f64 * target_w as f64 / target_h as f64).round() as u32),
            short_side,
        )
    }
}

/// The proof scale done on the GPU: the captioned frame goes up to VideoToolbox,
/// is scaled there and comes back. Needs `-init_hw_device videotoolbox=vt
/// -filter_hw_device vt` on the command line.
pub fn gpu_proof_scale_filter(target_w: u32, target_h: u32, short_side: u32) -> String {
    let (w, h) = proof_dims(target_w, target_h, short_side);
    format!("hwupload,scale_vt=w={w}:h={h},hwdownload,format=nv12")
}

/// The command-line options `gpu_proof_scale_filter` needs.
pub const VT_FILTER_DEVICE_ARGS: [&str; 4] = [
    "-init_hw_device",
    "videotoolbox=vt",
    "-filter_hw_device",
    "vt",
];

/// Convert CRF value (0-51) to VideoToolbox bitrate string
/// CRF scale: 0=lossless, 18=visually lossless, 23=default, 51=worst
/// we map these to reasonable fixed bitrates for hardware encoding
fn crf_to_bitrate(crf: &str) -> String {
    if let Ok(crf_value) = crf.parse::<i32>() {
        if crf_value < 20 {
            "12M".to_string() // High quality
        } else if crf_value < 24 {
            "8M".to_string() // Standard quality (default)
        } else if crf_value < 28 {
            "6M".to_string() // Medium quality
        } else {
            "4M".to_string() // Low quality
        }
    } else {
        "8M".to_string() // Fallback to standard quality
    }
}

/// Configure hardware encoder arguments based on available hardware (for TokioCommand)
/// Includes VideoToolbox optimizations and color metadata locking
pub fn configure_hardware_encoder_args(
    cmd: &mut TokioCommand,
    encoder: HardwareEncoder,
    crf: &str,
    gop_size_str: &str,
    preset: &str,
) {
    match encoder {
        HardwareEncoder::VideoToolbox => {
            // VideoToolbox H.264 encoder for macOS hardware acceleration
            // Note: VideoToolbox in this ffmpeg build doesn't support -q:v
            // We use -b:v (bitrate) instead as requested/verified
            let bitrate = crf_to_bitrate(crf);
            cmd.arg("-c:v")
                .arg("h264_videotoolbox")
                .arg("-b:v")
                .arg(&bitrate) // Bitrate setting (e.g. "8M")
                .arg("-allow_sw")
                .arg("1") // Allow software fallback if hardware fails
                .arg("-g")
                .arg(gop_size_str) // GOP size for seeking
                .arg("-pix_fmt")
                .arg("nv12"); // VideoToolbox prefers NV12
        }
        HardwareEncoder::Nvenc => {
            // NVIDIA NVENC with enhanced settings for quality
            cmd.arg("-c:v")
                .arg("h264_nvenc")
                .arg("-cq")
                .arg(crf) // Constant quality mode (19 = good quality)
                .arg("-preset")
                .arg("p5") // High quality preset (p1=fast, p7=slow)
                .arg("-tune")
                .arg("hq") // High quality tuning
                .arg("-rc")
                .arg("vbr") // Variable bitrate for quality
                .arg("-g")
                .arg(gop_size_str) // GOP size for seeking
                .arg("-pix_fmt")
                .arg("nv12"); // NVENC also prefers NV12
        }
        HardwareEncoder::Software => {
            cmd.arg("-c:v")
                .arg("libx264")
                .arg("-preset")
                .arg(preset) // Configurable preset
                .arg("-crf")
                .arg(crf) // Quality setting
                .arg("-g")
                .arg(gop_size_str) // GOP size for seeking
                .arg("-pix_fmt")
                .arg("yuv420p"); // Broad compatibility
        }
    }

    // Add color metadata locking to prevent unnecessary conversions
    cmd.arg("-color_range")
        .arg("tv") // TV range (16-235)
        .arg("-colorspace")
        .arg("bt709") // Rec. 709 color space
        .arg("-color_primaries")
        .arg("bt709") // Rec. 709 primaries
        .arg("-color_trc")
        .arg("bt709") // Rec. 709 transfer characteristics
        .arg("-benchmark") // Show overall timing
        .arg("-stats"); // Show per-filter timings
}

/// Get hardware encoder arguments as string slices (for std::process::Command)
/// Includes VideoToolbox optimizations, color metadata locking, and benchmark flags
pub fn get_hardware_encoder_args(
    encoder: HardwareEncoder,
    crf: &str,
    gop_size_str: &str,
    preset: &str,
) -> Vec<String> {
    let mut args = match encoder {
        HardwareEncoder::VideoToolbox => {
            let bitrate = crf_to_bitrate(crf);
            vec![
                "-c:v".to_string(),
                "h264_videotoolbox".to_string(),
                "-b:v".to_string(),
                bitrate, // Bitrate setting
                "-allow_sw".to_string(),
                "1".to_string(),
                "-g".to_string(),
                gop_size_str.to_string(),
                "-pix_fmt".to_string(),
                "nv12".to_string(), // VideoToolbox prefers NV12
            ]
        }
        HardwareEncoder::Nvenc => vec![
            "-c:v".to_string(),
            "h264_nvenc".to_string(),
            "-cq".to_string(),
            crf.to_string(),
            "-preset".to_string(),
            "p5".to_string(),
            "-tune".to_string(),
            "hq".to_string(),
            "-rc".to_string(),
            "vbr".to_string(),
            "-g".to_string(),
            gop_size_str.to_string(),
            "-pix_fmt".to_string(),
            "nv12".to_string(), // NVENC also prefers NV12
        ],
        HardwareEncoder::Software => vec![
            "-c:v".to_string(),
            "libx264".to_string(),
            "-preset".to_string(),
            preset.to_string(),
            "-crf".to_string(),
            crf.to_string(),
            "-g".to_string(),
            gop_size_str.to_string(),
            "-pix_fmt".to_string(),
            "yuv420p".to_string(), // Broad compatibility
        ],
    };

    // Add color metadata locking to prevent unnecessary conversions
    args.extend(vec![
        "-color_range".to_string(),
        "tv".to_string(), // TV range (16-235)
        "-colorspace".to_string(),
        "bt709".to_string(), // Rec. 709 color space
        "-color_primaries".to_string(),
        "bt709".to_string(), // Rec. 709 primaries
        "-color_trc".to_string(),
        "bt709".to_string(), // Rec. 709 transfer characteristics
    ]);

    // Add benchmark flags for performance monitoring
    args.extend(vec![
        "-benchmark".to_string(), // Show overall timing
        "-stats".to_string(),     // Show per-filter timings
    ]);

    args
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub video: String, // Path to the exported video file
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ExportParams {
    pub input: String,                    // Path to input video
    pub codec: String,                    // Output codec ("h264", "hevc", "prores")
    pub crf: Option<i32>,                 // Quality setting (lower = better quality, default: 18)
    pub preset: Option<String>, // Encoding preset (default: "slow" for final, "medium" for preview)
    pub tune: Option<String>, // Tuning (default: "film" for live-action, "animation" for synthetic)
    pub width: Option<i32>,   // Output width (exact dimensions, will letterbox to fit)
    pub height: Option<i32>,  // Output height (exact dimensions, will letterbox to fit)
    pub format: Option<String>, // Aspect ratio format ("16:9", "9:16", "1:1", "4:5")
    pub use_standard_sizes: Option<bool>, // Whether to scale to standard social media sizes after padding
    pub out: String,                      // Path for output video
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProbeResult {
    pub duration: Option<f64>,           // Length in seconds (None if unknown)
    pub width: Option<i32>,              // Video width in pixels (None if no video)
    pub height: Option<i32>,             // Video height in pixels (None if no video)
    pub fps: Option<f64>,                // Frames per second (None if no video/unknown)
    pub audio: bool,                     // True if file has audio track
    pub video: bool,                     // True if file has video track
    pub audio_codec: Option<String>,     // Audio codec name (e.g., "aac", "mp3", "pcm_s16le")
    pub audio_bitrate: Option<i32>,      // Audio bitrate in bits/sec (e.g., 128000)
    pub color_space: Option<String>,     // Color space (e.g. "bt2020nc")
    pub color_transfer: Option<String>, // Color transfer characteristics (e.g. "smpte2084" for PQ, "arib-std-b67" for HLG)
    pub color_primaries: Option<String>, // Color primaries (e.g. "bt2020")
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ExtractThumbnailParams {
    pub input: String,          // Path to input video
    pub timestamp: Option<f64>, // Time in seconds to extract frame from (default: 0.5)
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ThumbnailResult {
    pub image_data: String, // Base64 encoded image data
    pub width: i32,         // Width of the thumbnail
    pub height: i32,        // Height of the thumbnail
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

pub async fn export_video(
    id: &str,
    p: ExportParams,
    mut emit: impl FnMut(RpcEvent),
) -> anyhow::Result<ExportResult> {
    let pr = probe(id, &p.input, &mut emit).await.ok();
    let crf = p.crf.unwrap_or(18).to_string(); // Default to CRF 18 for balanced quality/size
    let preset = p.preset.as_deref().unwrap_or("slow"); // Default to slow for final exports
    let tune = p
        .tune
        .as_deref()
        .unwrap_or_else(|| detect_content_type(pr.as_ref()));
    let use_standard_sizes = p.use_standard_sizes.unwrap_or(false);

    // Verify libass availability if subtitles are requested
    // Note: The main multi-format export logic (which handles burning subtitles)
    // is in captions.rs, not here. We added the check there.

    // Determine the best available hardware encoder for H.264
    let hardware_encoder = if p.codec == "h264" {
        get_best_hardware_encoder().await
    } else {
        HardwareEncoder::Software
    };

    let ffmpeg_path = find_ffmpeg_binary()
        .await
        .map_err(|e| anyhow::anyhow!("FFmpeg not found: {}", e))?;
    let mut cmd = TokioCommand::new(ffmpeg_path);
    cmd.kill_on_drop(true);
    cmd.arg("-y").arg("-i").arg(&p.input);

    // High-quality scaler settings
    cmd.arg("-sws_flags")
        .arg("lanczos+accurate_rnd+full_chroma_int");

    // Build video filter for high-quality export
    let mut vf_parts = Vec::new();

    // Check for explicit width/height first (legacy/exact mode)
    if let (Some(width), Some(height)) = (p.width, p.height) {
        // exact dimensions specified - use old behavior for backward compatibility
        let filter = format!(
            "scale={}:{}:force_original_aspect_ratio=decrease,pad={}:{}:(ow-iw)/2:(oh-ih)/2:black",
            width, height, width, height
        );
        vf_parts.push(filter);

        emit(RpcEvent::Log {
            id: id.into(),
            message: format!("Scaling to {}x{} with letterboxing", width, height),
        });
    } else if let Some(format) = &p.format {
        // New high-quality aspect ratio conversion
        if let Some(probe_result) = &pr {
            if let (Some(orig_width), Some(orig_height)) = (probe_result.width, probe_result.height)
            {
                let target_ar = parse_target_ar(format)?;
                let src_w = orig_width as u32;
                let src_h = orig_height as u32;

                // Build pad filter (no scaling)
                let pad_filter = vf_fit_pad_no_scale(src_w, src_h, target_ar, "black");
                vf_parts.push(pad_filter);

                // Optional scaling to standard social media sizes
                if let Some((std_w, std_h)) = maybe_scale_to_standard(target_ar, use_standard_sizes)
                {
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
                    message: "Warning: Could not determine video dimensions for format conversion"
                        .into(),
                });
            }
        }
    }

    // Add tone mapping if HDR detected (after scaling/padding)
    if let Some(probe_result) = &pr {
        if is_hdr(probe_result) {
            emit(RpcEvent::Log {
                id: id.into(),
                message: "HDR content detected, applying zscale tone mapping...".into(),
            });
            // High-quality zscale tone mapping chain
            vf_parts.push("zscale=t=linear:npl=100,format=gbrpf32le,zscale=p=bt709,tonemap=tonemap=hable:desat=0,zscale=t=bt709:m=bt709:r=tv,format=yuv420p".to_string());
        }
    }

    // Apply video filters if any
    if !vf_parts.is_empty() {
        cmd.arg("-vf").arg(vf_parts.join(","));
    }

    // High-quality encoding settings with cadence preservation
    cmd.arg("-fps_mode")
        .arg("passthrough") // Preserve original frame timing (modern replacement for -vsync)
        .arg("-threads")
        .arg("0"); // Use all available CPU cores

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
                message: format!("Using {} for H.264 encoding", encoder_name),
            });

            configure_hardware_encoder_args(
                &mut cmd,
                hardware_encoder,
                &crf,
                &gop_size.to_string(),
                preset,
            );

            // Add tune parameter for software encoding only (hardware encoders have built-in tuning)
            if matches!(hardware_encoder, HardwareEncoder::Software) {
                cmd.arg("-tune").arg(tune);
            }
        }
        "hevc" | "h265" => {
            cmd.arg("-c:v")
                .arg("libx265")
                .arg("-preset")
                .arg(preset) // Configurable preset
                .arg("-tune")
                .arg(tune) // Content-aware tuning (if supported)
                .arg("-crf")
                .arg(&crf)
                .arg("-g")
                .arg(gop_size.to_string()) // GOP size for seeking
                .arg("-pix_fmt")
                .arg("yuv420p"); // Broad compatibility
        }
        "prores" => {
            cmd.arg("-c:v").arg("prores_ks").arg("-profile:v").arg("3");
        }
        other => {
            emit(RpcEvent::Log {
                id: id.into(),
                message: format!("Unknown codec '{}', using stream copy", other),
            });
            cmd.arg("-c:v").arg("copy");
        }
    }

    // Determine optimal audio codec and settings
    let (audio_codec, audio_args) = determine_audio_codec(pr.as_ref());

    // High-quality audio handling and metadata preservation
    cmd.arg("-c:a").arg(audio_codec); // Optimal audio codec

    // Add explicit bitrate for re-encoded audio if not using copy
    if audio_codec != "copy" && audio_codec == "aac" && audio_args.is_empty() {
        cmd.arg("-b:a").arg("160k"); // Explicit AAC bitrate for quality
    }

    for arg in &audio_args {
        cmd.arg(arg); // Additional audio encoding args
    }

    cmd.arg("-map_metadata")
        .arg("0") // Copy timing/metadata (colors, primaries, etc.)
        .arg("-map")
        .arg("0:v:0") // Map first video stream
        .arg("-map")
        .arg("0:a?") // Map audio if present (? makes it optional)
        .arg("-movflags")
        .arg("+faststart") // Fast start for web playback
        .arg(&p.out);

    let encoder_info = match hardware_encoder {
        HardwareEncoder::VideoToolbox => "h264_videotoolbox (GPU)",
        HardwareEncoder::Nvenc => "h264_nvenc (GPU)",
        HardwareEncoder::Software => "libx264 (CPU)",
    };
    // Add progress flag to output to stdout
    cmd.arg("-progress").arg("pipe:1");

    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(if crate::debug_enabled() {
        std::process::Stdio::inherit()
    } else {
        std::process::Stdio::null()
    });

    emit(RpcEvent::Log {
        id: id.into(),
        message: format!(
            "Starting export with CRF {}, encoder: {}, preset '{}', tune '{}', audio: {}",
            crf, encoder_info, preset, tune, audio_codec
        ),
    });

    // Calculate total duration in microseconds for progress
    let duration_us = pr
        .as_ref()
        .and_then(|p| p.duration)
        .map(|s| (s * 1_000_000.0) as u64);

    emit(RpcEvent::Log {
        id: id.into(),
        message: format!("Duration from probe: {:?} us", duration_us),
    });

    emit(RpcEvent::Progress {
        id: id.to_string(),
        status: "Starting export...".to_string(),
        progress: 0.0,
    });

    let mut child = cmd.spawn()?;

    // Handle stdout for progress
    if let Some(stdout) = child.stdout.take() {
        let reader = tokio::io::BufReader::new(stdout);
        let mut lines = reader.lines();

        while let Ok(Some(line)) = lines.next_line().await {
            // Debug log for checking what we get (temporary, can be verbose)
            // emit(RpcEvent::Log { id: id.into(), message: format!("FFmpeg stdout: {}", line) });

            if let Some(stripped) = line.strip_prefix("out_time_us=") {
                if let Ok(us) = stripped.trim().parse::<u64>() {
                    let progress = if let Some(total) = duration_us {
                        if total > 0 {
                            (us as f64 / total as f64).min(0.99) as f32
                        } else {
                            0.0
                        }
                    } else {
                        0.0 // Todo: maybe fake progress or indeterminate state?
                    };

                    emit(RpcEvent::Progress {
                        id: id.to_string(),
                        status: format!("Exporting... ({:.0}%)", progress * 100.0),
                        progress,
                    });
                }
            }
        }
    }

    let status = child.wait().await?;

    // If hardware encoder failed, try falling back to software encoding
    if !status.success() && !matches!(hardware_encoder, HardwareEncoder::Software) {
        emit(RpcEvent::Log {
            id: id.into(),
            message: format!(
                "Hardware encoder {} failed, falling back to software encoding (libx264)",
                encoder_info
            ),
        });

        // Rebuild command with software encoder
        let mut fallback_cmd = TokioCommand::new(find_ffmpeg_binary().await?);
        fallback_cmd.kill_on_drop(true);
        fallback_cmd.arg("-y").arg("-i").arg(&p.input);
        fallback_cmd
            .arg("-sws_flags")
            .arg("lanczos+accurate_rnd+full_chroma_int");

        // Reapply video filters
        if !vf_parts.is_empty() {
            fallback_cmd.arg("-vf").arg(vf_parts.join(","));
        }

        fallback_cmd
            .arg("-fps_mode")
            .arg("passthrough")
            .arg("-threads")
            .arg("0");

        // Use software encoder
        configure_hardware_encoder_args(
            &mut fallback_cmd,
            HardwareEncoder::Software,
            &crf,
            &gop_size.to_string(),
            preset,
        );
        fallback_cmd.arg("-tune").arg(tune);

        // Reapply audio settings
        fallback_cmd.arg("-c:a").arg(audio_codec);
        if audio_codec != "copy" && audio_codec == "aac" && audio_args.is_empty() {
            fallback_cmd.arg("-b:a").arg("160k");
        }
        for arg in &audio_args {
            fallback_cmd.arg(arg);
        }

        fallback_cmd
            .arg("-map_metadata")
            .arg("0")
            .arg("-map")
            .arg("0:v:0")
            .arg("-map")
            .arg("0:a?")
            .arg("-movflags")
            .arg("+faststart")
            .arg("-progress")
            .arg("pipe:1") // Enable progress for fallback too
            .arg(&p.out);

        fallback_cmd
            .stdout(std::process::Stdio::piped())
            .stderr(if crate::debug_enabled() {
                std::process::Stdio::inherit()
            } else {
                std::process::Stdio::null()
            });

        emit(RpcEvent::Log {
            id: id.into(),
            message: "Retrying with software encoder (libx264)...".into(),
        });

        let mut child_fallback = fallback_cmd.spawn()?;
        if let Some(stdout) = child_fallback.stdout.take() {
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

                        emit(RpcEvent::Progress {
                            id: id.to_string(),
                            status: format!("Exporting (software)... ({:.0}%)", progress * 100.0),
                            progress,
                        });
                    }
                }
            }
        }

        let fallback_status = child_fallback.wait().await?;
        if !fallback_status.success() {
            return Err(anyhow::anyhow!(
                "ffmpeg export failed with both hardware and software encoders"
            ));
        }
    } else if !status.success() {
        return Err(anyhow::anyhow!("ffmpeg export failed"));
    }

    // 100% completion
    emit(RpcEvent::Progress {
        id: id.into(),
        status: "Done".into(),
        progress: 1.0,
    });

    emit(RpcEvent::Log {
        id: id.into(),
        message: "High-quality export completed successfully".into(),
    });

    Ok(ExportResult { video: p.out })
}

// PROBE OPERATION - Analyze media file to get technical information
// This is typically the first operation run on any video/audio file
// Uses bundled ffprobe to extract metadata without processing the file
pub async fn probe(
    id: &str,
    input: &str,
    mut emit: impl FnMut(RpcEvent),
) -> anyhow::Result<ProbeResult> {
    // Check file metadata for caching
    let meta = std::fs::metadata(input).ok();
    let mtime = meta.as_ref().and_then(|m| m.modified().ok());
    let file_size = meta.as_ref().map(|m| m.len()).unwrap_or(0);

    if let Ok(cache) = get_probe_cache().read() {
        if let Some((cached_mtime, cached_size, cached_result)) = cache.get(input) {
            if *cached_mtime == mtime && *cached_size == file_size {
                emit(RpcEvent::Progress {
                    id: id.into(),
                    status: "Probed".into(),
                    progress: 0.05,
                });
                return Ok(cached_result.clone());
            }
        }
    }

    emit(RpcEvent::Progress {
        id: id.into(),
        status: "Probing…".into(),
        progress: 0.05,
    });

    // Get bundled ffprobe path
    let ffprobe_path = find_ffprobe_binary()
        .await
        .map_err(|e| anyhow::anyhow!("ffprobe not found: {}", e))?;

    emit(RpcEvent::Log {
        id: id.into(),
        message: format!("Found ffprobe at: {}", ffprobe_path),
    });

    emit(RpcEvent::Log {
        id: id.into(),
        message: format!("Probing input file: {}", input),
    });

    // Run ffprobe with proper arguments to get file information as JSON
    let mut cmd = TokioCommand::new(&ffprobe_path);
    cmd.arg("-v")
        .arg("error") // Only show errors, suppress info messages
        .arg("-print_format")
        .arg("json") // Output as JSON for easy parsing
        .arg("-show_streams") // Include information about audio/video streams
        .arg("-show_format") // Include information about file format
        .arg(input) // The file to analyze
        .stdout(std::process::Stdio::piped()) // Capture the output
        .stderr(std::process::Stdio::piped()); // Capture stderr for debugging

    cmd.kill_on_drop(true);

    let child = cmd.spawn()?;

    emit(RpcEvent::Log {
        id: id.into(),
        message: format!(
            "Running ffprobe command: {} -v error -print_format json -show_streams -show_format {}",
            ffprobe_path, input
        ),
    });

    // Wait for ffprobe to finish and get the output
    let out = child.wait_with_output().await?;

    emit(RpcEvent::Log {
        id: id.into(),
        message: format!("ffprobe exit status: {}", out.status),
    });

    if !out.stderr.is_empty() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        emit(RpcEvent::Log {
            id: id.into(),
            message: format!("ffprobe stderr: {}", stderr),
        });
    }

    if !out.stdout.is_empty() {
        let stdout_preview = String::from_utf8_lossy(&out.stdout);
        emit(RpcEvent::Log {
            id: id.into(),
            message: format!(
                "ffprobe stdout preview: {}",
                stdout_preview.chars().take(200).collect::<String>()
            ),
        });
    }

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(anyhow::anyhow!(
            "ffprobe failed with status {}: {}",
            out.status,
            stderr
        ));
    }

    // Parse the JSON output from ffprobe
    let v: serde_json::Value = serde_json::from_slice(&out.stdout)?;

    // Extract duration from format metadata (container level)
    let mut duration = v
        .get("format")
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
    let mut color_space = None;
    let mut color_transfer = None;
    let mut color_primaries = None;

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
                            duration = st
                                .get("duration")
                                .and_then(|x| x.as_str())
                                .and_then(|s| s.parse::<f64>().ok());
                        }

                        // Extract color metadata
                        color_space = st
                            .get("color_space")
                            .and_then(|x| x.as_str())
                            .map(|s| s.to_string());
                        color_transfer = st
                            .get("color_transfer")
                            .and_then(|x| x.as_str())
                            .map(|s| s.to_string());
                        color_primaries = st
                            .get("color_primaries")
                            .and_then(|x| x.as_str())
                            .map(|s| s.to_string());
                    }
                    "audio" => {
                        audio = true;
                        // Extract audio codec name
                        audio_codec = st
                            .get("codec_name")
                            .and_then(|x| x.as_str())
                            .map(|s| s.to_string());
                        // Extract audio bitrate (can be in stream or format)
                        audio_bitrate = st
                            .get("bit_rate")
                            .and_then(|x| x.as_str())
                            .and_then(|s| s.parse::<i32>().ok());
                    }
                    _ => {} // Ignore other stream types (subtitles, data, etc.)
                }
            }
        }
    }

    emit(RpcEvent::Progress {
        id: id.into(),
        status: "Probe complete".into(),
        progress: 1.0,
    });
    let result = ProbeResult {
        duration,
        width,
        height,
        fps,
        audio,
        video,
        audio_codec,
        audio_bitrate,
        color_space,
        color_transfer,
        color_primaries,
    };

    if let Ok(mut cache) = get_probe_cache().write() {
        cache.insert(input.to_string(), (mtime, file_size, result.clone()));
    }

    Ok(result)
}

/// Check if video is HDR based on probe result
pub fn is_hdr(probe: &ProbeResult) -> bool {
    // Check for common HDR transfer characteristics
    if let Some(transfer) = &probe.color_transfer {
        let t = transfer.to_lowercase();
        if t == "smpte2084" || t == "arib-std-b67" {
            return true;
        }
    }

    // Check for BT.2020 color primaries (often implies HDR/WCG)
    if let Some(primaries) = &probe.color_primaries {
        if primaries.to_lowercase().contains("bt2020") {
            return true;
        }
    }

    false
}

// ffmpeg sometimes reports frame rates as fractions (e.g., "30000/1001" for 29.97 fps)
// This function handles both fraction and decimal formats
fn parse_fps(s: &str) -> Option<f64> {
    if s.contains('/') {
        // Handle fraction format like "30000/1001"
        let mut sp = s.split('/');
        let num: f64 = sp.next()?.parse().ok()?; // Numerator
        let den: f64 = sp.next()?.parse().ok()?; // Denominator
        if den == 0.0 {
            return None;
        } // Avoid division by zero
        Some(num / den) // Calculate the actual fps
    } else {
        // Handle decimal format like "29.97"
        s.parse().ok()
    }
}

/// Does ffmpeg find a video stream in `path`? Cover art counts: it extracts
/// like any picture. A file ffmpeg cannot open at all has none, and that is
/// not an audio file either, so it is asked about with the file in hand.
fn has_video_stream(ffmpeg: &str, path: &str) -> bool {
    let out = Command::new(ffmpeg)
        .args(["-hide_banner", "-i", path])
        .output();
    match out {
        Ok(o) => {
            let info = String::from_utf8_lossy(&o.stderr);
            // With no output file ffmpeg lists the streams and then complains
            // about the missing output; an unreadable file never lists any.
            let opened = info.contains("Stream #");
            !opened || info.contains(": Video:")
        }
        Err(_) => true,
    }
}

/// A dark 16:9 frame as JPEG, for a file with no picture of its own.
fn placeholder_frame(ffmpeg: &str) -> anyhow::Result<Vec<u8>> {
    let out = Command::new(ffmpeg)
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=0x18181b:s=1280x720",
        ])
        .args([
            "-frames:v",
            "1",
            "-f",
            "image2",
            "-c:v",
            "mjpeg",
            "-q:v",
            "3",
            "-",
        ])
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to run ffmpeg: {}", e))?;
    if !out.status.success() || out.stdout.is_empty() {
        return Err(anyhow::anyhow!(
            "FFmpeg could not make a placeholder frame: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(out.stdout)
}

/// Extract the first frame of a video as a base64 encoded data URI (JPEG format, bounded to UI preview dimensions)
pub fn extract_first_frame(video_path: &str) -> anyhow::Result<String> {
    // Check in-memory cache first
    let metadata = std::fs::metadata(video_path).ok();
    let mtime = metadata.as_ref().and_then(|m| m.modified().ok());
    let size = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
    let canonical = std::path::Path::new(video_path)
        .canonicalize()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| video_path.to_string());

    if let Ok(cache) = get_frame_cache().read() {
        if let Some((cached_mtime, cached_size, cached_data)) = cache.get(&canonical) {
            if *cached_size == size && *cached_mtime == mtime {
                return Ok(cached_data.to_string());
            }
        }
    }

    let ffmpeg_path = get_ffmpeg_path_sync();

    // Fast input seeking with -ss 0 before -i, bound dimensions to max 1080p,
    // and produce MJPEG directly so we avoid multi-megabyte PNG allocations and copies.
    let output = Command::new(&ffmpeg_path)
        .arg("-ss")
        .arg("0")
        .arg("-i")
        .arg(video_path)
        .arg("-vframes")
        .arg("1")
        .arg("-threads")
        .arg("2")
        .arg("-vf")
        .arg("scale='min(1080,iw)':'min(1080,ih)':force_original_aspect_ratio=decrease:flags=fast_bilinear")
        .arg("-f")
        .arg("image2")
        .arg("-c:v")
        .arg("mjpeg")
        .arg("-q:v")
        .arg("3")
        .arg("-") // Output to stdout
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to run ffmpeg: {}", e))?;

    let jpeg = if output.status.success() {
        output.stdout
    } else if !has_video_stream(&ffmpeg_path, video_path) {
        // An audio-only file has no picture to take. The editor still wants a
        // frame to draw the captions on, so it gets a plain dark one.
        placeholder_frame(&ffmpeg_path)?
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!(
            "FFmpeg failed to extract frame: {}",
            stderr
        ));
    };

    use base64::{engine::general_purpose, Engine as _};
    let encoded = general_purpose::STANDARD.encode(&jpeg);

    // Return with data URI scheme
    let result = format!("data:image/jpeg;base64,{}", encoded);

    // Update cache
    if let Ok(mut cache) = get_frame_cache().write() {
        cache.insert(
            canonical,
            (mtime, size, std::sync::Arc::from(result.as_str())),
        );
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_phone_video_exported_at_source_size_stays_encodable() {
        use super::*;
        // 4032x3024 into 9:16 without downscaling wants 4032x7168, which
        // VideoToolbox refuses; the export then falls back to software and
        // takes long enough to look broken.
        let (w, h) = target_dimensions(Some("original"), 4032, 3024, TargetAR::AR9x16);
        assert!(
            w <= 4096 && h <= 4096,
            "canvas {}x{} is beyond what an H.264 encoder accepts",
            w,
            h
        );
        // The shape is kept.
        let ratio = w as f32 / h as f32;
        assert!(
            (ratio - 9.0 / 16.0).abs() < 0.01,
            "aspect drifted to {}",
            ratio
        );
        assert_eq!((w % 2, h % 2), (0, 0), "dimensions must stay even");
    }

    #[test]
    fn ordinary_sizes_pass_through_untouched() {
        use super::*;
        assert_eq!(
            target_dimensions(Some("1080p"), 1920, 1080, TargetAR::AR9x16),
            (1080, 1920)
        );
        assert_eq!(
            target_dimensions(Some("4k"), 3840, 2160, TargetAR::AR9x16),
            (2160, 3840)
        );
        // A 16:9 source into 16:9 at its own size needs no clamping.
        assert_eq!(
            target_dimensions(Some("original"), 1920, 1080, TargetAR::AR16x9),
            (1920, 1080)
        );
    }

    use super::*;

    // ============================================
    // parse_target_ar tests
    // ============================================

    #[test]
    fn test_parse_target_ar_valid_9x16() {
        let result = parse_target_ar("9:16");
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), TargetAR::AR9x16));
    }

    #[test]
    fn test_parse_target_ar_valid_16x9() {
        let result = parse_target_ar("16:9");
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), TargetAR::AR16x9));
    }

    #[test]
    fn test_parse_target_ar_valid_4x5() {
        let result = parse_target_ar("4:5");
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), TargetAR::AR4x5));
    }

    #[test]
    fn test_parse_target_ar_valid_1x1() {
        let result = parse_target_ar("1:1");
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), TargetAR::AR1x1));
    }

    #[test]
    fn test_parse_target_ar_invalid_format() {
        assert!(parse_target_ar("invalid").is_err());
        assert!(parse_target_ar("3:2").is_err());
        assert!(parse_target_ar("").is_err());
        assert!(parse_target_ar("16x9").is_err()); // wrong separator
    }

    // ============================================
    // round_even tests
    // ============================================

    #[test]
    fn test_round_even_already_even() {
        assert_eq!(round_even(2), 2);
        assert_eq!(round_even(100), 100);
        assert_eq!(round_even(1920), 1920);
    }

    #[test]
    fn test_round_even_odd_numbers() {
        assert_eq!(round_even(1), 2);
        assert_eq!(round_even(3), 4);
        assert_eq!(round_even(1919), 1920);
        assert_eq!(round_even(1081), 1082);
    }

    #[test]
    fn test_round_even_zero() {
        assert_eq!(round_even(0), 0);
    }

    // ============================================
    // ar_wh tests
    // ============================================

    #[test]
    fn test_ar_wh_all_formats() {
        assert_eq!(ar_wh(TargetAR::AR9x16), (9, 16));
        assert_eq!(ar_wh(TargetAR::AR16x9), (16, 9));
        assert_eq!(ar_wh(TargetAR::AR4x5), (4, 5));
        assert_eq!(ar_wh(TargetAR::AR1x1), (1, 1));
    }

    // ============================================
    // canvas_no_downscale tests
    // ============================================

    #[test]
    fn test_canvas_no_downscale_landscape_to_portrait() {
        // 1920x1080 landscape video going to 9:16 portrait
        let (w, h) = canvas_no_downscale(1920, 1080, TargetAR::AR9x16);
        // Should not downscale, so canvas >= source on both dimensions
        assert!(w >= 1920 || h >= 1080);
        // Result should maintain 9:16 aspect ratio
        let ratio = w as f32 / h as f32;
        assert!((ratio - 9.0 / 16.0).abs() < 0.02);
    }

    #[test]
    fn test_canvas_no_downscale_portrait_to_landscape() {
        // 1080x1920 portrait video going to 16:9 landscape
        let (w, h) = canvas_no_downscale(1080, 1920, TargetAR::AR16x9);
        // Canvas should fit the source
        assert!(w >= 1080 || h >= 1920);
        // Result should maintain 16:9 aspect ratio
        let ratio = w as f32 / h as f32;
        assert!((ratio - 16.0 / 9.0).abs() < 0.02);
    }

    #[test]
    fn test_canvas_no_downscale_same_ar() {
        // 1920x1080 to 16:9 - should be minimal change
        let (w, h) = canvas_no_downscale(1920, 1080, TargetAR::AR16x9);
        assert!(w >= 1920);
        assert!(h >= 1080);
    }

    #[test]
    fn test_canvas_no_downscale_square() {
        // Any video to square
        let (w, h) = canvas_no_downscale(1920, 1080, TargetAR::AR1x1);
        assert_eq!(w, h); // Square means equal dimensions
        assert!(w >= 1920); // Should be at least as wide as source
    }

    #[test]
    fn test_canvas_no_downscale_returns_even_dimensions() {
        let (w, h) = canvas_no_downscale(1921, 1081, TargetAR::AR16x9);
        assert_eq!(w % 2, 0, "Width should be even for yuv420");
        assert_eq!(h % 2, 0, "Height should be even for yuv420");
    }

    // ============================================
    // escape_subtitle_path tests
    // ============================================

    #[test]
    fn test_escape_subtitle_path_simple() {
        let result = escape_subtitle_path("/path/to/file.ass");
        assert_eq!(result, "'/path/to/file.ass'");
    }

    #[test]
    fn test_escape_subtitle_path_with_colon() {
        let result = escape_subtitle_path("/path/to/file:name.ass");
        assert!(result.contains(r"\:"));
    }

    #[test]
    fn test_escape_subtitle_path_with_special_chars() {
        let result = escape_subtitle_path("/path/to/file [1].ass");
        // Should be quoted to handle spaces and brackets
        assert!(result.starts_with("'"));
        assert!(result.ends_with("'"));
    }

    #[test]
    fn test_escape_subtitle_path_windows_style() {
        let result = escape_subtitle_path("C:\\Users\\test\\file.ass");
        // Backslashes should be escaped
        assert!(result.contains(r"\\"));
        // Drive letter colon should be escaped
        assert!(result.contains(r"\:"));
    }

    // ============================================
    // crf_to_bitrate tests
    // ============================================

    #[test]
    fn test_crf_to_bitrate_high_quality() {
        assert_eq!(crf_to_bitrate("16"), "12M");
        assert_eq!(crf_to_bitrate("18"), "12M");
        assert_eq!(crf_to_bitrate("19"), "12M");
    }

    #[test]
    fn test_crf_to_bitrate_standard_quality() {
        assert_eq!(crf_to_bitrate("20"), "8M");
        assert_eq!(crf_to_bitrate("22"), "8M");
        assert_eq!(crf_to_bitrate("23"), "8M");
    }

    #[test]
    fn test_crf_to_bitrate_medium_quality() {
        assert_eq!(crf_to_bitrate("24"), "6M");
        assert_eq!(crf_to_bitrate("26"), "6M");
        assert_eq!(crf_to_bitrate("27"), "6M");
    }

    #[test]
    fn test_crf_to_bitrate_low_quality() {
        assert_eq!(crf_to_bitrate("28"), "4M");
        assert_eq!(crf_to_bitrate("35"), "4M");
        assert_eq!(crf_to_bitrate("51"), "4M");
    }

    #[test]
    fn test_crf_to_bitrate_invalid_fallback() {
        assert_eq!(crf_to_bitrate("invalid"), "8M");
        assert_eq!(crf_to_bitrate(""), "8M");
    }

    // ============================================
    // determine_audio_codec tests
    // ============================================

    #[test]
    fn test_determine_audio_codec_no_probe() {
        let (codec, args) = determine_audio_codec(None);
        assert_eq!(codec, "aac");
        assert!(!args.is_empty());
    }

    #[test]
    fn test_determine_audio_codec_no_audio() {
        let probe = ProbeResult {
            duration: Some(10.0),
            width: Some(1920),
            height: Some(1080),
            fps: Some(30.0),
            audio: false,
            video: true,
            audio_codec: None,
            audio_bitrate: None,
            color_space: None,
            color_transfer: None,
            color_primaries: None,
        };
        let (codec, _) = determine_audio_codec(Some(&probe));
        assert_eq!(codec, "aac");
    }

    #[test]
    fn test_determine_audio_codec_mp3_copy() {
        let probe = ProbeResult {
            duration: Some(10.0),
            width: Some(1920),
            height: Some(1080),
            fps: Some(30.0),
            audio: true,
            video: true,
            audio_codec: Some("mp3".to_string()),
            audio_bitrate: Some(128000),
            color_space: None,
            color_transfer: None,
            color_primaries: None,
        };
        let (codec, args) = determine_audio_codec(Some(&probe));
        assert_eq!(codec, "copy");
        assert!(args.is_empty());
    }

    #[test]
    fn test_determine_audio_codec_pcm_reencode() {
        let probe = ProbeResult {
            duration: Some(10.0),
            width: Some(1920),
            height: Some(1080),
            fps: Some(30.0),
            audio: true,
            video: true,
            audio_codec: Some("pcm_s16le".to_string()),
            audio_bitrate: None,
            color_space: None,
            color_transfer: None,
            color_primaries: None,
        };
        let (codec, _) = determine_audio_codec(Some(&probe));
        assert_eq!(codec, "aac"); // PCM should be re-encoded
    }

    #[test]
    fn test_determine_audio_codec_flac_reencode() {
        let probe = ProbeResult {
            duration: Some(10.0),
            width: Some(1920),
            height: Some(1080),
            fps: Some(30.0),
            audio: true,
            video: true,
            audio_codec: Some("flac".to_string()),
            audio_bitrate: None,
            color_space: None,
            color_transfer: None,
            color_primaries: None,
        };
        let (codec, _) = determine_audio_codec(Some(&probe));
        assert_eq!(codec, "aac"); // Lossless should be re-encoded for size
    }

    #[test]
    fn test_determine_audio_codec_low_bitrate_aac_copy() {
        let probe = ProbeResult {
            duration: Some(10.0),
            width: Some(1920),
            height: Some(1080),
            fps: Some(30.0),
            audio: true,
            video: true,
            audio_codec: Some("aac".to_string()),
            audio_bitrate: Some(128000), // 128kbps - should copy
            color_space: None,
            color_transfer: None,
            color_primaries: None,
        };
        let (codec, args) = determine_audio_codec(Some(&probe));
        assert_eq!(codec, "copy");
        assert!(args.is_empty());
    }

    // ============================================
    // build_fitpad_filter tests
    // ============================================

    #[test]
    fn test_build_fitpad_filter_basic() {
        let filter = build_fitpad_filter(1920, 1080, None, false);
        assert!(filter.contains("scale="));
        assert!(filter.contains("pad="));
        assert!(filter.contains("format=yuv420p"));
    }

    #[test]
    fn test_build_fitpad_filter_with_subtitles() {
        let filter = build_fitpad_filter(1920, 1080, Some("/path/to/subs.ass"), false);
        assert!(filter.contains("ass="));
    }

    #[test]
    fn test_build_fitpad_filter_with_hdr() {
        let filter = build_fitpad_filter(1920, 1080, None, true);
        assert!(filter.contains("tonemap"));
        assert!(filter.contains("zscale"));
    }

    #[test]
    fn test_build_fitpad_filter_videotoolbox_format() {
        let filter =
            build_fitpad_filter_with_format(1920, 1080, None, HardwareEncoder::VideoToolbox, false);
        assert!(filter.contains("format=nv12"));
    }

    #[test]
    fn test_build_fitpad_filter_software_format() {
        let filter =
            build_fitpad_filter_with_format(1920, 1080, None, HardwareEncoder::Software, false);
        assert!(filter.contains("format=yuv420p"));
    }

    #[test]
    fn test_build_fitpad_filter_fill_strategy() {
        let filter = build_fitpad_filter_with_options(
            1080,
            1920,
            None,
            HardwareEncoder::Software,
            "fill",
            false,
        );
        assert!(filter.contains("crop="));
        assert!(filter.contains("force_original_aspect_ratio=increase"));
    }

    #[test]
    fn test_build_fitpad_filter_fit_strategy() {
        let filter = build_fitpad_filter_with_options(
            1080,
            1920,
            None,
            HardwareEncoder::Software,
            "fit",
            false,
        );
        assert!(filter.contains("pad="));
        assert!(filter.contains("force_original_aspect_ratio=decrease"));
    }

    #[test]
    fn test_bounded_cache_capacity_and_eviction() {
        let mut cache = BoundedCache::new(3);
        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());

        cache.insert("a".to_string(), 1);
        cache.insert("b".to_string(), 2);
        cache.insert("c".to_string(), 3);
        assert_eq!(cache.len(), 3);
        assert_eq!(cache.get(&"a".to_string()), Some(&1));

        // Inserting 4th item should evict oldest "a"
        cache.insert("d".to_string(), 4);
        assert_eq!(cache.len(), 3);
        assert_eq!(cache.get(&"a".to_string()), None);
        assert_eq!(cache.get(&"b".to_string()), Some(&2));
        assert_eq!(cache.get(&"c".to_string()), Some(&3));
        assert_eq!(cache.get(&"d".to_string()), Some(&4));

        // Updating existing key "b" shouldn't change len or evict anything
        cache.insert("b".to_string(), 20);
        assert_eq!(cache.len(), 3);
        assert_eq!(cache.get(&"b".to_string()), Some(&20));

        // Clear
        cache.clear();
        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
    }

    #[test]
    fn test_extract_first_frame_jpeg_and_cache() {
        let sample = "../electron/app/assets/oneliner.mp4";
        if !std::path::Path::new(sample).exists() {
            return;
        }

        let first = extract_first_frame(sample).expect("extract_first_frame failed");
        assert!(
            first.starts_with("data:image/jpeg;base64,"),
            "Expected data:image/jpeg;base64 prefix, got {}",
            &first[..first.len().min(30)]
        );

        // Verify base64 decode
        use base64::{engine::general_purpose, Engine as _};
        let b64_part = first.strip_prefix("data:image/jpeg;base64,").unwrap();
        let decoded = general_purpose::STANDARD
            .decode(b64_part)
            .expect("valid base64");
        assert!(!decoded.is_empty());
        // JPEG magic header bytes: 0xFF, 0xD8
        assert_eq!(decoded[0], 0xFF);
        assert_eq!(decoded[1], 0xD8);

        // Verify it is compact (< 250 KB)
        assert!(
            first.len() < 250_000,
            "thumbnail data URI should be compact, was {}",
            first.len()
        );

        // Cache hit test
        let second = extract_first_frame(sample).expect("cached extract failed");
        assert_eq!(first, second);
    }

    #[test]
    fn gpu_scale_dims_fill_covers_the_target_with_even_sides() {
        // 16:9 into 9:16: the height is matched, the width overshoots to be cropped.
        assert_eq!(gpu_scale_dims(1920, 1080, 1080, 1920, true), (3414, 1920));
        // Same shape: nothing to crop, nothing lost.
        assert_eq!(gpu_scale_dims(3840, 2160, 1920, 1080, true), (1920, 1080));
        // Odd source: still even, never under the target.
        let (w, h) = gpu_scale_dims(853, 481, 1080, 1920, true);
        assert!(w % 2 == 0 && h % 2 == 0 && w >= 1080 && h >= 1920);
    }

    #[test]
    fn gpu_scale_dims_fit_stays_inside_the_target_with_even_sides() {
        assert_eq!(gpu_scale_dims(1920, 1080, 1080, 1920, false), (1080, 608));
        let (w, h) = gpu_scale_dims(853, 481, 1080, 1920, false);
        assert!(w % 2 == 0 && h % 2 == 0 && w <= 1080 && h <= 1920);
    }

    #[test]
    fn gpu_filter_scales_on_the_gpu_then_crops_or_pads_on_the_cpu() {
        let fill = build_gpu_fitpad_filter(1920, 1080, 1080, 1920, None, "fill");
        assert_eq!(
            fill,
            "scale_vt=w=3414:h=1920,hwdownload,format=nv12,crop=1080:1920:(iw-ow)/2:(ih-oh)/2,format=nv12"
        );
        let fit = build_gpu_fitpad_filter(1920, 1080, 1080, 1920, None, "fit");
        assert_eq!(
            fit,
            "scale_vt=w=1080:h=608,hwdownload,format=nv12,pad=1080:1920:(ow-iw)/2:(oh-ih)/2:black,format=nv12"
        );
        // No CPU scaler is left in the chain.
        assert!(!fill.contains(",scale=") && !fit.contains(",scale="));
    }

    #[test]
    fn gpu_filter_draws_the_captions_after_the_download() {
        let f = build_gpu_fitpad_filter(1920, 1080, 1080, 1920, Some("/tmp/a.ass"), "fill");
        let download = f.find("hwdownload").unwrap();
        let ass = f.find("ass=").unwrap();
        assert!(download < ass, "libass only reads system memory");
    }

    #[test]
    fn proof_dims_keep_the_aspect_and_round_to_even() {
        assert_eq!(proof_dims(1080, 1920, 720), (720, 1280));
        assert_eq!(proof_dims(1920, 1080, 540), (960, 540));
        assert_eq!(proof_dims(1080, 1350, 540), (540, 676));
        assert_eq!(
            gpu_proof_scale_filter(1080, 1920, 720),
            "hwupload,scale_vt=w=720:h=1280,hwdownload,format=nv12"
        );
    }

    #[test]
    fn videotoolbox_tries_the_gpu_pipeline_first_and_ends_on_software() {
        use Accel::*;
        use HardwareEncoder::*;
        assert_eq!(
            encode_attempts(VideoToolbox, true, false),
            vec![
                (VideoToolbox, Pipeline),
                (VideoToolbox, Decode),
                (VideoToolbox, Off),
                (Software, Off)
            ]
        );
        // No scale_vt in this ffmpeg: decode on the GPU, filter as before.
        assert_eq!(
            encode_attempts(VideoToolbox, false, false),
            vec![(VideoToolbox, Decode), (VideoToolbox, Off), (Software, Off)]
        );
        // HDR keeps the CPU tone-mapping chain.
        assert_eq!(
            encode_attempts(VideoToolbox, true, true),
            vec![(VideoToolbox, Off), (Software, Off)]
        );
        assert_eq!(
            encode_attempts(Nvenc, true, false),
            vec![(Nvenc, Off), (Software, Off)]
        );
        assert_eq!(
            encode_attempts(Software, true, false),
            vec![(Software, Off)]
        );
    }

    #[test]
    fn vt_decode_args_keep_frames_on_the_gpu_only_when_asked() {
        assert_eq!(vt_decode_args(false), vec!["-hwaccel", "videotoolbox"]);
        assert!(vt_decode_args(true).contains(&"videotoolbox_vld"));
    }

    #[test]
    fn first_frame_of_an_audio_only_file_is_a_placeholder_not_an_error() {
        // An audio episode has no picture to take; the editor still wants one to
        // draw captions on. Before, ffmpeg's "Output file does not contain any
        // stream" came back as an error dialog on every load.
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("episode.wav");
        let made = std::process::Command::new(get_ffmpeg_path_sync())
            .args(["-v", "error", "-y", "-f", "lavfi", "-i", "sine=duration=1"])
            .arg(&wav)
            .status()
            .expect("ffmpeg runs");
        assert!(made.success());

        let frame = extract_first_frame(wav.to_str().unwrap())
            .expect("an audio file gets a placeholder frame");
        assert!(frame.starts_with("data:image/jpeg;base64,"));
        assert!(frame.len() > 200, "an actual image, not an empty payload");
    }

    #[test]
    fn first_frame_of_a_missing_file_is_still_an_error() {
        // The placeholder is for audio only; a file ffmpeg cannot read at all
        // must not be dressed up as one.
        let err = extract_first_frame("/nonexistent/nothing.mp4").unwrap_err();
        assert!(err.to_string().contains("FFmpeg failed to extract frame"));
    }
}
