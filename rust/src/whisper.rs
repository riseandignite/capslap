use crate::rpc::RpcEvent;
use crate::types::{
    CaptionSegment, TranscribeSegmentsParams, TranscribeSegmentsResult, WhisperCacheEntry,
    WhisperCacheIndex, WhisperResponse, WhisperWord, WordSpan,
};
use crate::video::{is_ffmpeg_whisper_available, is_whisper_cpp_available};
use blake3;
use regex::Regex;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::OnceLock;
use tokio::fs;
use tokio::process::Command as TokioCommand;

static CACHED_WHISPER_PATH: OnceLock<Option<String>> = OnceLock::new();
static CACHED_FFMPEG_PATH: OnceLock<Option<String>> = OnceLock::new();
static CACHED_FFPROBE_PATH: OnceLock<Option<String>> = OnceLock::new();

/// Bounded buffer for subprocess log collection that retains head lines (for startup logs)
/// and a rolling tail (for error reporting), avoiding unbounded memory growth on long runs.
#[derive(Debug)]
pub struct BoundedLogBuffer {
    head: String,
    tail_lines: std::collections::VecDeque<String>,
    max_head_bytes: usize,
    max_tail_bytes: usize,
    tail_bytes: usize,
}

impl BoundedLogBuffer {
    pub fn new(max_head_bytes: usize, max_tail_bytes: usize) -> Self {
        Self {
            head: String::new(),
            tail_lines: std::collections::VecDeque::new(),
            max_head_bytes,
            max_tail_bytes,
            tail_bytes: 0,
        }
    }

    pub fn push_line(&mut self, line: &str) {
        if self.head.len() < self.max_head_bytes {
            let available = self.max_head_bytes - self.head.len();
            if line.len() < available {
                self.head.push_str(line);
                self.head.push('\n');
            } else {
                let end = line.floor_char_boundary(available.min(line.len()));
                self.head.push_str(&line[..end]);
            }
        }

        let line_len = line.len() + 1;
        while self.tail_bytes + line_len > self.max_tail_bytes && !self.tail_lines.is_empty() {
            if let Some(removed) = self.tail_lines.pop_front() {
                self.tail_bytes = self.tail_bytes.saturating_sub(removed.len() + 1);
            }
        }

        self.tail_bytes += line_len;
        self.tail_lines.push_back(line.to_string());
    }

    pub fn head_chars(&self, count: usize) -> String {
        self.head.chars().take(count).collect()
    }

    pub fn error_summary(&self, count: usize) -> String {
        if !self.tail_lines.is_empty() {
            let tail: String = self
                .tail_lines
                .iter()
                .flat_map(|l| [l.as_str(), "\n"])
                .collect();
            tail.chars().take(count).collect()
        } else {
            self.head.chars().take(count).collect()
        }
    }
}

/// Transcribe audio using whisper.cpp CLI (preferred method)
pub async fn transcribe_with_whisper_cpp(
    id: &str,
    audio_path: &str,
    model: Option<String>,
    language: Option<String>,
    mut emit: impl FnMut(RpcEvent),
) -> anyhow::Result<WhisperResponse> {
    // Use requested model or default to tiny
    let whisper_model = match model.as_deref() {
        Some(m) => m.to_string(),
        None => "tiny".to_string(),
    };

    emit(RpcEvent::Log {
        id: id.into(),
        message: format!(
            "Starting local whisper.cpp transcription with model: {}",
            whisper_model
        ),
    });

    emit(RpcEvent::Log {
        id: id.into(),
        message: format!(
            "Model requested: {}, DTW preset: disabled (testing without DTW)",
            whisper_model
        ),
    });

    // Find model with fallbacks
    let (model_path, actual_model) = ensure_whisper_model(&whisper_model).await?;

    if actual_model != whisper_model {
        emit(RpcEvent::Log {
            id: id.into(),
            message: format!(
                "Model '{}' not found, using '{}' instead",
                whisper_model, actual_model
            ),
        });
    }

    emit(RpcEvent::Log {
        id: id.into(),
        message: format!("Using model file: {} ({})", model_path, actual_model),
    });

    let whisper_binary = match find_whisper_binary().await {
        Ok(binary) => {
            emit(RpcEvent::Log {
                id: id.into(),
                message: format!("Found whisper binary at: {}", binary),
            });
            binary
        }
        Err(e) => {
            emit(RpcEvent::Log {
                id: id.into(),
                message: format!("Failed to find whisper binary: {}", e),
            });
            return Err(e);
        }
    };
    let mut cmd = TokioCommand::new(&whisper_binary);
    // Make sure we don't leak orphaned whisper.cpp processes if the
    // transcription request gets cancelled mid-run.
    cmd.kill_on_drop(true);
    // DTW disabled - causes timestamp issues for some audio files

    let threads = std::thread::available_parallelism()
        .map(|n| n.get().min(16))
        .unwrap_or(4);

    cmd.arg("-m")
        .arg(&model_path)
        .arg("-t")
        .arg(threads.to_string())
        .arg("--output-json-full") // Full JSON output
        // NOTE: no --no-prints — we rely on whisper.cpp's stderr "progress = N%"
        // lines to report transcription progress back to the UI. That line is
        // only printed when --print-progress is passed (print_progress
        // defaults to false in whisper.cpp's CLI), so it must be requested
        // explicitly or the progress bar never moves despite transcription
        // actually running.
        .arg("--print-progress")
        .arg("--word-thold")
        .arg("0.01") // Better word boundary detection
        .arg("--max-len")
        .arg("0") // No segment length limit
        .arg("--output-words") // Enable word-level timestamps
        .arg("--entropy-thold")
        .arg("2.8") // Anti-repetition
        .arg("--suppress-nst"); // Suppress non-speech tokens

    cmd.arg(audio_path);

    if let Some(lang) = &language {
        cmd.arg("-l").arg(lang);
    }

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd.spawn()?;

    // Drain stdout on a background task so child cannot deadlock, while capturing
    // only a bounded prefix for diagnostic logging (the full transcript is written to JSON file).
    let stdout_stream = child.stdout.take();
    let stdout_task = tokio::spawn(async move {
        use tokio::io::AsyncReadExt;
        if let Some(mut stream) = stdout_stream {
            let mut prefix = vec![0u8; 1024];
            let mut total_read = 0;
            while total_read < prefix.len() {
                match stream.read(&mut prefix[total_read..]).await {
                    Ok(0) => break,
                    Ok(n) => total_read += n,
                    Err(_) => break,
                }
            }
            prefix.truncate(total_read);
            // Drain remaining stdout to sink to prevent child process buffer deadlock
            let _ = tokio::io::copy(&mut stream, &mut tokio::io::sink()).await;
            return String::from_utf8_lossy(&prefix).into_owned();
        }
        String::new()
    });

    // Stream whisper.cpp's stderr with a bounded buffer so memory stays strictly
    // bounded during long transcriptions, while still reporting real progress and
    // capturing diagnostics / error details.
    let mut stderr_buf = BoundedLogBuffer::new(1024, 4096);
    if let Some(stderr) = child.stderr.take() {
        use tokio::io::AsyncBufReadExt;
        let mut lines = tokio::io::BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            // whisper.cpp prints lines like:
            //   whisper_print_progress_callback: progress = 42%
            if let Some(idx) = line.find("progress = ") {
                let rest = &line[idx + "progress = ".len()..];
                if let Some(pct_part) = rest.split('%').next() {
                    if let Ok(pct) = pct_part
                        .trim()
                        .trim_start_matches('=')
                        .trim()
                        .parse::<f32>()
                    {
                        emit(RpcEvent::Progress {
                            id: id.into(),
                            status: format!("Transcribing... {:.0}%", pct),
                            progress: (pct / 100.0).min(0.95),
                        });
                    }
                }
            }
            stderr_buf.push_line(&line);
        }
    }

    let status = child.wait().await?;
    let stdout = stdout_task.await.unwrap_or_default();

    emit(RpcEvent::Log {
        id: id.into(),
        message: format!(
            "whisper.cpp stdout: {}",
            stdout.chars().take(500).collect::<String>()
        ),
    });
    emit(RpcEvent::Log {
        id: id.into(),
        message: format!("whisper.cpp stderr: {}", stderr_buf.head_chars(500)),
    });

    if !status.success() {
        return Err(anyhow::anyhow!(
            "whisper.cpp failed with status {}: {}",
            status,
            stderr_buf.error_summary(2000)
        ));
    }

    emit(RpcEvent::Log {
        id: id.into(),
        message: "Parsing whisper.cpp output...".into(),
    });

    // whisper.cpp creates a JSON file next to the audio file
    let json_file_path = format!("{}.json", audio_path);

    emit(RpcEvent::Log {
        id: id.into(),
        message: format!("Looking for JSON output at: {}", json_file_path),
    });

    // Check if file exists before trying to read
    if !std::path::Path::new(&json_file_path).exists() {
        emit(RpcEvent::Log {
            id: id.into(),
            message: format!("JSON file does not exist at: {}", json_file_path),
        });

        // List files in the directory to see what was actually created
        if let Some(parent_dir) = std::path::Path::new(audio_path).parent() {
            if let Ok(entries) = std::fs::read_dir(parent_dir) {
                let files: Vec<String> = entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .collect();
                emit(RpcEvent::Log {
                    id: id.into(),
                    message: format!("Files in directory: {:?}", files),
                });
            }
        }

        return Err(anyhow::anyhow!(
            "whisper.cpp did not create expected JSON output file: {}",
            json_file_path
        ));
    }

    let json_content = std::fs::read_to_string(&json_file_path)
        .map_err(|e| anyhow::anyhow!("Failed to read whisper.cpp JSON output: {}", e))?;

    // Debug: Log first 1000 chars of JSON to understand structure
    emit(RpcEvent::Log {
        id: id.into(),
        message: format!(
            "whisper.cpp JSON preview: {}",
            json_content.chars().take(1000).collect::<String>()
        ),
    });

    // Parse the JSON output from file
    let whisper_response = parse_whisper_cpp_output(&json_content)?;

    emit(RpcEvent::Log {
        id: id.into(),
        message: format!(
            "Local whisper.cpp transcription completed. Duration: {:.2}s, Segments: {}, Words: {}",
            whisper_response.duration.unwrap_or(0.0),
            whisper_response
                .segments
                .as_ref()
                .map(|s| s.len())
                .unwrap_or(0),
            whisper_response
                .words
                .as_ref()
                .map(|w| w.len())
                .unwrap_or(0)
        ),
    });

    Ok(whisper_response)
}

