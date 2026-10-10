use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptionSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    // Optional word-level timing (used when split_by_words = true)
    #[serde(default)]
    pub words: Vec<WordSpan>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WordSpan {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    /// This is the tail of the previous word, not a word of its own.
    ///
    /// Whisper emits sub-word pieces and marks a new word with a leading space.
    /// When it also breaks a segment mid-word — common in Finnish, where words
    /// are long — the pieces arrive in different segments and would otherwise be
    /// drawn with a space between them ("KORKE ALLA" instead of "KORKEALLA").
    #[serde(default)]
    pub glue_to_previous: bool,
}

/// A manual vertical placement for the caption(s) shown during a time range.
///
/// `y_pct` is the position of the caption's anchor point (the same point ASS
/// puts at `\pos`, which depends on the style alignment) as a percentage of the
/// frame height, measured from the top. A cue adopts an override when its
/// midpoint falls inside the range, so overrides survive text edits that shift
/// segment boundaries a little.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PositionOverride {
    pub start_ms: u64,
    pub end_ms: u64,
    pub y_pct: f32,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TranscribeSegmentsParams {
    pub audio: String,              // Path to audio file to transcribe
    pub model: Option<String>,      // Whisper model to use (default: "whisper-1")
    pub language: Option<String>,   // Language hint for better accuracy
    pub split_by_words: bool,       // Whether to split by words or segments
    pub api_key: Option<String>,    // OpenAI API key
    pub prompt: Option<String>,     // Context prompt to improve accuracy
    pub video_file: Option<String>, // Original video file path (for JSON output location)
    #[serde(default)]
    pub whisper_base_url: Option<String>, // Optional OpenAI-compatible Whisper server base URL (e.g. a whisper.cpp server on another machine)
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TranscribeSegmentsResult {
    pub segments: Vec<CaptionSegment>, // Caption segments with timing
    pub full_text: String,             // Complete transcription text
    pub duration: Option<f64>,         // Total audio duration
    pub json_file: String,             // Path to saved JSON captions file
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BurnResult {
    pub video: String, // Path to video with burned-in subtitles
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WhisperCacheEntry {
    pub audio_hash: String,    // blake3 hash of audio file content
    pub params_hash: String,   // blake3 hash of transcription parameters
    pub response_path: String, // path to cached JSON response file
    pub timestamp: u64,        // unix timestamp for LRU eviction
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WhisperCacheIndex {
    pub entries: Vec<WhisperCacheEntry>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WhisperSegment {
    pub id: u32,
    pub start: f64,
    pub end: f64,
    pub text: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WhisperWord {
    pub word: String,
    pub start: f64,
    pub end: f64,
    /// Set when this piece had no leading space, meaning it continues the
    /// previous word rather than starting a new one. Defaults to false so
    /// cached responses and remote servers that send whole words still parse.
    #[serde(default)]
    pub glue_to_previous: bool,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WhisperResponse {
    pub task: Option<String>,
    pub language: Option<String>,
    pub duration: Option<f64>,
    pub text: String,
    pub segments: Option<Vec<WhisperSegment>>,
    pub words: Option<Vec<WhisperWord>>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ExtractAudioParams {
    pub input: String,         // Path to input video file
    pub codec: Option<String>, // Audio codec to use (default: "aac")
    pub out: Option<String>,   // Output path (default: input filename with .m4a extension)
    /// Only this part of the input; the whole of it when absent.
    #[serde(default)]
    pub range: Option<AudioRange>,
}

/// A stretch of a recording, in seconds from its start.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AudioRange {
    pub start_sec: f64,
    pub duration_sec: f64,
}

impl AudioRange {
    /// From the editor's millisecond marks. None unless there is a start or
    /// an end to honour, and the end comes after the start.
    pub fn from_ms(start_ms: Option<u64>, end_ms: Option<u64>) -> Option<AudioRange> {
        let start = start_ms.unwrap_or(0);
        let end = end_ms?;
        (end > start).then(|| AudioRange {
            start_sec: start as f64 / 1000.0,
            duration_sec: (end - start) as f64 / 1000.0,
        })
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ExtractAudioResult {
    pub audio: String, // Path to the extracted audio file
}

fn default_true() -> bool {
    true
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GenerateCaptionsParams {
    pub input_video: String, // Path to input video file
    /// Transcribe only from here (ms); with `transcribe_end_ms`. A long episode
    /// is transcribed a part at a time rather than all of it at once.
    #[serde(default)]
    pub transcribe_start_ms: Option<u64>,
    #[serde(default)]
    pub transcribe_end_ms: Option<u64>,
    #[serde(default)]
    pub export_formats: Vec<String>, // List of aspect ratios to export (e.g., ["9:16", "16:9"])
    #[serde(default)]
    pub karaoke: bool, // Whether to use karaoke-style highlighting
    #[serde(default)]
    pub multiline: bool, // Whether to allow multiple lines (karaoke)
    /// Two lines flush to the same width, each sized to fill it.
    #[serde(default)]
    pub justify_lines: bool,
    pub font_name: Option<String>, // Font name for captions (defaults to "Montserrat Black")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_size: Option<u32>, // Base font size (at 1080p reference)
    #[serde(default = "default_true")]
    pub split_by_words: bool, // Whether to split transcription by words or segments
    pub model: Option<String>,     // Whisper model to use (default: "whisper-1")
    pub language: Option<String>,  // Language hint for better accuracy
    pub prompt: Option<String>,    // Context prompt to improve accuracy
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_color: Option<String>, // Text color as hex string (e.g., "#ffffff")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub highlight_word_color: Option<String>, // Highlight word color as hex string
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outline_color: Option<String>, // Outline color as hex string
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline_width: Option<u32>, // Outline stroke width in px (default: 4)
    #[serde(default)]
    pub background_box: bool, // Semi-transparent box behind caption text (default: false, outline only)
    #[serde(default)]
    pub glow_effect: bool, // Whether to apply glow effect
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<String>, // Caption position: "bottom" or "center"
    pub api_key: Option<String>, // OpenAI API key
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_size: Option<String>, // Target output size (e.g., "1080p", "original")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crop_strategy: Option<String>, // "start", "center", "end", "fit" (letterbox)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub whisper_base_url: Option<String>, // Optional OpenAI-compatible Whisper server base URL
    #[serde(default)]
    pub position_overrides: Vec<PositionOverride>, // Manually placed captions
    /// Vertical stretches the target platforms cover with their own interface,
    /// as (top, bottom) percentages of frame height. Captions dodge these unless
    /// the user placed them by hand.
    #[serde(default)]
    pub blocked_bands: Vec<(f32, f32)>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GenerateCaptionsResult {
    pub probe_result: crate::video::ProbeResult, // Original video information
    pub audio_file: String,                      // Path to extracted audio file
    pub transcription: TranscribeSegmentsResult, // Transcription results and segments
    pub captioned_videos: Vec<CaptionedVideoResult>, // List of generated videos with captions
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BurnCaptionsParams {
    pub input_video: String,           // Path to input video file
    pub segments: Vec<CaptionSegment>, // The edited segments to burn
    #[serde(default)]
    pub trim_start_ms: u64,
    #[serde(default)]
    pub trim_end_ms: u64,
    pub export_formats: Vec<String>, // List of aspect ratios to export
    /// Also write a smaller proof copy whose short side is this many pixels (e.g. 720),
    /// from the same encode. None or 0: no proof copy.
    #[serde(default)]
    pub proof_short_side: Option<u32>,
    /// A ready-made clip played before the captioned video (a channel ident, say).
    /// Scaled and padded to the output frame; its own audio, or silence.
    #[serde(default)]
    pub intro_video: Option<String>,
    /// A ready-made clip played after it ("listen to the new episode").
    #[serde(default)]
    pub outro_video: Option<String>,
    /// A logo ("bug") kept in a corner of the captioned video, for sources that
    /// don't carry one. Not put on the intro or outro.
    #[serde(default)]
    pub watermark: Option<WatermarkParams>,
    pub karaoke: bool, // Whether to use karaoke-style highlighting
    #[serde(default)]
    pub multiline: bool, // Whether to allow multiple lines (karaoke)
    /// Two lines flush to the same width, each sized to fill it.
    #[serde(default)]
    pub justify_lines: bool,
    pub font_name: Option<String>, // Font name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_size: Option<u32>, // Base font size
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_color: Option<String>, // Text color
    #[serde(skip_serializing_if = "Option::is_none")]
    pub highlight_word_color: Option<String>, // Highlight word color
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outline_color: Option<String>, // Outline color
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline_width: Option<u32>, // Outline stroke width in px (default: 4)
    #[serde(default)]
    pub background_box: bool, // Semi-transparent box behind caption text (default: false, outline only)
    #[serde(default)]
    pub glow_effect: bool, // Whether to apply glow effect
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<String>, // Caption position
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_size: Option<String>, // Target output size
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crop_strategy: Option<String>, // Crop strategy
    #[serde(default)]
    pub position_overrides: Vec<PositionOverride>, // Manually placed captions
    /// Vertical stretches the target platforms cover with their own interface,
    /// as (top, bottom) percentages of frame height. Captions dodge these unless
    /// the user placed them by hand.
    #[serde(default)]
    pub blocked_bands: Vec<(f32, f32)>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WatermarkParams {
    /// An image, best a PNG with transparency.
    pub path: String,
    /// "top-left", "top-right", "bottom-left" or "bottom-right".
    #[serde(default = "default_watermark_corner")]
    pub corner: String,
    /// Width of the logo, in percent of the frame's shorter side.
    #[serde(default = "default_watermark_size")]
    pub size_pct: f32,
    /// Distance from the two edges, in percent of the frame's shorter side.
    #[serde(default = "default_watermark_margin")]
    pub margin_pct: f32,
    /// 0..1
    #[serde(default = "default_watermark_opacity")]
    pub opacity: f32,
}

fn default_watermark_corner() -> String {
    "top-right".into()
}
fn default_watermark_size() -> f32 {
    12.0
}
fn default_watermark_margin() -> f32 {
    4.0
}
fn default_watermark_opacity() -> f32 {
    0.85
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CaptionedVideoResult {
    pub format: String,          // The aspect ratio format (e.g., "9:16")
    pub raw_video: String,       // Path to reformatted video without captions
    pub captioned_video: String, // Path to final video with captions
    pub width: u32,              // Video width
    pub height: u32,             // Video height
    /// The smaller proof copy, when one was asked for and the video is bigger than it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proof_video: Option<String>,
}

// Model download types
#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DownloadModelParams {
    pub model: String, // Model name: "tiny", "base", "small", "medium", "large", "turbo"
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DownloadModelResult {
    pub model: String, // Model name that was downloaded
    pub path: String,  // Path where model was saved
    pub size: u64,     // Downloaded file size in bytes
}

// Frame extraction types
#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ExtractFirstFrameParams {
    pub video_path: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ExtractFirstFrameResult {
    pub image_data: String, // Base64 encoded image
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PreviewLayoutParams {
    pub segments: Vec<CaptionSegment>,
    pub width: u32,
    pub height: u32,
    pub font_name: Option<String>,
    pub font_size: Option<u32>,
    pub text_color: Option<String>,
    pub highlight_word_color: Option<String>,
    pub outline_color: Option<String>,
    #[serde(default)]
    pub outline_width: Option<u32>,
    #[serde(default)]
    pub background_box: bool,
    pub position: Option<String>,
    pub karaoke: bool,
    #[serde(default)]
    pub multiline: bool,
    #[serde(default)]
    pub justify_lines: bool,
    pub glow_effect: bool,
    #[serde(default)]
    pub position_overrides: Vec<PositionOverride>,
    /// Vertical stretches the target platforms cover with their own interface,
    /// as (top, bottom) percentages of frame height. Captions dodge these unless
    /// the user placed them by hand.
    #[serde(default)]
    pub blocked_bands: Vec<(f32, f32)>,
    /// The canvas the captions will actually be burned onto: an export format
    /// ("9:16", "16:9", "4:5", "1:1") the source gets fitted into, or "source"
    /// / absent for the frame as it is. `width`/`height` are the source's, so
    /// the layout is computed for the same canvas the export will use.
    #[serde(default)]
    pub export_format: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PreviewLayoutResult {
    pub cues: Vec<PreviewCue>,
    pub font_size_px: u32, // Rendered font size at the requested frame size
    /// The canvas the layout was computed for — the export canvas when
    /// `export_format` asked for one, otherwise the frame that was passed in.
    pub frame_width: u32,
    pub frame_height: u32,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PreviewCue {
    pub start_ms: u64,
    pub end_ms: u64,
    pub lines: Vec<PreviewLine>,
    pub y_pct: f32, // Anchor position as percentage from top
    // The visual block this cue belongs to. Karaoke emits one cue per word
    // window, but all windows of a block share these bounds — so the editor can
    // treat a block as a single draggable caption.
    pub group_start_ms: u64,
    pub group_end_ms: u64,
    pub anchor: String, // Which edge of the text block sits at y_pct: "top" | "center" | "bottom"
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PreviewLine {
    pub words: Vec<PreviewWord>,
    /// Size this line renders at. Lines differ under justified layout.
    pub font_size_px: u32,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PreviewWord {
    pub text: String,
    pub is_highlighted: bool,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SaveCaptionsParams {
    pub video_path: String,
    pub segments: Vec<CaptionSegment>,
    #[serde(default)]
    pub position_overrides: Vec<PositionOverride>,
}

/// On-disk shape of the `<video>.capslap.json` sidecar.
///
/// Older builds wrote a bare array of segments; `load_captions` still reads
/// those, and saving migrates the file to this form.
#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CaptionsFile {
    pub segments: Vec<CaptionSegment>,
    #[serde(default)]
    pub position_overrides: Vec<PositionOverride>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LoadCaptionsParams {
    pub video_path: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LoadCaptionsResult {
    pub segments: Option<Vec<CaptionSegment>>,
    #[serde(default)]
    pub position_overrides: Vec<PositionOverride>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PreviewFrameParams {
    pub input_video: String,           // Path to input video file
    pub segments: Vec<CaptionSegment>, // The edited segments (we usually only need the one at timestamp)
    pub timestamp_ms: u64,             // Time in ms to extract frame from
    pub export_format: String,         // Aspect ratio format (e.g., "9:16")
    pub fit_mode: Option<String>,      // "cover" or "contain" (default "cover" aka "fit")
    pub karaoke: bool,                 // Whether to use karaoke-style highlighting
    #[serde(default)]
    pub multiline: bool, // Whether to allow multiple lines (karaoke)
    /// Two lines flush to the same width, each sized to fill it.
    #[serde(default)]
    pub justify_lines: bool,
    pub font_name: Option<String>, // Font name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_size: Option<u32>, // Base font size
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_color: Option<String>, // Text color
    #[serde(skip_serializing_if = "Option::is_none")]
    pub highlight_word_color: Option<String>, // Highlight word color
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outline_color: Option<String>, // Outline color
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline_width: Option<u32>, // Outline stroke width in px (default: 4)
    #[serde(default)]
    pub background_box: bool, // Semi-transparent box behind caption text (default: false, outline only)
    #[serde(default)]
    pub glow_effect: bool, // Whether to apply glow effect
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<String>, // Caption position
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_size: Option<String>, // Target output size
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crop_strategy: Option<String>, // Crop strategy
    #[serde(default)]
    pub position_overrides: Vec<PositionOverride>, // Manually placed captions
    /// What to draw:
    /// - "video" (default): the frame with captions burned in
    /// - "clean": the frame with no captions
    /// - "captions": only the captions, on a transparent canvas, so the editor
    ///   can composite and drag them over a "clean" frame without re-rendering
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_mode: Option<String>,
    /// Scale the result down to this height (filmstrip thumbnails).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail_height: Option<u32>,
    /// Vertical stretches the target platforms cover with their own interface,
    /// as (top, bottom) percentages of frame height. Captions dodge these unless
    /// the user placed them by hand.
    #[serde(default)]
    pub blocked_bands: Vec<(f32, f32)>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AutoPlaceParams {
    pub input_video: String,
    pub segments: Vec<CaptionSegment>,
    pub export_format: String,
    pub karaoke: bool,
    #[serde(default)]
    pub multiline: bool,
    /// Two lines flush to the same width, each sized to fill it.
    #[serde(default)]
    pub justify_lines: bool,
    pub font_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_size: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub highlight_word_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outline_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline_width: Option<u32>,
    #[serde(default)]
    pub background_box: bool,
    #[serde(default)]
    pub glow_effect: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_size: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crop_strategy: Option<String>,
    /// Vertical stretches the caption should stay out of, as (top, bottom)
    /// percentages of frame height — where the target platforms draw their own
    /// interface over the video.
    #[serde(default)]
    pub blocked_bands: Vec<(f32, f32)>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AutoPlaceResult {
    pub position_overrides: Vec<PositionOverride>,
    /// How many captions the picture argued for moving.
    pub moved: usize,
    /// How many captions were considered.
    pub total: usize,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PreviewFrameResult {
    pub image_data: String, // Base64 encoded image data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_captions_params_deserialization_defaults() {
        let json = r#"{"inputVideo": "test.mp4"}"#;
        let params: GenerateCaptionsParams =
            serde_json::from_str(json).expect("should deserialize with defaults");
        assert_eq!(params.input_video, "test.mp4");
        assert!(params.export_formats.is_empty());
        assert!(!params.karaoke);
        assert!(params.split_by_words);
    }
}

#[cfg(test)]
mod audio_range_tests {
    use super::AudioRange;

    #[test]
    fn marks_in_milliseconds_become_a_start_and_a_length_in_seconds() {
        assert_eq!(
            AudioRange::from_ms(Some(60_000), Some(90_500)),
            Some(AudioRange {
                start_sec: 60.0,
                duration_sec: 30.5
            })
        );
    }

    #[test]
    fn a_missing_start_means_the_beginning() {
        assert_eq!(
            AudioRange::from_ms(None, Some(5_000)),
            Some(AudioRange {
                start_sec: 0.0,
                duration_sec: 5.0
            })
        );
    }

    #[test]
    fn no_end_or_an_end_before_the_start_means_the_whole_file() {
        assert_eq!(AudioRange::from_ms(Some(1_000), None), None);
        assert_eq!(AudioRange::from_ms(None, None), None);
        assert_eq!(AudioRange::from_ms(Some(5_000), Some(5_000)), None);
        assert_eq!(AudioRange::from_ms(Some(9_000), Some(2_000)), None);
    }
}