/// Ensure whisper model exists with intelligent fallbacks
async fn ensure_whisper_model(model: &str) -> anyhow::Result<(String, String)> {
    // Define fallback chain: requested -> base -> tiny
    let fallback_chain = match model {
        "turbo" => vec!["turbo", "large", "medium", "base", "tiny"],
        "large" => vec!["large", "medium", "base", "tiny"],
        "medium" => vec!["medium", "base", "tiny"],
        "small" => vec!["small", "base", "tiny"],
        "base" => vec!["base", "tiny"],
        "tiny" => vec!["tiny"],
        _ => vec!["base", "tiny"], // Unknown models fallback to base then tiny
    };

    for &fallback_model in &fallback_chain {
        let model_filename = match fallback_model {
            "tiny" => "ggml-tiny.bin",
            "base" => "ggml-base.bin",
            "small" => "ggml-small.bin",
            "medium" => "ggml-medium.bin",
            "large" => "ggml-large-v3.bin",
            "turbo" => "ggml-large-v3-turbo.bin",
            _ => continue,
        };

        // Check if model exists using the centralized models directory function
        // This handles dev, production, and all platform-specific paths
        if let Ok(models_dir) = get_models_dir() {
            let model_path = models_dir.join(model_filename);
            if model_path.exists() {
                return Ok((
                    model_path.to_string_lossy().to_string(),
                    fallback_model.to_string(),
                ));
            }
        }

        // Also check system-wide locations as fallback
        let system_paths = vec![
            format!("/opt/homebrew/share/whisper-models/{}", model_filename),
            format!(
                "{}/.cache/whisper/{}",
                std::env::var("HOME").unwrap_or_default(),
                model_filename
            ),
        ];

        for path in system_paths {
            if std::path::Path::new(&path).exists() {
                return Ok((path, fallback_model.to_string()));
            }
        }
    }

    // No models found locally - this will trigger OpenAI API fallback at higher level
    Err(anyhow::anyhow!(
        "No whisper models found locally. Tried fallback chain: {:?}",
        fallback_chain
    ))
}

/// Returns true if an executable at `path` can run on the current machine.
///
/// On macOS this rejects Mach-O binaries built for the other CPU
/// architecture — e.g. an x86_64 ffmpeg bundled into the app on an arm64 Mac
/// that does not have Rosetta. This makes the binary search fall through to a
/// runnable build (system ffmpeg, correct-arch bundle, etc.) instead of
/// failing with "Bad CPU type in executable".
pub fn binary_runnable(path: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        use std::io::Read;

        let mut file = match std::fs::File::open(path) {
            Ok(f) => f,
            Err(_) => return false,
        };

        // Mach-O header prefix: magic (4 bytes) + cputype (4 bytes).
        let mut hdr = [0u8; 8];
        if file.read_exact(&mut hdr).is_err() {
            // Not a readable binary (script, dir, etc.) — let it run/fail naturally.
            return true;
        }

        let magic = u32::from_le_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]);
        // Fat/universal binaries contain all architectures.
        if matches!(magic, 0xcafe_babe | 0xbeba_feca | 0xcafe_babf | 0xbfba_feca) {
            return true;
        }
        // Anything that isn't a thin Mach-O is accepted as-is.
        if !matches!(magic, 0xfeed_facf | 0xcefa_edfe) {
            return true;
        }

        let cputype = u32::from_le_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]);
        const CPU_TYPE_ARM64: u32 = 0x0100_000c;
        const CPU_TYPE_X86_64: u32 = 0x0100_0007;

        if cputype == CPU_TYPE_ARM64 {
            return cfg!(target_arch = "aarch64");
        }
        if cputype == CPU_TYPE_X86_64 {
            return cfg!(target_arch = "x86_64");
        }
        true
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        true
    }
}

/// Find whisper.cpp binary across different locations and platforms
pub async fn find_whisper_binary() -> anyhow::Result<String> {
    if let Some(cached) = CACHED_WHISPER_PATH.get() {
        return match cached {
            Some(p) => Ok(p.clone()),
            None => Err(anyhow::anyhow!(
                "whisper.cpp binary not found in any location"
            )),
        };
    }

    let result = (|| {
        // Priority order:
        // 0. Explicit override (WHISPER_CLI_PATH) — what a packaged app's
        //    launcher sets, since its own bundle layout is the one thing it
        //    actually knows for certain, rather than something this process
        //    has to guess at via its own exe path.
        // 1. Bundled binary (next to executable)
        // 2. Project binary (for development)
        // 3. System installation (Homebrew, etc.)

        if let Ok(path) = std::env::var("WHISPER_CLI_PATH") {
            let p = std::path::Path::new(&path);
            if p.exists() && binary_runnable(p) {
                return Some(path);
            }
        }

        // Try to get the directory where the current executable is located
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                let bundled_paths = get_bundled_whisper_paths(exe_dir);
                for path in bundled_paths {
                    if path.exists() && binary_runnable(&path) {
                        return Some(path.to_string_lossy().to_string());
                    }
                }
            }
        }

        // Try project directory (for development)
        let project_paths = get_project_whisper_paths();
        for path in project_paths {
            if path.exists() && binary_runnable(&path) {
                return Some(path.to_string_lossy().to_string());
            }
        }

        // Try system installations
        let system_paths = get_system_whisper_paths();
        for path in system_paths {
            if let Ok(which_path) = which::which(&path) {
                if binary_runnable(&which_path) {
                    return Some(which_path.to_string_lossy().to_string());
                }
            }
        }

        None
    })();

    let _ = CACHED_WHISPER_PATH.set(result.clone());
    result.ok_or_else(|| anyhow::anyhow!("whisper.cpp binary not found in any location"))
}

/// Get possible bundled whisper binary paths (next to executable)
fn get_bundled_whisper_paths(exe_dir: &std::path::Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    #[cfg(target_os = "macos")]
    {
        if cfg!(target_arch = "aarch64") {
            paths.push(exe_dir.join("bin/whisper-cli-macos-arm64")); // Check bin subdirectory first
            paths.push(exe_dir.join("whisper-cli-macos-arm64"));
            paths.push(exe_dir.join("bin/whisper-macos-arm64"));
            paths.push(exe_dir.join("whisper-macos-arm64"));
        } else {
            paths.push(exe_dir.join("bin/whisper-cli-macos-x64")); // Check bin subdirectory first
            paths.push(exe_dir.join("whisper-cli-macos-x64"));
            paths.push(exe_dir.join("bin/whisper-macos-x64"));
            paths.push(exe_dir.join("whisper-macos-x64"));
        }
        paths.push(exe_dir.join("bin/whisper-cli")); // Check bin subdirectory first
        paths.push(exe_dir.join("whisper-cli"));
        paths.push(exe_dir.join("bin/whisper")); // Check bin subdirectory first
        paths.push(exe_dir.join("whisper"));
    }

    #[cfg(target_os = "windows")]
    {
        // Check bin-win directory for Windows-specific binaries (from electron-builder)
        paths.push(exe_dir.join("bin-win/whisper-cli.exe"));
        paths.push(exe_dir.join("bin-win/whisper.exe"));
        // Fallback to bin directory
        paths.push(exe_dir.join("bin/whisper.exe"));
        paths.push(exe_dir.join("whisper.exe"));
        paths.push(exe_dir.join("bin/whisper-cli.exe"));
        paths.push(exe_dir.join("whisper-cli.exe"));
        if cfg!(target_arch = "x86_64") {
            paths.push(exe_dir.join("bin/whisper-win-x64.exe"));
            paths.push(exe_dir.join("whisper-win-x64.exe"));
        }
    }

    #[cfg(target_os = "linux")]
    {
        // Check bin-linux directory for Linux-specific binaries (from electron-builder)
        paths.push(exe_dir.join("bin-linux/whisper-cli"));
        paths.push(exe_dir.join("bin-linux/whisper"));
        if cfg!(target_arch = "x86_64") {
            paths.push(exe_dir.join("bin-linux/whisper-linux-x64"));
            paths.push(exe_dir.join("bin/whisper-linux-x64")); // Check bin subdirectory first
            paths.push(exe_dir.join("whisper-linux-x64"));
        } else if cfg!(target_arch = "aarch64") {
            paths.push(exe_dir.join("bin-linux/whisper-linux-arm64"));
            paths.push(exe_dir.join("bin/whisper-linux-arm64")); // Check bin subdirectory first
            paths.push(exe_dir.join("whisper-linux-arm64"));
        }
        paths.push(exe_dir.join("bin/whisper-cli")); // Check bin subdirectory first
        paths.push(exe_dir.join("whisper-cli"));
        paths.push(exe_dir.join("bin/whisper")); // Check bin subdirectory first
        paths.push(exe_dir.join("whisper"));
    }

    paths
}

/// Get possible project whisper binary paths (for development)
fn get_project_whisper_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // Project bin directory (only used in development mode)
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let bin_dir = project_root.join("bin");

    // Only add paths if the development directory exists
    if !bin_dir.exists() {
        return paths;
    }

    #[cfg(target_os = "macos")]
    {
        if cfg!(target_arch = "aarch64") {
            paths.push(bin_dir.join("whisper-cli-macos-arm64"));
        } else {
            paths.push(bin_dir.join("whisper-cli-macos-x64"));
        }
        paths.push(bin_dir.join("whisper-cli"));
    }

    #[cfg(target_os = "windows")]
    {
        paths.push(bin_dir.join("whisper.exe"));
        paths.push(bin_dir.join("whisper-cli.exe"));
    }

    #[cfg(target_os = "linux")]
    {
        paths.push(bin_dir.join("whisper-linux-x64"));
        paths.push(bin_dir.join("whisper-cli"));
    }

    paths
}

/// Get possible system whisper binary paths
fn get_system_whisper_paths() -> Vec<String> {
    vec![
        "whisper-cli".to_string(),
        "whisper".to_string(),
        "/opt/homebrew/bin/whisper-cli".to_string(),
        "/usr/local/bin/whisper-cli".to_string(),
        "/usr/bin/whisper-cli".to_string(),
    ]
}

/// Find FFmpeg binary using priority order (bundled > project > system)
pub async fn find_ffmpeg_binary() -> anyhow::Result<String> {
    if let Some(cached) = CACHED_FFMPEG_PATH.get() {
        return match cached {
            Some(p) => Ok(p.clone()),
            None => Err(anyhow::anyhow!("FFmpeg binary not found in any location")),
        };
    }

    let result = (|| {
        // Allow override via environment
        if let Ok(path) = std::env::var("FFMPEG_PATH") {
            let p = std::path::Path::new(&path);
            if p.exists() && binary_runnable(p) {
                return Some(path);
            }
        }

        // Try bundled binary first (next to executable)
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                let bundled_paths = get_bundled_ffmpeg_paths(exe_dir);
                for path in bundled_paths {
                    if path.exists() && binary_runnable(&path) {
                        return Some(path.to_string_lossy().to_string());
                    }
                }
            }
        }

        // Try project directory (for development)
        let project_paths = get_project_ffmpeg_paths();
        for path in project_paths {
            if path.exists() && binary_runnable(&path) {
                return Some(path.to_string_lossy().to_string());
            }
        }

        // Try system installations
        let system_paths = get_system_ffmpeg_paths();
        for path in system_paths {
            if let Ok(which_path) = which::which(&path) {
                if binary_runnable(&which_path) {
                    return Some(which_path.to_string_lossy().to_string());
                }
            }
        }

        None
    })();

    let _ = CACHED_FFMPEG_PATH.set(result.clone());
    result.ok_or_else(|| anyhow::anyhow!("FFmpeg binary not found in any location"))
}

/// Find ffprobe binary using priority order (bundled > project > system)
pub async fn find_ffprobe_binary() -> anyhow::Result<String> {
    if let Some(cached) = CACHED_FFPROBE_PATH.get() {
        return match cached {
            Some(p) => Ok(p.clone()),
            None => Err(anyhow::anyhow!("ffprobe binary not found in any location")),
        };
    }

    let result = (|| {
        // Allow override via environment
        if let Ok(path) = std::env::var("FFPROBE_PATH") {
            let p = std::path::Path::new(&path);
            if p.exists() && binary_runnable(p) {
                return Some(path);
            }
        }

        // Try bundled binary first (next to executable)
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                let bundled_paths = get_bundled_ffprobe_paths(exe_dir);
                for path in bundled_paths {
                    if path.exists() && binary_runnable(&path) {
                        return Some(path.to_string_lossy().to_string());
                    }
                }
            }
        }

        // Try project directory (for development)
        let project_paths = get_project_ffprobe_paths();
        for path in project_paths {
            if path.exists() && binary_runnable(&path) {
                return Some(path.to_string_lossy().to_string());
            }
        }

        // Try system installations
        let system_paths = get_system_ffprobe_paths();
        for path in system_paths {
            if let Ok(which_path) = which::which(&path) {
                if binary_runnable(&which_path) {
                    return Some(which_path.to_string_lossy().to_string());
                }
            }
        }

        None
    })();

    let _ = CACHED_FFPROBE_PATH.set(result.clone());
    result.ok_or_else(|| anyhow::anyhow!("ffprobe binary not found in any location"))
}

/// Get possible bundled FFmpeg binary paths (next to executable)
fn get_bundled_ffmpeg_paths(exe_dir: &std::path::Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    #[cfg(target_os = "macos")]
    {
        paths.push(exe_dir.join("bin/ffmpeg")); // Check bin subdirectory first
        paths.push(exe_dir.join("ffmpeg"));
    }

    #[cfg(target_os = "windows")]
    {
        paths.push(exe_dir.join("bin/ffmpeg.exe")); // Check bin subdirectory first
        paths.push(exe_dir.join("ffmpeg.exe"));
    }

    #[cfg(target_os = "linux")]
    {
        // Check bin-linux directory for Linux-specific binaries (from electron-builder)
        paths.push(exe_dir.join("bin-linux/ffmpeg"));
        paths.push(exe_dir.join("bin/ffmpeg")); // Check bin subdirectory first
        paths.push(exe_dir.join("ffmpeg"));
    }

    paths
}

/// Get possible project FFmpeg binary paths
fn get_project_ffmpeg_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    // Use proper Cargo manifest directory to find bin folder (same as whisper paths)
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let bin_dir = project_root.join("bin");

    #[cfg(target_os = "macos")]
    {
        paths.push(bin_dir.join("ffmpeg"));
    }

    #[cfg(target_os = "windows")]
    {
        paths.push(bin_dir.join("ffmpeg.exe"));
    }

    #[cfg(target_os = "linux")]
    {
        paths.push(bin_dir.join("ffmpeg"));
    }

    paths
}

/// Get possible system FFmpeg binary paths
fn get_system_ffmpeg_paths() -> Vec<String> {
    vec![
        "ffmpeg".to_string(),
        "/opt/homebrew/bin/ffmpeg".to_string(),
        "/usr/local/bin/ffmpeg".to_string(),
        "/usr/bin/ffmpeg".to_string(),
    ]
}

/// Get possible bundled ffprobe binary paths (next to executable)
fn get_bundled_ffprobe_paths(exe_dir: &std::path::Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    #[cfg(target_os = "macos")]
    {
        paths.push(exe_dir.join("bin/ffprobe")); // Check bin subdirectory first
        paths.push(exe_dir.join("ffprobe"));
    }

    #[cfg(target_os = "windows")]
    {
        paths.push(exe_dir.join("bin/ffprobe.exe")); // Check bin subdirectory first
        paths.push(exe_dir.join("ffprobe.exe"));
    }

    #[cfg(target_os = "linux")]
    {
        // Check bin-linux directory for Linux-specific binaries (from electron-builder)
        paths.push(exe_dir.join("bin-linux/ffprobe"));
        paths.push(exe_dir.join("bin/ffprobe")); // Check bin subdirectory first
        paths.push(exe_dir.join("ffprobe"));
    }

    paths
}

/// Get possible project ffprobe binary paths
fn get_project_ffprobe_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    // Use proper Cargo manifest directory to find bin folder (same as whisper paths)
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let bin_dir = project_root.join("bin");

    #[cfg(target_os = "macos")]
    {
        paths.push(bin_dir.join("ffprobe"));
    }

    #[cfg(target_os = "windows")]
    {
        paths.push(bin_dir.join("ffprobe.exe"));
    }

    #[cfg(target_os = "linux")]
    {
        paths.push(bin_dir.join("ffprobe"));
    }

    paths
}

/// Get possible system ffprobe binary paths
fn get_system_ffprobe_paths() -> Vec<String> {
    vec![
        "ffprobe".to_string(),
        "/opt/homebrew/bin/ffprobe".to_string(),
        "/usr/local/bin/ffprobe".to_string(),
        "/usr/bin/ffprobe".to_string(),
    ]
}

/// Get download URL for whisper model
fn get_model_download_url(model_filename: &str) -> String {
    format!(
        "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{}",
        model_filename
    )
}

/// Public RPC method to download a whisper model with progress reporting
pub async fn download_model_rpc(
    id: &str,
    params: crate::types::DownloadModelParams,
    mut emit: impl FnMut(crate::rpc::RpcEvent),
) -> anyhow::Result<crate::types::DownloadModelResult> {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    let model_filename = match params.model.as_str() {
        "tiny" => "ggml-tiny.bin",
        "base" => "ggml-base.bin",
        "small" => "ggml-small.bin",
        "medium" => "ggml-medium.bin",
        "large" => "ggml-large-v3.bin",
        "turbo" => "ggml-large-v3-turbo.bin",
        _ => {
            return Err(anyhow::anyhow!(
                "Unknown model: {}. Supported: tiny, base, small, medium, large, turbo",
                params.model
            ))
        }
    };

    let url = get_model_download_url(model_filename);
    let models_dir = get_models_dir().map_err(|e| {
        anyhow::anyhow!(
            "Cannot access models directory: {}. Please check app permissions.",
            e
        )
    })?;
    let output_path = models_dir.join(model_filename);

    emit(crate::rpc::RpcEvent::Log {
        id: id.into(),
        message: format!("Models will be saved to: {}", models_dir.display()),
    });

    emit(crate::rpc::RpcEvent::Log {
        id: id.into(),
        message: format!(
            "Starting download of {} model from HuggingFace",
            params.model
        ),
    });

    // Download with progress
    let client = reqwest::Client::new();
    let response = client.get(&url).send().await?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Failed to download model: HTTP {}",
            response.status()
        ));
    }

    let total_size = response.content_length().unwrap_or(0);

    emit(crate::rpc::RpcEvent::Log {
        id: id.into(),
        message: format!(
            "Downloading {} ({:.1} MB)...",
            model_filename,
            total_size as f64 / 1024.0 / 1024.0
        ),
    });

    let mut file = tokio::fs::File::create(&output_path).await
        .map_err(|e| anyhow::anyhow!("Cannot create model file at {}: {}. Check app permissions in System Settings > Privacy & Security.", output_path.display(), e))?;
    let mut downloaded = 0u64;
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;

        let progress = if total_size > 0 {
            (downloaded as f64 / total_size as f64) as f32
        } else {
            0.0_f32
        };

        emit(crate::rpc::RpcEvent::Progress {
            id: id.into(),
            status: format!("Downloading {}...", params.model),
            progress,
        });
    }

    file.flush().await?;

    emit(crate::rpc::RpcEvent::Log {
        id: id.into(),
        message: format!(
            "Successfully downloaded {} model to {}",
            params.model,
            output_path.display()
        ),
    });

    Ok(crate::types::DownloadModelResult {
        model: params.model,
        path: output_path.to_string_lossy().to_string(),
        size: downloaded,
    })
}

/// Check if a model exists
pub fn check_model_exists(model_name: &str) -> anyhow::Result<bool> {
    let model_filename = match model_name {
        "tiny" => "ggml-tiny.bin",
        "base" => "ggml-base.bin",
        "small" => "ggml-small.bin",
        "medium" => "ggml-medium.bin",
        "large" => "ggml-large-v3.bin",
        "turbo" => "ggml-large-v3-turbo.bin",
        _ => return Ok(false),
    };

    let models_dir = get_models_dir().map_err(|e| {
        anyhow::anyhow!(
            "Cannot access models directory: {}. Please check app permissions.",
            e
        )
    })?;
    let model_path = models_dir.join(model_filename);
    Ok(model_path.exists())
}

/// Get the models directory path
fn get_models_dir() -> anyhow::Result<std::path::PathBuf> {
    // Priority 1: Check if we're in development (project exists)
    let dev_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("models");
    if dev_path.exists() && dev_path.is_dir() {
        return Ok(dev_path);
    }

    // Priority 2: Production - use Application Support directory (standard macOS location)
    #[cfg(target_os = "macos")]
    {
        if let Some(home_dir) = std::env::var_os("HOME") {
            let app_support = std::path::PathBuf::from(home_dir)
                .join("Library/Application Support/CapSlap/models");
            std::fs::create_dir_all(&app_support).map_err(|e| {
                anyhow::anyhow!(
                    "Failed to create models directory at {}: {}",
                    app_support.display(),
                    e
                )
            })?;
            return Ok(app_support);
        }
    }

    // Priority 3: Check bundled resources (app bundle)
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            // For macOS app bundle: Contents/MacOS/../Resources/models/
            if let Some(contents_dir) = exe_dir.parent() {
                let bundled_models = contents_dir.join("Resources/models");
                if bundled_models.exists() {
                    return Ok(bundled_models);
                }
            }
        }
    }

    // Priority 4: Windows/Linux - use local app data
    #[cfg(target_os = "windows")]
    {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            let models_dir = std::path::PathBuf::from(appdata).join("CapSlap/models");
            std::fs::create_dir_all(&models_dir)?;
            return Ok(models_dir);
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(home_dir) = std::env::var_os("HOME") {
            let models_dir = std::path::PathBuf::from(home_dir).join(".local/share/capslap/models");
            std::fs::create_dir_all(&models_dir)?;
            return Ok(models_dir);
        }
    }

    // Fallback: current directory
    let fallback = std::path::PathBuf::from("./models");
    std::fs::create_dir_all(&fallback)?;
    Ok(fallback)
}

/// Parse whisper.cpp JSON output and convert to WhisperResponse
fn parse_whisper_cpp_output(json_output: &str) -> anyhow::Result<WhisperResponse> {
    let json: serde_json::Value = serde_json::from_str(json_output)?;

    let mut full_text = String::new();
    let mut segments = Vec::new();
    let mut words = Vec::new();
    let mut duration = 0.0f64;

    if let Some(transcription) = json.get("transcription") {
        if let Some(array) = transcription.as_array() {
            for (i, segment) in array.iter().enumerate() {
                if let (Some(start), Some(end), Some(text)) = (
                    segment
                        .get("offsets")
                        .and_then(|o| o.get("from"))
                        .and_then(|f| f.as_f64()),
                    segment
                        .get("offsets")
                        .and_then(|o| o.get("to"))
                        .and_then(|t| t.as_f64()),
                    segment.get("text").and_then(|t| t.as_str()),
                ) {
                    let start_sec = start / 1000.0; // Convert ms to seconds
                    let end_sec = end / 1000.0;

                    full_text.push_str(text);
                    full_text.push(' ');

                    if end_sec > duration {
                        duration = end_sec;
                    }

                    segments.push(crate::types::WhisperSegment {
                        id: i as u32,
                        start: start_sec,
                        end: end_sec,
                        text: text.trim().to_string(),
                    });

                    // TEMPORARILY DISABLE TOKEN PARSING - use only segment-level timing
                    // This fixes sync issues with whisper.cpp tokens
                    /*
                    let tokens_array = segment.get("tokens")
                        .and_then(|t| t.as_array())
                        .or_else(|| segment.get("words").and_then(|w| w.as_array()));

                    if let Some(tokens) = tokens_array {
                        for token in tokens {
                            // Try different JSON structures for token timing
                            let (token_text, token_start, token_end) = if let (Some(text), Some(start), Some(end)) = (
                                token.get("text").and_then(|t| t.as_str()),
                                token.get("offsets").and_then(|o| o.get("from")).and_then(|f| f.as_f64()),
                                token.get("offsets").and_then(|o| o.get("to")).and_then(|t| t.as_f64()),
                            ) {
                                (text, start, end)
                            } else if let (Some(text), Some(start), Some(end)) = (
                                token.get("word").and_then(|t| t.as_str()),
                                token.get("start").and_then(|s| s.as_f64()),
                                token.get("end").and_then(|e| e.as_f64()),
                            ) {
                                // Alternative JSON format: direct start/end fields in seconds
                                (text, start * 1000.0, end * 1000.0) // Convert to ms for consistency
                            } else {
                                continue; // Skip if we can't parse this token
                            };

                            // Skip special tokens like [_BEG_] and empty/whitespace-only tokens
                            let token_text_trimmed = token_text.trim();
                            if !token_text_trimmed.is_empty()
                                && !token_text_trimmed.starts_with('[')
                                && !token_text_trimmed.ends_with(']')
                                && token_start < token_end {

                                words.push(crate::types::WhisperWord {
                                    word: token_text_trimmed.to_string(),
                                    start: token_start / 1000.0, // Convert ms to seconds
                                    end: token_end / 1000.0,
                                });
                            }
                        }
                    }
                    */

                    // Parse word-level timestamps from tokens array
                    // Prioritize "words" (full words) over "tokens" (sub-word tokens)
                    // Parse word-level timestamps from tokens array
                    // Prioritize "words" (full words) over "tokens" (sub-word tokens)

                    let (tokens_array, is_subword_tokens) =
                        if let Some(arr) = segment.get("words").and_then(|w| w.as_array()) {
                            (Some(arr), false)
                        } else if let Some(arr) = segment.get("tokens").and_then(|t| t.as_array()) {
                            (Some(arr), true)
                        } else {
                            (None, false)
                        };

                    if let Some(tokens) = tokens_array {
                        let mut current_merged_word: Option<crate::types::WhisperWord> = None;

                        for token in tokens {
                            // Try different JSON structures for token timing
                            let (token_text, token_start, token_end) =
                                if let (Some(text), Some(start), Some(end)) = (
                                    token.get("text").and_then(|t| t.as_str()),
                                    token
                                        .get("offsets")
                                        .and_then(|o| o.get("from"))
                                        .and_then(|f| f.as_f64()),
                                    token
                                        .get("offsets")
                                        .and_then(|o| o.get("to"))
                                        .and_then(|t| t.as_f64()),
                                ) {
                                    (text, start, end)
                                } else if let (Some(text), Some(start), Some(end)) = (
                                    token.get("word").and_then(|t| t.as_str()),
                                    token.get("start").and_then(|s| s.as_f64()),
                                    token.get("end").and_then(|e| e.as_f64()),
                                ) {
                                    // Alternative JSON format: direct start/end fields in seconds
                                    (text, start * 1000.0, end * 1000.0) // Convert to ms for consistency
                                } else {
                                    continue; // Skip if we can't parse this token
                                };

                            let token_start_sec = token_start / 1000.0;
                            let token_end_sec = token_end / 1000.0;

                            // Skip special tokens like [_BEG_]
                            if token_text.starts_with('[') && token_text.ends_with(']') {
                                continue;
                            }

                            if is_subword_tokens {
                                // Merge logic for BPE tokens
                                let starts_with_space = token_text.starts_with(' ');
                                let _is_punctuation = token_text.trim().len() == 1
                                    && token_text
                                        .trim()
                                        .chars()
                                        .next()
                                        .unwrap()
                                        .is_ascii_punctuation();

                                // Decision: Is this a new word?
                                // Standard Whisper: new word starts with space.
                                // But also check if we have no current word.

                                if starts_with_space || current_merged_word.is_none() {
                                    // Flush previous word if exists
                                    if let Some(mut w) = current_merged_word.take() {
                                        w.word = w.word.trim().to_string();
                                        if !w.word.is_empty() {
                                            words.push(w);
                                        }
                                    }

                                    // Start new word. Merging is per segment, so
                                    // a piece opening a segment without a leading
                                    // space is really the tail of the last word of
                                    // the segment before it — remember that, or
                                    // the two halves get drawn with a space
                                    // between them.
                                    current_merged_word = Some(crate::types::WhisperWord {
                                        word: token_text.to_string(),
                                        start: token_start_sec,
                                        end: token_end_sec,
                                        glue_to_previous: !starts_with_space,
                                    });
                                } else {
                                    // Append to current word
                                    // Punctuation often attaches to previous word even without space
                                    // or middle of word parts
                                    if let Some(w) = current_merged_word.as_mut() {
                                        w.word.push_str(token_text);
                                        w.end = token_end_sec; // Extend duration
                                    }
                                }
                            } else {
                                // Direct words mode (already full words)
                                let token_text_trimmed = token_text.trim();
                                if !token_text_trimmed.is_empty() && token_start < token_end {
                                    words.push(crate::types::WhisperWord {
                                        word: token_text_trimmed.to_string(),
                                        start: token_start_sec,
                                        end: token_end_sec,
                                        glue_to_previous: false,
                                    });
                                }
                            }
                        }

                        // Flush last merged word
                        if let Some(mut w) = current_merged_word.take() {
                            w.word = w.word.trim().to_string();
                            if !w.word.is_empty() {
                                words.push(w);
                            }
                        }
                    }
                }
            }
        }
    }

    full_text = full_text.trim().to_string();

    if full_text.is_empty() {
        return Err(anyhow::anyhow!(
            "No transcription text found in whisper.cpp output"
        ));
    }

    let response = WhisperResponse {
        task: Some("transcribe".to_string()),
        language: None,
        duration: Some(duration),
        text: full_text,
        segments: Some(segments.clone()),
        words: if words.is_empty() {
            None
        } else {
            Some(words.clone())
        },
    };

    Ok(response)
}

/// Transcribe audio using local FFmpeg Whisper (requires FFmpeg 8.0+)
pub async fn transcribe_with_ffmpeg_whisper(
    id: &str,
    audio_path: &str,
    model: Option<String>,
    language: Option<String>,
    mut emit: impl FnMut(RpcEvent),
) -> anyhow::Result<WhisperResponse> {
    let whisper_model = model.unwrap_or_else(|| "medium".to_string());

    emit(RpcEvent::Log {
        id: id.into(),
        message: format!(
            "Starting local FFmpeg Whisper transcription with model: {}",
            whisper_model
        ),
    });

    let ffmpeg_path = find_ffmpeg_binary()
        .await
        .map_err(|e| anyhow::anyhow!("FFmpeg not found: {}", e))?;
    let mut cmd = TokioCommand::new(ffmpeg_path);
    cmd.kill_on_drop(true);
    cmd.arg("-y") // overwrite output
        .arg("-i")
        .arg(audio_path)
        .arg("-af");

    // Build whisper filter arguments
    let mut whisper_filter = format!("whisper=model={}:print_text=1", whisper_model);

    if let Some(lang) = &language {
        whisper_filter.push_str(&format!(":language={}", lang));
    }

    cmd.arg(whisper_filter)
        .arg("-f")
        .arg("null")
        .arg("-")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    emit(RpcEvent::Log {
        id: id.into(),
        message: "Running FFmpeg Whisper transcription...".into(),
    });

    let output = cmd.output().await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("FFmpeg Whisper failed: {}", stderr));
    }

    let stderr = String::from_utf8_lossy(&output.stderr);

    emit(RpcEvent::Log {
        id: id.into(),
        message: "Parsing FFmpeg Whisper output...".into(),
    });

    // Parse the whisper output from stderr
    let whisper_response = parse_ffmpeg_whisper_output(&stderr)?;

    emit(RpcEvent::Log {
        id: id.into(),
        message: format!(
            "Local FFmpeg Whisper transcription completed. Duration: {:.2}s",
            whisper_response.duration.unwrap_or(0.0)
        ),
    });

    Ok(whisper_response)
}

/// Parse FFmpeg Whisper output from stderr and convert to WhisperResponse
fn parse_ffmpeg_whisper_output(stderr: &str) -> anyhow::Result<WhisperResponse> {
    // FFmpeg Whisper outputs text with timestamps in stderr
    // Format example: "[00:00.000 --> 00:05.000]  Hello world"

    let mut full_text = String::new();
    let mut segments = Vec::new();
    let mut duration = 0.0f64;

    // Regex to match whisper output lines with timestamps
    let re = Regex::new(r"\[(\d{2}):(\d{2})\.(\d{3}) --> (\d{2}):(\d{2})\.(\d{3})\]\s*(.+)")?;

    for line in stderr.lines() {
        if let Some(caps) = re.captures(line) {
            // Parse start time
            let start_min: f64 = caps[1].parse()?;
            let start_sec: f64 = caps[2].parse()?;
            let start_ms: f64 = caps[3].parse()?;
            let start = start_min * 60.0 + start_sec + start_ms / 1000.0;

            // Parse end time
            let end_min: f64 = caps[4].parse()?;
            let end_sec: f64 = caps[5].parse()?;
            let end_ms: f64 = caps[6].parse()?;
            let end = end_min * 60.0 + end_sec + end_ms / 1000.0;

            let text = caps[7].trim().to_string();

            if !text.is_empty() {
                full_text.push_str(&text);
                full_text.push(' ');

                // Update duration
                if end > duration {
                    duration = end;
                }

                // Create segment (simplified - FFmpeg doesn't provide word-level timing by default)
                segments.push(crate::types::WhisperSegment {
                    id: segments.len() as u32,
                    start,
                    end,
                    text: text.clone(),
                });
            }
        }
    }

    // Trim final space
    full_text = full_text.trim().to_string();

    if full_text.is_empty() {
        return Err(anyhow::anyhow!(
            "No transcription text found in FFmpeg output"
        ));
    }

    Ok(WhisperResponse {
        task: Some("transcribe".to_string()),
        language: None, // FFmpeg doesn't always report detected language
        duration: Some(duration),
        text: full_text,
        segments: Some(segments),
        words: None, // Word-level timing not available by default in FFmpeg Whisper
    })
}

/// Helper function to create transcription result with JSON file generation
async fn create_transcription_result(
    id: &str,
    segments: &[CaptionSegment],
    whisper_response: &WhisperResponse,
    params: &TranscribeSegmentsParams,
    temp_dir: Option<&std::path::PathBuf>,
) -> anyhow::Result<TranscribeSegmentsResult> {
    use tokio::fs;

    // Generate JSON file path based on temp directory (or video file location if no temp dir)
    let json_path = if let Some(temp_dir) = temp_dir {
        let json_filename = format!("transcription_{}.json", id);
        temp_dir.join(json_filename).to_string_lossy().to_string()
    } else {
        let base_path = if let Some(ref video_file) = params.video_file {
            std::path::Path::new(video_file)
        } else {
            std::path::Path::new(&params.audio)
        };
        let mut json_path = base_path.to_path_buf();
        json_path.set_extension("json");
        json_path.to_string_lossy().to_string()
    };

    // Create JSON export data
    let json_data = serde_json::json!({
        "segments": segments,
        "fullText": whisper_response.text,
        "duration": whisper_response.duration,
        "splitByWords": params.split_by_words,
        "model": params.model.clone().unwrap_or_else(|| "whisper-1".to_string()),
        "language": params.language.clone(),
        "generatedAt": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    });

    let json_content = serde_json::to_string_pretty(&json_data)?;
    fs::write(&json_path, json_content).await?;

    Ok(TranscribeSegmentsResult {
        segments: segments.to_vec(),
        full_text: whisper_response.text.clone(),
        duration: whisper_response.duration,
        json_file: json_path,
    })
}

pub async fn transcribe_segments(
    id: &str,
    p: TranscribeSegmentsParams,
    emit: impl FnMut(RpcEvent),
) -> anyhow::Result<TranscribeSegmentsResult> {
    transcribe_segments_with_temp(id, p, None, emit).await
}

pub async fn transcribe_segments_with_temp(
    id: &str,
    p: TranscribeSegmentsParams,
    temp_dir: Option<&std::path::PathBuf>,
    mut emit: impl FnMut(RpcEvent),
) -> anyhow::Result<TranscribeSegmentsResult> {
    use mime_guess::MimeGuess;
    use reqwest::multipart;
    use tokio::fs;

    // QUICK SWITCH: Set to false to force OpenAI API, true for local whisper
    const USE_LOCAL_WHISPER: bool = true;

    // Check cache first
    if let Ok(Some(cached_response)) = get_cached_whisper_response(&p.audio, &p).await {
        let segments = whisper_to_caption_segments(&cached_response, p.split_by_words);

        // generate JSON file path for cached response too
        let json_path = if let Some(temp_dir) = temp_dir {
            let json_filename = format!("transcription_{}.json", id);
            temp_dir.join(json_filename).to_string_lossy().to_string()
        } else {
            let base_path = if let Some(ref video_file) = p.video_file {
                std::path::Path::new(video_file)
            } else {
                std::path::Path::new(&p.audio)
            };
            let mut json_path = base_path.to_path_buf();
            json_path.set_extension("json");
            json_path.to_string_lossy().to_string()
        };

        // save JSON file for cached response as well
        let json_data = serde_json::json!({
            "segments": segments,
            "fullText": cached_response.text,
            "duration": cached_response.duration,
            "splitByWords": p.split_by_words,
            "model": p.model.clone().unwrap_or_else(|| "whisper-1".to_string()),
            "language": p.language.clone(),
            "generatedAt": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
        });

        let json_content = serde_json::to_string_pretty(&json_data)?;
        fs::write(&json_path, json_content).await?;

        return Ok(TranscribeSegmentsResult {
            segments,
            full_text: cached_response.text,
            duration: cached_response.duration,
            json_file: json_path,
        });
    }

    // Check if user explicitly selected OpenAI API (whisper-1)
    let use_openai_directly = p.model.as_ref().map(|m| m == "whisper-1").unwrap_or(false);

    // If a remote Whisper server URL is configured, use it. This lets the app
    // use a Whisper model installed on another machine (e.g. a whisper.cpp
    // server on the local network) instead of downloading models here.
    if let Some(base_url) = p
        .whisper_base_url
        .as_deref()
        .map(|u| u.trim())
        .filter(|u| !u.is_empty())
    {
        emit(RpcEvent::Log {
            id: id.into(),
            message: format!("Using remote Whisper server at: {}", base_url),
        });

        match transcribe_with_remote_server(
            id,
            &p.audio,
            base_url,
            p.api_key.as_deref(),
            p.language.as_deref(),
            p.prompt.as_deref(),
            p.split_by_words,
            &mut emit,
        )
        .await
        {
            Ok(whisper_response) => {
                emit(RpcEvent::Log {
                    id: id.into(),
                    message: "Remote Whisper server transcription successful".into(),
                });

                let segments = whisper_to_caption_segments(&whisper_response, p.split_by_words);

                // Save to cache
                if let Err(e) = save_cached_whisper_response(&p.audio, &p, &whisper_response).await
                {
                    emit(RpcEvent::Log {
                        id: id.into(),
                        message: format!("Failed to cache remote transcription: {}", e),
                    });
                }

                return create_transcription_result(id, &segments, &whisper_response, &p, temp_dir)
                    .await;
            }
            Err(e) => {
                emit(RpcEvent::Log {
                    id: id.into(),
                    message: format!("Remote Whisper server failed: {}", e),
                });
            }
        }
    }

    // Try local whisper.cpp first if available (unless whisper-1 is explicitly selected)
    if !use_openai_directly && USE_LOCAL_WHISPER && is_whisper_cpp_available().await {
        emit(RpcEvent::Log {
            id: id.into(),
            message: "whisper.cpp detected, attempting local transcription...".into(),
        });

        match transcribe_with_whisper_cpp(
            id,
            &p.audio,
            p.model.clone(),
            p.language.clone(),
            &mut emit,
        )
        .await
        {
            Ok(whisper_response) => {
                emit(RpcEvent::Log {
                    id: id.into(),
                    message: "Local whisper.cpp transcription successful".into(),
                });

                let segments = whisper_to_caption_segments(&whisper_response, p.split_by_words);

                emit(RpcEvent::Log {
                    id: id.into(),
                    message: format!(
                        "Converted to {} caption segments (split_by_words={})",
                        segments.len(),
                        p.split_by_words
                    ),
                });

                // Save to cache
                if let Err(e) = save_cached_whisper_response(&p.audio, &p, &whisper_response).await
                {
                    emit(RpcEvent::Log {
                        id: id.into(),
                        message: format!("Failed to cache local transcription: {}", e),
                    });
                }

                // Generate JSON file and return result
                return create_transcription_result(id, &segments, &whisper_response, &p, temp_dir)
                    .await;
            }
            Err(e) => {
                let error_msg = if e.to_string().contains("No whisper models found") {
                    format!(
                        "No local whisper models available, falling back to OpenAI API. ({})",
                        e
                    )
                } else {
                    format!(
                        "Local whisper.cpp failed: {}, falling back to OpenAI API",
                        e
                    )
                };

                emit(RpcEvent::Log {
                    id: id.into(),
                    message: error_msg,
                });
            }
        }
    }

    // Try local FFmpeg Whisper as fallback (unless whisper-1 is explicitly selected)
    if !use_openai_directly && USE_LOCAL_WHISPER && is_ffmpeg_whisper_available().await {
        emit(RpcEvent::Log {
            id: id.into(),
            message: "FFmpeg Whisper detected, attempting local transcription...".into(),
        });

        match transcribe_with_ffmpeg_whisper(
            id,
            &p.audio,
            p.model.clone(),
            p.language.clone(),
            &mut emit,
        )
        .await
        {
            Ok(whisper_response) => {
                emit(RpcEvent::Log {
                    id: id.into(),
                    message: "Local FFmpeg Whisper transcription successful".into(),
                });

                let segments = whisper_to_caption_segments(&whisper_response, p.split_by_words);

                // Save to cache
                if let Err(e) = save_cached_whisper_response(&p.audio, &p, &whisper_response).await
                {
                    emit(RpcEvent::Log {
                        id: id.into(),
                        message: format!("Failed to cache local transcription: {}", e),
                    });
                }

                // Generate JSON file and return result
                return create_transcription_result(id, &segments, &whisper_response, &p, temp_dir)
                    .await;
            }
            Err(e) => {
                emit(RpcEvent::Log {
                    id: id.into(),
                    message: format!("Local FFmpeg Whisper failed: {}, falling back to API", e),
                });
            }
        }
    }

    emit(RpcEvent::Log {
        id: id.into(),
        message: "No local Whisper available, using OpenAI API".into(),
    });

    // Fallback to OpenAI API
    let api_key = p
        .api_key
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("OpenAI API key not provided"))?;
    // Always use whisper-1 for OpenAI API (local model names like "tiny" are not valid for the API)
    let model = "whisper-1".to_string();

    let bytes = fs::read(&p.audio).await?;
    let filename = std::path::Path::new(&p.audio)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let mime = MimeGuess::from_path(&p.audio).first_or_octet_stream();

    // build form for verbose_json with appropriate timestamp granularities
    let mut form = multipart::Form::new()
        .text("model", model.clone())
        .part(
            "file",
            multipart::Part::bytes(bytes.clone())
                .file_name(filename.clone())
                .mime_str(mime.as_ref())
                .unwrap(),
        )
        .text("response_format", "verbose_json".to_string());

    if let Some(lang) = &p.language {
        form = form.text("language", lang.clone());
    }
    if let Some(prompt) = &p.prompt {
        form = form.text("prompt", prompt.clone());
    }

    // set timestamp granularities based on split_by_words preference
    if p.split_by_words {
        form = form.text("timestamp_granularities[]", "word".to_string());
    } else {
        form = form.text("timestamp_granularities[]", "segment".to_string());
    }

    let client = reqwest::Client::builder()
        .user_agent("core/1.0.0")
        .build()?;

    let resp = client
        .post("https://api.openai.com/v1/audio/transcriptions")
        .header("Authorization", format!("Bearer {}", api_key))
        .multipart(form)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("OpenAI error {}: {}", status, body));
    }

    let whisper_response: WhisperResponse = resp.json().await?;

    let segments = whisper_to_caption_segments(&whisper_response, p.split_by_words);

    // Save to cache
    if let Err(e) = save_cached_whisper_response(&p.audio, &p, &whisper_response).await {
        emit(RpcEvent::Log {
            id: id.into(),
            message: format!("Failed to cache transcription: {}", e),
        });
    }

    create_transcription_result(id, &segments, &whisper_response, &p, temp_dir).await
}

/// Transcribe audio using a remote, OpenAI-compatible Whisper server
/// (e.g. a whisper.cpp `server` binary or faster-whisper-server running on
/// another machine on your network). This lets the app offload transcription
/// to a machine that already has a Whisper model installed, instead of
/// downloading a model here.
///
/// The audio is streamed directly from disk instead of loading it fully into
/// memory, which prevents OOM and avoids the "stream reading error: unexpected
/// EOF" that occurred when servers dropped the connection after receiving a
/// single large allocation.
#[allow(clippy::too_many_arguments)]
pub async fn transcribe_with_remote_server(
    id: &str,
    audio_path: &str,
    base_url: &str,
    api_key: Option<&str>,
    language: Option<&str>,
    prompt: Option<&str>,
    split_by_words: bool,
    mut emit: impl FnMut(RpcEvent),
) -> anyhow::Result<WhisperResponse> {
    use mime_guess::MimeGuess;
    use reqwest::multipart;

    emit(RpcEvent::Log {
        id: id.into(),
        message: format!("Remote Whisper server base URL: {}", base_url),
    });

    let filename = std::path::Path::new(audio_path)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let mime = MimeGuess::from_path(audio_path).first_or_octet_stream();
    let file_size = fs::metadata(audio_path).await.map(|m| m.len()).unwrap_or(0);

    emit(RpcEvent::Log {
        id: id.into(),
        message: format!(
            "Streaming {:.1} MB audio file to remote server…",
            file_size as f64 / 1_048_576.0
        ),
    });

    // Stream the file instead of loading it all into RAM. This prevents OOM
    // on large recordings and avoids server-side connection drops.
    let file = fs::File::open(audio_path)
        .await
        .map_err(|e| anyhow::anyhow!("Cannot open audio file for upload: {}", e))?;
    let stream = tokio_util::io::ReaderStream::new(file);
    let body = reqwest::Body::wrap_stream(stream);
    let file_part = multipart::Part::stream_with_length(body, file_size)
        .file_name(filename)
        .mime_str(mime.as_ref())
        .unwrap();

    let mut form = multipart::Form::new()
        .text("model", "whisper-1".to_string())
        .part("file", file_part)
        .text("response_format", "verbose_json".to_string());

    if let Some(lang) = language {
        form = form.text("language", lang.to_string());
    }
    if let Some(p) = prompt {
        form = form.text("prompt", p.to_string());
    }
    if split_by_words {
        form = form.text("timestamp_granularities[]", "word".to_string());
    } else {
        form = form.text("timestamp_granularities[]", "segment".to_string());
    }

    let url = format!("{}/v1/audio/transcriptions", base_url.trim_end_matches('/'));

    emit(RpcEvent::Log {
        id: id.into(),
        message: format!("Sending audio to: {}", url),
    });

    let client = reqwest::Client::builder()
        .user_agent("capslap/1.0")
        // 15-minute ceiling for very long recordings over the LAN.
        .timeout(std::time::Duration::from_secs(900))
        .build()?;

    let mut request = client.post(&url).multipart(form);
    if let Some(key) = api_key.filter(|k| !k.trim().is_empty()) {
        request = request.header("Authorization", format!("Bearer {}", key));
    }

    let resp = request.send().await.map_err(|e| {
        // Produce a human-readable message instead of the raw hyper error.
        if e.is_timeout() {
            anyhow::anyhow!("Remote Whisper server timed out (900 s). Is the server running at {}?", base_url)
        } else if e.is_connect() {
            anyhow::anyhow!("Cannot connect to remote Whisper server at {}. Check the URL and that the server is running.", base_url)
        } else {
            anyhow::anyhow!("Remote Whisper server request failed: {}", e)
        }
    })?;

    let status = resp.status();
    // Always collect the body as text first so we can show useful errors if
    // the server returns non-JSON (e.g. an HTML error page or a truncated body).
    let body_text = resp.text().await.map_err(|e| {
        anyhow::anyhow!(
            "Failed to read response from remote Whisper server (connection dropped?): {}",
            e
        )
    })?;

    if !status.is_success() {
        return Err(anyhow::anyhow!(
            "Remote Whisper server error {}: {}",
            status,
            &body_text[..body_text.len().min(500)]
        ));
    }

    let whisper_response: WhisperResponse = serde_json::from_str(&body_text).map_err(|e| {
        anyhow::anyhow!(
            "Remote Whisper server returned unexpected response (not valid JSON). \
             This can happen if the server closed the connection mid-transfer. \
             Parse error: {}. Body preview: {}",
            e,
            &body_text[..body_text.len().min(300)]
        )
    })?;

    emit(RpcEvent::Log {
        id: id.into(),
        message: format!(
            "Remote transcription completed. Duration: {:.2}s, Segments: {}, Words: {}",
            whisper_response.duration.unwrap_or(0.0),
            whisper_response
                .segments
                .as_ref()
                .map(|s| s.len())
                .unwrap_or(0),
            whisper_response
                .words
                .as_ref()
                .map(|w| w.len())
                .unwrap_or(0)
        ),
    });

    Ok(whisper_response)
}

fn is_digits(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

fn format_with_thousands(digits: String) -> String {
    // insert commas every 3 from right
    let mut out = String::new();
    for (cnt, ch) in digits.chars().rev().enumerate() {
        if cnt > 0 && cnt % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

/// Merge currency symbols, thousand-groups, and decimals into single tokens.
/// Handles patterns like ["$", "225", "000"] → "$225,000" and ["19", ".", "99"] → "19.99"
/// Returns (text, start_ms, end_ms) tuples ready for CaptionSegment mapping.
fn merge_numbers_and_currency(
    words: &[WhisperWord],
    max_duration_ms: Option<u64>,
) -> Vec<(String, u64, u64)> {
    let mut out = Vec::new();
    let mut i = 0usize;

    while i < words.len() {
        let cur = words[i].word.trim();
        // FIX: words[i].start is already in seconds, convert to ms properly
        let start_ms = (words[i].start * 1000.0) as u64;
        let mut end_ms = (words[i].end * 1000.0) as u64;

        if let Some(max_ms) = max_duration_ms {
            if start_ms > max_ms {
                break;
            }
            end_ms = end_ms.min(max_ms);
        }

        // Branch A: "$" prefix followed by number groups
        if cur == "$" && i + 1 < words.len() {
            let next = words[i + 1].word.trim();
            if next.len() <= 3 && is_digits(next) {
                // consume numeric groups after the "$"
                let mut j = i + 1;
                let mut groups: Vec<String> = vec![next.to_string()];
                end_ms = ((words[j].end * 1000.0) as u64).min(max_duration_ms.unwrap_or(u64::MAX));
                j += 1;

                while j < words.len() {
                    let t = words[j].word.trim();
                    if t.len() == 3 && is_digits(t) {
                        groups.push(t.to_string());
                        end_ms = ((words[j].end * 1000.0) as u64)
                            .min(max_duration_ms.unwrap_or(u64::MAX));
                        j += 1;
                    } else {
                        break;
                    }
                }

                // optional decimal part: "." + 1–2 digits
                if j + 1 < words.len()
                    && words[j].word.trim() == "."
                    && is_digits(words[j + 1].word.trim())
                    && words[j + 1].word.trim().len() <= 2
                {
                    let decimal = words[j + 1].word.trim();
                    end_ms = ((words[j + 1].end * 1000.0) as u64)
                        .min(max_duration_ms.unwrap_or(u64::MAX));
                    let merged = format!("${}.{}", format_with_thousands(groups.join("")), decimal);
                    out.push((merged, start_ms, end_ms));
                    i = j + 2;
                    continue;
                }

                // no decimals
                let merged = format!("${}", format_with_thousands(groups.join("")));
                out.push((merged, start_ms, end_ms));
                i = j;
                continue;
            }
        }

        // Branch B: plain thousand-group numbers (no "$")
        if cur.len() <= 3 && is_digits(cur) {
            let mut j = i + 1;
            let mut groups: Vec<String> = vec![cur.to_string()];

            while j < words.len() {
                let t = words[j].word.trim();
                if t.len() == 3 && is_digits(t) {
                    groups.push(t.to_string());
                    end_ms =
                        ((words[j].end * 1000.0) as u64).min(max_duration_ms.unwrap_or(u64::MAX));
                    j += 1;
                } else {
                    break;
                }
            }

            // optional decimals
            if j + 1 < words.len()
                && words[j].word.trim() == "."
                && is_digits(words[j + 1].word.trim())
                && words[j + 1].word.trim().len() <= 2
            {
                let decimal = words[j + 1].word.trim();
                end_ms =
                    ((words[j + 1].end * 1000.0) as u64).min(max_duration_ms.unwrap_or(u64::MAX));
                let merged = format!("{}.{}", format_with_thousands(groups.join("")), decimal);
                out.push((merged, start_ms, end_ms));
                i = j + 2;
                continue;
            }

            if groups.len() > 1 {
                let merged = format_with_thousands(groups.join(""));
                out.push((merged, start_ms, end_ms));
                i = j;
                continue;
            }
        }

        // Fallback: keep token as-is
        if end_ms > start_ms {
            out.push((words[i].word.trim().to_string(), start_ms, end_ms));
        }
        i += 1;
    }

    out
}

pub fn whisper_to_caption_segments(
    response: &WhisperResponse,
    split_by_words: bool,
) -> Vec<CaptionSegment> {
    let max_duration_ms = response.duration.map(|d| (d * 1000.0) as u64);

    if let Some(words) = &response.words {
        if split_by_words {
            let merged = merge_numbers_and_currency(words, max_duration_ms);

            return merged
                .into_iter()
                .filter_map(|(text, start_ms, end_ms)| {
                    if end_ms <= start_ms {
                        return None;
                    }
                    Some(CaptionSegment {
                        start_ms,
                        end_ms,
                        text: text.clone(),
                        words: vec![WordSpan {
                            start_ms,
                            end_ms,
                            text,
                            glue_to_previous: false,
                        }],
                    })
                })
                .collect();
        }
    }

    if split_by_words {
        if let Some(segments) = &response.segments {
            // Auto-split segments into words when word-level timestamps are not available
            let mut word_segments = Vec::new();

            for seg in segments {
                let start_ms = (seg.start * 1000.0) as u64;
                let end_ms = (seg.end * 1000.0) as u64;
                let segment_duration_ms = end_ms.saturating_sub(start_ms);

                // Skip segments that are beyond the actual audio duration
                if let Some(max_ms) = max_duration_ms {
                    if start_ms > max_ms {
                        continue;
                    }
                }

                let final_end_ms = if let Some(max_ms) = max_duration_ms {
                    end_ms.min(max_ms)
                } else {
                    end_ms
                };

                // Split text into words
                let words: Vec<&str> = seg.text.split_whitespace().collect();
                if words.is_empty() {
                    continue;
                }

                // Distribute time based on word length (better than linear distribution)
                let word_lengths: Vec<usize> = words.iter().map(|w| w.len()).collect();
                let total_chars: usize = word_lengths.iter().sum();
                let base_time = segment_duration_ms as f64;

                let mut cumulative_time = 0.0;
                for (i, word) in words.iter().enumerate() {
                    let word_start_ms = start_ms + cumulative_time as u64;

                    // Allocate time based on word length ratio with minimum duration
                    let char_ratio = if total_chars > 0 {
                        word_lengths[i] as f64 / total_chars as f64
                    } else {
                        1.0 / words.len() as f64 // Fallback to equal distribution
                    };

                    // Ensure minimum 100ms per word, but don't exceed segment duration
                    let word_duration = (base_time * char_ratio).max(100.0);
                    cumulative_time += word_duration;

                    let word_end_ms = if i == words.len() - 1 {
                        final_end_ms // Last word gets remaining time
                    } else {
                        (word_start_ms as f64 + word_duration).min(final_end_ms as f64) as u64
                    };

                    if word_end_ms <= word_start_ms {
                        continue;
                    }

                    let word_text = word.to_string();
                    word_segments.push(CaptionSegment {
                        start_ms: word_start_ms,
                        end_ms: word_end_ms,
                        text: word_text.clone(),
                        words: vec![WordSpan {
                            start_ms: word_start_ms,
                            end_ms: word_end_ms,
                            text: word_text,
                            glue_to_previous: false,
                        }],
                    });
                }
            }

            return word_segments;
        }
    }

    if let Some(segments) = &response.segments {
        // Use segment-level timing, but try to populate words if available.
        // Words are emitted in timestamp order by whisper, so we walk the
        // word list once with a running pointer instead of rescanning all
        // words for every segment (previously O(segments * words)).
        let all_words = response.words.clone().unwrap_or_default();
        let mut wi = 0usize;
        let mut out = Vec::with_capacity(segments.len());

        for (seg_idx, seg) in segments.iter().enumerate() {
            let start_ms = (seg.start * 1000.0) as u64;
            let end_ms = (seg.end * 1000.0) as u64;
            // Where the next segment's own words begin. The tolerance below may
            // only reach into silence, never past this line.
            let next_start_ms = segments
                .get(seg_idx + 1)
                .map(|next| (next.start * 1000.0) as u64);

            // skip segments that are beyond the actual audio duration
            if let Some(max_ms) = max_duration_ms {
                if start_ms > max_ms {
                    continue;
                }
            }

            let final_end_ms = if let Some(max_ms) = max_duration_ms {
                end_ms.min(max_ms)
            } else {
                end_ms
            };

            // skip segments with very short duration (less than 50ms)
            let duration_ms = final_end_ms.saturating_sub(start_ms);
            if duration_ms < 50 {
                continue;
            }

            // Advance our pointer past words that end well before this segment.
            if !all_words.is_empty() {
                let window_start = start_ms.saturating_sub(100);
                while wi < all_words.len() && ((all_words[wi].end * 1000.0) as u64) < window_start {
                    wi += 1;
                }
            }

            // Consume the words as we take them. Scanning with a separate cursor
            // and leaving `wi` behind handed every word inside the ±100ms
            // tolerance to this segment *and* the next one, so boundary words
            // appeared twice — and two segments holding the same words render as
            // two caption blocks on top of each other.
            //
            // The tolerance also has to stop at the next segment: whisper puts a
            // segment boundary exactly where the next word starts, so a plain
            // `end + 100ms` window swallowed that word — and with the cursor
            // moved past it, every following segment held the words of the one
            // before it. Sentences and word timings then disagreed by one word
            // all the way to the end of the video.
            let mut segment_words = Vec::new();
            let window_end = match next_start_ms {
                // Only reach into the gap, and stop just short of the next word.
                Some(next) => (final_end_ms + 100).min(next.saturating_sub(1)),
                None => final_end_ms + 100,
            };
            while wi < all_words.len() {
                let w = &all_words[wi];
                let w_start_ms = (w.start * 1000.0) as u64;
                if w_start_ms > window_end {
                    break; // words are ordered by start: no more matches
                }
                let w_end_ms = (w.end * 1000.0) as u64;
                segment_words.push(WordSpan {
                    start_ms: w_start_ms,
                    end_ms: w_end_ms,
                    text: w.word.clone(),
                    // Carried through so the renderer can put a word back
                    // together that whisper split across a segment boundary.
                    glue_to_previous: w.glue_to_previous,
                });
                wi += 1;
            }

            out.push(CaptionSegment {
                start_ms,
                end_ms: final_end_ms,
                text: seg.text.clone(),
                words: segment_words,
            });
        }

        out
    } else {
        // fallback: create single segment from full text
        let duration = response.duration.unwrap_or(60.0) * 1000.0;
        let text = response.text.clone();
        vec![CaptionSegment {
            start_ms: 0,
            end_ms: duration as u64,
            text: text.clone(),
            words: vec![WordSpan {
                start_ms: 0,
                end_ms: duration as u64,
                text,
                glue_to_previous: false,
            }],
        }]
    }
}

pub async fn get_cached_whisper_response(
    audio_path: &str,
    params: &TranscribeSegmentsParams,
) -> anyhow::Result<Option<WhisperResponse>> {
    let (audio_hash, params_hash) = compute_segments_cache_key(audio_path, params).await?;
    let index = load_cache_index().await?;

    for entry in &index.entries {
        if entry.audio_hash == audio_hash
            && entry.params_hash == params_hash
            && std::path::Path::new(&entry.response_path).exists()
        {
            let content = fs::read_to_string(&entry.response_path).await?;
            let response: WhisperResponse = serde_json::from_str(&content)?;
            return Ok(Some(response));
        }
    }
    Ok(None)
}

pub async fn save_cached_whisper_response(
    audio_path: &str,
    params: &TranscribeSegmentsParams,
    response: &WhisperResponse,
) -> anyhow::Result<()> {
    let (audio_hash, params_hash) = compute_segments_cache_key(audio_path, params).await?;
    let mut index = load_cache_index().await?;
    let cache_dir = get_cache_dir()?;
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();

    // create cache filename and save JSON response
    let cache_filename = format!("{}_{}.json", &audio_hash[..8], &params_hash[..8]);
    let cached_json_path = cache_dir.join(cache_filename);
    let json_content = serde_json::to_string_pretty(response)?;
    fs::write(&cached_json_path, json_content).await?;

    // add new entry
    let new_entry = WhisperCacheEntry {
        audio_hash,
        params_hash,
        response_path: cached_json_path.to_string_lossy().to_string(),
        timestamp,
    };

    // remove old entry if exists
    index.entries.retain(|e| {
        !(e.audio_hash == new_entry.audio_hash && e.params_hash == new_entry.params_hash)
    });

    // add new entry
    index.entries.push(new_entry);

    // keep only 4 most recent entries (LRU eviction)
    if index.entries.len() > 4 {
        index.entries.sort_by_key(|e| e.timestamp);
        let to_remove = index
            .entries
            .drain(0..index.entries.len() - 4)
            .collect::<Vec<_>>();

        // delete old cached files
        for entry in to_remove {
            let _ = fs::remove_file(&entry.response_path).await;
        }
    }

    save_cache_index(&index).await?;
    Ok(())
}

pub async fn compute_segments_cache_key(
    audio_path: &str,
    params: &TranscribeSegmentsParams,
) -> anyhow::Result<(String, String)> {
    use tokio::io::AsyncReadExt;

    // Stream the audio file through the hash instead of loading it all into
    // memory — a multi-hundred-MB file must never be buffered on every
    // transcribe call, and a synchronous read would block the async runtime.
    let mut file = fs::File::open(audio_path).await?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let audio_hash = hasher.finalize().to_hex().to_string();

    // hash relevant parameters (excluding video_file as it doesn't affect transcription)
    let params_for_hash = serde_json::json!({
        "model": params.model,
        "language": params.language,
        "split_by_words": params.split_by_words,
        "prompt": params.prompt,
        // Cached entries hold parsed words, so a change to the parser has to
        // invalidate them: v3 records which pieces continue the previous word.
        "version": "v3_word_glue",
    });
    let params_hash = blake3::hash(params_for_hash.to_string().as_bytes())
        .to_hex()
        .to_string();

    Ok((audio_hash, params_hash))
}

pub async fn save_cache_index(index: &WhisperCacheIndex) -> anyhow::Result<()> {
    let cache_dir = get_cache_dir()?;
    let index_path = cache_dir.join("index.json");
    let content = serde_json::to_string_pretty(index)?;
    fs::write(index_path, content).await?;
    Ok(())
}

pub async fn load_cache_index() -> anyhow::Result<WhisperCacheIndex> {
    let cache_dir = get_cache_dir()?;
    let index_path = cache_dir.join("index.json");

    if index_path.exists() {
        let content = fs::read_to_string(index_path).await?;
        Ok(serde_json::from_str(&content).unwrap_or(WhisperCacheIndex {
            entries: Vec::new(),
        }))
    } else {
        Ok(WhisperCacheIndex {
            entries: Vec::new(),
        })
    }
}

pub fn get_cache_dir() -> std::io::Result<PathBuf> {
    let mut cache_dir = std::env::temp_dir();
    cache_dir.push("capslap_whisper_cache");
    std::fs::create_dir_all(&cache_dir)?;
    Ok(cache_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_words_are_never_shared_between_segments() {
        // Two adjacent segments with a word starting right on the boundary. The
        // ±100ms tolerance used to hand that word to both, which showed up as
        // duplicated words in the editor and doubled captions in the preview.
        use crate::types::{WhisperResponse, WhisperSegment, WhisperWord};

        let response = WhisperResponse {
            task: None,
            language: None,
            duration: Some(4.0),
            text: "one two three four".to_string(),
            segments: Some(vec![
                WhisperSegment {
                    id: 0,
                    start: 0.0,
                    end: 2.0,
                    text: "one two".to_string(),
                },
                WhisperSegment {
                    id: 1,
                    start: 2.0,
                    end: 4.0,
                    text: "three four".to_string(),
                },
            ]),
            words: Some(vec![
                WhisperWord {
                    word: "one".into(),
                    start: 0.0,
                    end: 0.9,
                    glue_to_previous: false,
                },
                WhisperWord {
                    word: "two".into(),
                    start: 1.0,
                    end: 1.95,
                    glue_to_previous: false,
                },
                WhisperWord {
                    word: "three".into(),
                    start: 2.02,
                    end: 3.0,
                    glue_to_previous: false,
                },
                WhisperWord {
                    word: "four".into(),
                    start: 3.0,
                    end: 4.0,
                    glue_to_previous: false,
                },
            ]),
        };

        let segments = whisper_to_caption_segments(&response, false);
        assert_eq!(segments.len(), 2);

        let mut seen: Vec<String> = Vec::new();
        for segment in &segments {
            for word in &segment.words {
                assert!(
                    !seen.contains(&word.text),
                    "word {:?} appears in more than one segment",
                    word.text
                );
                seen.push(word.text.clone());
            }
        }
        assert_eq!(seen, vec!["one", "two", "three", "four"]);

        // ...and each word stays with the sentence it was spoken in. A word
        // starting just after a boundary belongs to the segment that follows it,
        // not to the one whose tolerance window happens to reach it.
        let words_of = |segment: &CaptionSegment| {
            segment
                .words
                .iter()
                .map(|w| w.text.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(words_of(&segments[0]), vec!["one", "two"]);
        assert_eq!(words_of(&segments[1]), vec!["three", "four"]);
    }

    #[test]
    fn boundary_word_does_not_shift_every_later_segment() {
        // Whisper puts the boundary exactly where the next word starts, so the
        // tolerance used to swallow that word into the previous segment. With
        // the cursor moved past it, every following segment held the words of
        // the sentence before it: the editor showed one text, the burner
        // rendered another.
        use crate::types::{WhisperResponse, WhisperSegment, WhisperWord};

        let seg = |id: u32, start: f64, end: f64, text: &str| WhisperSegment {
            id,
            start,
            end,
            text: text.to_string(),
        };
        let word = |text: &str, start: f64, end: f64| WhisperWord {
            word: text.to_string(),
            start,
            end,
            glue_to_previous: false,
        };

        let response = WhisperResponse {
            task: None,
            language: None,
            duration: Some(6.0),
            text: "yksi kaksi kolme neljä viisi".to_string(),
            segments: Some(vec![
                seg(0, 0.0, 2.0, "yksi kaksi"),
                seg(1, 2.0, 4.0, "kolme neljä"),
                seg(2, 4.0, 6.0, "viisi"),
            ]),
            words: Some(vec![
                word("yksi", 0.04, 0.9),
                word("kaksi", 1.0, 2.0),
                // Starts on the boundary, i.e. inside the old +100ms window.
                word("kolme", 2.0, 3.0),
                word("neljä", 3.02, 4.0),
                word("viisi", 4.04, 6.0),
            ]),
        };

        let segments = whisper_to_caption_segments(&response, false);
        let words_of = |segment: &CaptionSegment| {
            segment
                .words
                .iter()
                .map(|w| w.text.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(words_of(&segments[0]), vec!["yksi", "kaksi"]);
        assert_eq!(words_of(&segments[1]), vec!["kolme", "neljä"]);
        assert_eq!(words_of(&segments[2]), vec!["viisi"]);

        // The words a segment holds are the words its own text names.
        for segment in &segments {
            assert_eq!(
                words_of(segment).join(" "),
                segment.text.trim(),
                "segment text and word timings disagree"
            );
        }
    }

    // ============================================
    // is_digits tests
    // ============================================

    #[test]
    fn test_is_digits_valid() {
        assert!(is_digits("12345"));
        assert!(is_digits("0"));
        assert!(is_digits("999"));
    }

    #[test]
    fn test_is_digits_invalid() {
        assert!(!is_digits(""));
        assert!(!is_digits("12.34"));
        assert!(!is_digits("abc"));
        assert!(!is_digits("12a34"));
        assert!(!is_digits("12 34"));
        assert!(!is_digits("-123"));
    }

    // ============================================
    // format_with_thousands tests
    // ============================================

    #[test]
    fn test_format_with_thousands_large_numbers() {
        assert_eq!(format_with_thousands("1000".to_string()), "1,000");
        assert_eq!(format_with_thousands("1000000".to_string()), "1,000,000");
        assert_eq!(
            format_with_thousands("1234567890".to_string()),
            "1,234,567,890"
        );
    }

    #[test]
    fn test_format_with_thousands_small_numbers() {
        assert_eq!(format_with_thousands("100".to_string()), "100");
        assert_eq!(format_with_thousands("1".to_string()), "1");
        assert_eq!(format_with_thousands("99".to_string()), "99");
    }

    #[test]
    fn test_format_with_thousands_empty() {
        assert_eq!(format_with_thousands("".to_string()), "");
    }

    // ============================================
    // get_model_download_url tests
    // ============================================

    #[test]
    fn test_get_model_download_url_valid() {
        let url = get_model_download_url("ggml-base.bin");
        assert!(url.contains("huggingface.co"));
        assert!(url.contains("ggerganov/whisper.cpp"));
        assert!(url.contains("ggml-base.bin"));
    }

    #[test]
    fn test_get_model_download_url_different_models() {
        let url_tiny = get_model_download_url("ggml-tiny.bin");
        let url_large = get_model_download_url("ggml-large-v3.bin");

        assert!(url_tiny.contains("ggml-tiny.bin"));
        assert!(url_large.contains("ggml-large-v3.bin"));

        // Both should use same base URL
        assert!(url_tiny.starts_with("https://huggingface.co/"));
        assert!(url_large.starts_with("https://huggingface.co/"));
    }

    // ============================================
    // parse_whisper_cpp_output tests
    // ============================================

    #[test]
    fn test_parse_whisper_cpp_output_valid_simple() {
        let json = r#"{
            "transcription": [
                {
                    "offsets": {"from": 0, "to": 1000},
                    "text": " Hello world"
                }
            ]
        }"#;

        let result = parse_whisper_cpp_output(json);
        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(response.text.contains("Hello"));
        assert_eq!(response.segments.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn test_parse_whisper_cpp_output_multiple_segments() {
        let json = r#"{
            "transcription": [
                {
                    "offsets": {"from": 0, "to": 1000},
                    "text": " Hello"
                },
                {
                    "offsets": {"from": 1000, "to": 2000},
                    "text": " world"
                }
            ]
        }"#;

        let result = parse_whisper_cpp_output(json);
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.segments.as_ref().unwrap().len(), 2);
        assert!(response.text.contains("Hello"));
        assert!(response.text.contains("world"));
    }

    #[test]
    fn test_parse_whisper_cpp_output_with_tokens() {
        let json = r#"{
            "transcription": [
                {
                    "offsets": {"from": 0, "to": 2000},
                    "text": " Hello world",
                    "tokens": [
                        {"text": " Hello", "offsets": {"from": 0, "to": 1000}},
                        {"text": " world", "offsets": {"from": 1000, "to": 2000}}
                    ]
                }
            ]
        }"#;

        let result = parse_whisper_cpp_output(json);
        assert!(result.is_ok());
        let response = result.unwrap();
        // Should have parsed word-level timestamps
        assert!(response.words.is_some());
        let words = response.words.unwrap();
        assert_eq!(words.len(), 2);
    }

    #[test]
    fn test_parse_whisper_cpp_output_empty_transcription() {
        let json = r#"{"transcription": []}"#;
        let result = parse_whisper_cpp_output(json);
        // Empty transcription should error
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_whisper_cpp_output_invalid_json() {
        let result = parse_whisper_cpp_output("not valid json");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_whisper_cpp_output_missing_transcription() {
        let json = r#"{"other_field": "value"}"#;
        let result = parse_whisper_cpp_output(json);
        // No transcription = empty text = error
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_whisper_cpp_output_duration_calculation() {
        let json = r#"{
            "transcription": [
                {
                    "offsets": {"from": 0, "to": 5000},
                    "text": " First segment"
                },
                {
                    "offsets": {"from": 5000, "to": 10000},
                    "text": " Second segment"
                }
            ]
        }"#;

        let result = parse_whisper_cpp_output(json).unwrap();
        // Duration should be the end time of the last segment (10s)
        assert_eq!(result.duration, Some(10.0));
    }

    #[test]
    fn test_parse_whisper_cpp_output_skips_special_tokens() {
        let json = r#"{
            "transcription": [
                {
                    "offsets": {"from": 0, "to": 2000},
                    "text": " Hello",
                    "tokens": [
                        {"text": "[_BEG_]", "offsets": {"from": 0, "to": 100}},
                        {"text": " Hello", "offsets": {"from": 100, "to": 2000}}
                    ]
                }
            ]
        }"#;

        let result = parse_whisper_cpp_output(json).unwrap();
        if let Some(words) = result.words {
            // Should not include [_BEG_] special token
            for word in &words {
                assert!(!word.word.starts_with('['));
                assert!(!word.word.ends_with(']'));
            }
        }
    }

    // ============================================
    // check_model_exists tests
    // ============================================

    #[test]
    fn test_check_model_exists_unknown_model() {
        // Unknown model names should return Ok(false)
        let result = check_model_exists("nonexistent_model_xyz");
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[test]
    fn test_check_model_exists_known_model_names() {
        // These should at least not error, even if model doesn't exist
        let models = ["tiny", "base", "small", "medium", "large", "turbo"];
        for model in models {
            let result = check_model_exists(model);
            assert!(
                result.is_ok(),
                "check_model_exists should not error for {}",
                model
            );
        }
    }

    // ============================================
    // get_system_whisper_paths tests
    // ============================================

    #[test]
    fn test_get_system_whisper_paths_not_empty() {
        let paths = get_system_whisper_paths();
        assert!(!paths.is_empty());
        assert!(paths.contains(&"whisper-cli".to_string()));
    }

    // ============================================
    // get_system_ffmpeg_paths tests
    // ============================================

    #[test]
    fn test_get_system_ffmpeg_paths_not_empty() {
        let paths = get_system_ffmpeg_paths();
        assert!(!paths.is_empty());
        assert!(paths.contains(&"ffmpeg".to_string()));
    }

    // ============================================
    // get_system_ffprobe_paths tests
    // ============================================

    #[test]
    fn test_get_system_ffprobe_paths_not_empty() {
        let paths = get_system_ffprobe_paths();
        assert!(!paths.is_empty());
        assert!(paths.contains(&"ffprobe".to_string()));
    }

    // ============================================
    // get_cache_dir tests
    // ============================================

    #[test]
    fn test_get_cache_dir_creates_directory() {
        let result = get_cache_dir();
        assert!(result.is_ok());
        let cache_dir = result.unwrap();
        assert!(cache_dir.exists());
        assert!(cache_dir
            .to_string_lossy()
            .contains("capslap_whisper_cache"));
    }

    #[test]
    fn test_bounded_log_buffer_bounds_and_summary() {
        let mut buf = BoundedLogBuffer::new(50, 100);

        // Push 100 lines to verify head and tail stay bounded
        for i in 0..100 {
            buf.push_line(&format!("Line {:03}: processing step", i));
        }

        let head = buf.head_chars(500);
        // Head should contain the earliest lines and be bounded
        assert!(head.starts_with("Line 000:"));
        assert!(head.len() <= 60);

        // Error summary should contain the latest lines
        let err = buf.error_summary(500);
        assert!(err.contains("Line 099:"));
        assert!(err.len() <= 120);
    }
}
