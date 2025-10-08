use crate::{types::{CaptionSegment, WhisperResponse, WhisperCacheEntry, WhisperCacheIndex, TranscribeSegmentsParams, TranscribeSegmentsResult, WhisperWord}};
use blake3;
use tokio::fs;
use tokio::process::Command as TokioCommand;
use std::path::PathBuf;
use std::process::Stdio;
use crate::rpc::RpcEvent;
use crate::video::{is_ffmpeg_whisper_available, is_whisper_cpp_available};
use regex::Regex;

/// Transcribe audio using whisper.cpp CLI (preferred method)
pub async fn transcribe_with_whisper_cpp(
    id: &str,
    audio_path: &str,
    model: Option<String>,
    language: Option<String>,
    mut emit: impl FnMut(RpcEvent)
) -> anyhow::Result<WhisperResponse> {
    // Use requested model or default to tiny
    let whisper_model = match model.as_deref() {
        Some(m) => m.to_string(),
        None => "tiny".to_string(),
    };

    emit(RpcEvent::Log {
        id: id.into(),
        message: format!("Starting local whisper.cpp transcription with model: {}", whisper_model)
    });

    emit(RpcEvent::Log {
        id: id.into(),
        message: format!("Model requested: {}, DTW preset: disabled (testing without DTW)", whisper_model)
    });

    // Find model with fallbacks
    let (model_path, actual_model) = ensure_whisper_model(&whisper_model).await?;

    if actual_model != whisper_model {
        emit(RpcEvent::Log {
            id: id.into(),
            message: format!("Model '{}' not found, using '{}' instead", whisper_model, actual_model)
        });
    }

    emit(RpcEvent::Log {
        id: id.into(),
        message: format!("Using model file: {} ({})", model_path, actual_model)
    });

    let whisper_binary = match find_whisper_binary().await {
        Ok(binary) => {
            emit(RpcEvent::Log {
                id: id.into(),
                message: format!("Found whisper binary at: {}", binary)
            });
            binary
        }
        Err(e) => {
            emit(RpcEvent::Log {
                id: id.into(),
                message: format!("Failed to find whisper binary: {}", e)
            });
            return Err(e);
        }
    };
    let mut cmd = TokioCommand::new(&whisper_binary);
    // DTW disabled - causes timestamp issues for some audio files
    let dtw_preset: Option<&str> = None;

    cmd.arg("-m").arg(&model_path)
       .arg("--output-json-full")    // Full JSON output
       .arg("--no-prints")          // Suppress progress output
       .arg("--word-thold").arg("0.01")   // Better word boundary detection
       .arg("--max-len").arg("0")         // No segment length limit
       .arg("--output-words")            // Enable word-level timestamps
       .arg("--entropy-thold").arg("2.8") // Anti-repetition
       .arg("--suppress-nst");           // Suppress non-speech tokens

    cmd.arg(audio_path);

    if let Some(lang) = &language {
        cmd.arg("-l").arg(lang);
    }

    cmd.stdout(Stdio::piped())
       .stderr(Stdio::piped());

    let output = cmd.output().await?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    emit(RpcEvent::Log {
        id: id.into(),
        message: format!("whisper.cpp stdout: {}", stdout.chars().take(500).collect::<String>())
    });
    emit(RpcEvent::Log {
        id: id.into(),
        message: format!("whisper.cpp stderr: {}", stderr.chars().take(500).collect::<String>())
    });

    if !output.status.success() {
        return Err(anyhow::anyhow!("whisper.cpp failed with status {}: {}", output.status, stderr));
    }

    emit(RpcEvent::Log {
        id: id.into(),
        message: "Parsing whisper.cpp output...".into()
    });

    // whisper.cpp creates a JSON file next to the audio file
    let json_file_path = format!("{}.json", audio_path);

    emit(RpcEvent::Log {
        id: id.into(),
        message: format!("Looking for JSON output at: {}", json_file_path)
    });

    // Check if file exists before trying to read
    if !std::path::Path::new(&json_file_path).exists() {
        emit(RpcEvent::Log {
            id: id.into(),
            message: format!("JSON file does not exist at: {}", json_file_path)
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
                    message: format!("Files in directory: {:?}", files)
                });
            }
        }

        return Err(anyhow::anyhow!("whisper.cpp did not create expected JSON output file: {}", json_file_path));
    }

    let json_content = std::fs::read_to_string(&json_file_path)
        .map_err(|e| anyhow::anyhow!("Failed to read whisper.cpp JSON output: {}", e))?;

    // Debug: Log first 1000 chars of JSON to understand structure
    emit(RpcEvent::Log {
        id: id.into(),
        message: format!("whisper.cpp JSON preview: {}", &json_content.chars().take(1000).collect::<String>())
    });

    // Parse the JSON output from file
    let whisper_response = parse_whisper_cpp_output(&json_content)?;

    emit(RpcEvent::Log {
        id: id.into(),
        message: format!("Local whisper.cpp transcription completed. Duration: {:.2}s, Segments: {}, Words: {}",
            whisper_response.duration.unwrap_or(0.0),
            whisper_response.segments.as_ref().map(|s| s.len()).unwrap_or(0),
            whisper_response.words.as_ref().map(|w| w.len()).unwrap_or(0))
    });

    Ok(whisper_response)
}

/// Ensure whisper model exists with intelligent fallbacks
async fn ensure_whisper_model(model: &str) -> anyhow::Result<(String, String)> {
    // Define fallback chain: requested -> base -> tiny
    let fallback_chain = match model {
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
            _ => continue,
        };

        // Check if model exists in common locations
        let mut possible_paths = vec![];

        // Add bundled model paths (relative to executable for production)
        // Priority 1: Check next to executable (production macOS app bundle)
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                // For macOS app bundle: CapSlap.app/Contents/MacOS/../Resources/models/
                possible_paths.push(format!("{}/models/{}", exe_dir.display(), model_filename));
                if let Some(contents_dir) = exe_dir.parent() {
                    possible_paths.push(format!("{}/Resources/models/{}", contents_dir.display(), model_filename));
                }
            }
        }

        // Priority 2: Development environment
        possible_paths.push(format!("{}/models/{}", env!("CARGO_MANIFEST_DIR"), model_filename));

        // Priority 3: System locations
        possible_paths.push(format!("./models/{}", model_filename));
        possible_paths.push(format!("/opt/homebrew/share/whisper-models/{}", model_filename));
        possible_paths.push(format!("~/.cache/whisper/{}", model_filename));

        for path in possible_paths {
            let expanded_path = if path.starts_with("~/") {
                path.replace("~", &std::env::var("HOME").unwrap_or_default())
            } else {
                path
            };

            if std::path::Path::new(&expanded_path).exists() {
                return Ok((expanded_path, fallback_model.to_string()));
            }
        }
    }

    // No models found locally - this will trigger OpenAI API fallback at higher level
    Err(anyhow::anyhow!("No whisper models found locally. Tried fallback chain: {:?}", fallback_chain))
}

/// Find whisper.cpp binary across different locations and platforms
pub async fn find_whisper_binary() -> anyhow::Result<String> {
    // Priority order:
    // 1. Bundled binary (next to executable)
    // 2. Project binary (for development)
    // 3. System installation (Homebrew, etc.)

    // Try to get the directory where the current executable is located
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let bundled_paths = get_bundled_whisper_paths(exe_dir);
            for path in bundled_paths {
                if path.exists() {
                    return Ok(path.to_string_lossy().to_string());
                }
            }
        }
    }

    // Try project directory (for development)
    let project_paths = get_project_whisper_paths();
    for path in project_paths {
        if path.exists() {
            return Ok(path.to_string_lossy().to_string());
        }
    }

    // Try system installations
    let system_paths = get_system_whisper_paths();
    for path in system_paths {
        if let Ok(which_path) = which::which(&path) {
            return Ok(which_path.to_string_lossy().to_string());
        }
    }

    Err(anyhow::anyhow!("whisper.cpp binary not found in any location"))
}

/// Get possible bundled whisper binary paths (next to executable)
fn get_bundled_whisper_paths(exe_dir: &std::path::Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    #[cfg(target_os = "macos")]
    {
        if cfg!(target_arch = "aarch64") {
            paths.push(exe_dir.join("bin/whisper-cli-macos-arm64"));  // Check bin subdirectory first
            paths.push(exe_dir.join("whisper-cli-macos-arm64"));
            paths.push(exe_dir.join("bin/whisper-macos-arm64"));
            paths.push(exe_dir.join("whisper-macos-arm64"));
        } else {
            paths.push(exe_dir.join("bin/whisper-cli-macos-x64"));    // Check bin subdirectory first
            paths.push(exe_dir.join("whisper-cli-macos-x64"));
            paths.push(exe_dir.join("bin/whisper-macos-x64"));
            paths.push(exe_dir.join("whisper-macos-x64"));
        }
        paths.push(exe_dir.join("bin/whisper-cli"));                  // Check bin subdirectory first
        paths.push(exe_dir.join("whisper-cli"));
        paths.push(exe_dir.join("bin/whisper"));                      // Check bin subdirectory first
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
        if cfg!(target_arch = "x86_64") {
            paths.push(exe_dir.join("bin/whisper-linux-x64")); // Check bin subdirectory first
            paths.push(exe_dir.join("whisper-linux-x64"));
        } else if cfg!(target_arch = "aarch64") {
            paths.push(exe_dir.join("bin/whisper-linux-arm64")); // Check bin subdirectory first
            paths.push(exe_dir.join("whisper-linux-arm64"));
        }
        paths.push(exe_dir.join("bin/whisper-cli"));          // Check bin subdirectory first
        paths.push(exe_dir.join("whisper-cli"));
        paths.push(exe_dir.join("bin/whisper"));              // Check bin subdirectory first
        paths.push(exe_dir.join("whisper"));
    }

    paths
}

/// Get possible project whisper binary paths (for development)
fn get_project_whisper_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // Project bin directory
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let bin_dir = project_root.join("bin");

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
    // Priority order:
    // 1. Bundled binary (next to executable)
    // 2. Project binary (for development)
    // 3. System installation (Homebrew, etc.)

    // Allow override via environment
    if let Ok(path) = std::env::var("FFMPEG_PATH") {
        if std::path::Path::new(&path).exists() {
            return Ok(path);
        }
    }

    // Try bundled binary first (next to executable)
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let bundled_paths = get_bundled_ffmpeg_paths(exe_dir);
            for path in bundled_paths {
                if path.exists() {
                    return Ok(path.to_string_lossy().to_string());
                }
            }
        }
    }

    // Try project directory (for development)
    let project_paths = get_project_ffmpeg_paths();
    for path in project_paths {
        if path.exists() {
            return Ok(path.to_string_lossy().to_string());
        }
    }

    // Try system installations
    let system_paths = get_system_ffmpeg_paths();
    for path in system_paths {
        if let Ok(which_path) = which::which(&path) {
            return Ok(which_path.to_string_lossy().to_string());
        }
    }

    Err(anyhow::anyhow!("FFmpeg binary not found in any location"))
}

/// Find ffprobe binary using priority order (bundled > project > system)
pub async fn find_ffprobe_binary() -> anyhow::Result<String> {
    // Priority order:
    // 1. Bundled binary (next to executable)
    // 2. Project binary (for development)
    // 3. System installation (Homebrew, etc.)

    // Allow override via environment
    if let Ok(path) = std::env::var("FFPROBE_PATH") {
        if std::path::Path::new(&path).exists() {
            return Ok(path);
        }
    }

    // Try bundled binary first (next to executable)
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let bundled_paths = get_bundled_ffprobe_paths(exe_dir);
            for path in bundled_paths {
                if path.exists() {
                    return Ok(path.to_string_lossy().to_string());
                }
            }
        }
    }

    // Try project directory (for development)
    let project_paths = get_project_ffprobe_paths();
    for path in project_paths {
        if path.exists() {
            return Ok(path.to_string_lossy().to_string());
        }
    }

    // Try system installations
    let system_paths = get_system_ffprobe_paths();
    for path in system_paths {
        if let Ok(which_path) = which::which(&path) {
            return Ok(which_path.to_string_lossy().to_string());
        }
    }

    Err(anyhow::anyhow!("ffprobe binary not found in any location"))
}

/// Get possible bundled FFmpeg binary paths (next to executable)
fn get_bundled_ffmpeg_paths(exe_dir: &std::path::Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    #[cfg(target_os = "macos")]
    {
        paths.push(exe_dir.join("bin/ffmpeg"));  // Check bin subdirectory first
        paths.push(exe_dir.join("ffmpeg"));
    }

    #[cfg(target_os = "windows")]
    {
        paths.push(exe_dir.join("bin/ffmpeg.exe"));  // Check bin subdirectory first
        paths.push(exe_dir.join("ffmpeg.exe"));
    }

    #[cfg(target_os = "linux")]
    {
        paths.push(exe_dir.join("bin/ffmpeg"));      // Check bin subdirectory first
        paths.push(exe_dir.join("ffmpeg"));
    }

    paths
}

/// Get possible project FFmpeg binary paths
fn get_project_ffmpeg_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let bin_dir = std::env::current_dir().unwrap_or_default().join("bin");

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
        paths.push(exe_dir.join("bin/ffprobe"));  // Check bin subdirectory first
        paths.push(exe_dir.join("ffprobe"));
    }

    #[cfg(target_os = "windows")]
    {
        paths.push(exe_dir.join("bin/ffprobe.exe"));  // Check bin subdirectory first
        paths.push(exe_dir.join("ffprobe.exe"));
    }

    #[cfg(target_os = "linux")]
    {
        paths.push(exe_dir.join("bin/ffprobe"));      // Check bin subdirectory first
        paths.push(exe_dir.join("ffprobe"));
    }

    paths
}

/// Get possible project ffprobe binary paths
fn get_project_ffprobe_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let bin_dir = std::env::current_dir().unwrap_or_default().join("bin");

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
    format!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{}", model_filename)
}

/// Download whisper model from HuggingFace
async fn download_whisper_model(url: &str, path: &str) -> anyhow::Result<()> {
    use reqwest;
    use tokio::io::AsyncWriteExt;

    let response = reqwest::get(url).await?;
    if !response.status().is_success() {
        return Err(anyhow::anyhow!("Failed to download model: HTTP {}", response.status()));
    }

    let mut file = tokio::fs::File::create(path).await?;
    let mut stream = response.bytes_stream();

    use futures_util::StreamExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
    }

    file.flush().await?;
    Ok(())
}

/// Public RPC method to download a whisper model with progress reporting
pub async fn download_model_rpc(
    id: &str,
    params: crate::types::DownloadModelParams,
    mut emit: impl FnMut(crate::rpc::RpcEvent)
) -> anyhow::Result<crate::types::DownloadModelResult> {
    use tokio::io::AsyncWriteExt;
    use futures_util::StreamExt;

    let model_filename = match params.model.as_str() {
        "tiny" => "ggml-tiny.bin",
        "base" => "ggml-base.bin",
        "small" => "ggml-small.bin",
        "medium" => "ggml-medium.bin",
        "large" => "ggml-large-v3.bin",
        _ => return Err(anyhow::anyhow!("Unknown model: {}. Supported: tiny, base, small, medium, large", params.model))
    };

    let url = get_model_download_url(model_filename);
    let models_dir = get_models_dir()?;
    let output_path = models_dir.join(model_filename);

    emit(crate::rpc::RpcEvent::Log {
        id: id.into(),
        message: format!("Starting download of {} model from HuggingFace", params.model)
    });

    // Download with progress
    let client = reqwest::Client::new();
    let response = client.get(&url).send().await?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!("Failed to download model: HTTP {}", response.status()));
    }

    let total_size = response.content_length().unwrap_or(0);

    emit(crate::rpc::RpcEvent::Log {
        id: id.into(),
        message: format!("Downloading {} ({:.1} MB)...", model_filename, total_size as f64 / 1024.0 / 1024.0)
    });

    let mut file = tokio::fs::File::create(&output_path).await?;
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
            progress
        });
    }

    file.flush().await?;

    emit(crate::rpc::RpcEvent::Log {
        id: id.into(),
        message: format!("Successfully downloaded {} model to {}", params.model, output_path.display())
    });

    Ok(crate::types::DownloadModelResult {
        model: params.model,
        path: output_path.to_string_lossy().to_string(),
        size: downloaded
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
        _ => return Ok(false)
    };

    let models_dir = get_models_dir()?;
    let model_path = models_dir.join(model_filename);
    Ok(model_path.exists())
}

/// Get the models directory path
fn get_models_dir() -> anyhow::Result<std::path::PathBuf> {
    // Use project root models directory for both dev and production
    // This ensures models are in one place: /Users/egor/Desktop/capslap/rust/models/
    let project_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let models_dir = project_root.join("models");
    std::fs::create_dir_all(&models_dir)?;
    Ok(models_dir)
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
                    segment.get("offsets").and_then(|o| o.get("from")).and_then(|f| f.as_f64()),
                    segment.get("offsets").and_then(|o| o.get("to")).and_then(|t| t.as_f64()),
                    segment.get("text").and_then(|t| t.as_str())
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
                }
            }
        }
    }

    full_text = full_text.trim().to_string();

    if full_text.is_empty() {
        return Err(anyhow::anyhow!("No transcription text found in whisper.cpp output"));
    }

    let response = WhisperResponse {
        task: Some("transcribe".to_string()),
        language: None,
        duration: Some(duration),
        text: full_text,
        segments: Some(segments.clone()),
        words: if words.is_empty() { None } else { Some(words.clone()) },
    };

    Ok(response)
}

/// Transcribe audio using local FFmpeg Whisper (requires FFmpeg 8.0+)
pub async fn transcribe_with_ffmpeg_whisper(
    id: &str,
    audio_path: &str,
    model: Option<String>,
    language: Option<String>,
    mut emit: impl FnMut(RpcEvent)
) -> anyhow::Result<WhisperResponse> {
    let whisper_model = model.unwrap_or_else(|| "medium".to_string());

    emit(RpcEvent::Log {
        id: id.into(),
        message: format!("Starting local FFmpeg Whisper transcription with model: {}", whisper_model)
    });

    let ffmpeg_path = find_ffmpeg_binary().await.map_err(|e| anyhow::anyhow!("FFmpeg not found: {}", e))?;
    let mut cmd = TokioCommand::new(ffmpeg_path);
    cmd.arg("-y") // overwrite output
       .arg("-i").arg(audio_path)
       .arg("-af");

    // Build whisper filter arguments
    let mut whisper_filter = format!("whisper=model={}:print_text=1", whisper_model);

    if let Some(lang) = &language {
        whisper_filter.push_str(&format!(":language={}", lang));
    }

    cmd.arg(whisper_filter)
       .arg("-f").arg("null")
       .arg("-")
       .stdout(Stdio::piped())
       .stderr(Stdio::piped());

    emit(RpcEvent::Log {
        id: id.into(),
        message: "Running FFmpeg Whisper transcription...".into()
    });

    let output = cmd.output().await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("FFmpeg Whisper failed: {}", stderr));
    }

    let stderr = String::from_utf8_lossy(&output.stderr);

    emit(RpcEvent::Log {
        id: id.into(),
        message: "Parsing FFmpeg Whisper output...".into()
    });

    // Parse the whisper output from stderr
    let whisper_response = parse_ffmpeg_whisper_output(&stderr)?;

    emit(RpcEvent::Log {
        id: id.into(),
        message: format!("Local FFmpeg Whisper transcription completed. Duration: {:.2}s", whisper_response.duration.unwrap_or(0.0))
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
        return Err(anyhow::anyhow!("No transcription text found in FFmpeg output"));
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
    temp_dir: Option<&std::path::PathBuf>
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

pub async fn transcribe_segments(id: &str, p: TranscribeSegmentsParams, emit: impl FnMut(RpcEvent)) -> anyhow::Result<TranscribeSegmentsResult> {
    transcribe_segments_with_temp(id, p, None, emit).await
}

pub async fn transcribe_segments_with_temp(id: &str, p: TranscribeSegmentsParams, temp_dir: Option<&std::path::PathBuf>, mut emit: impl FnMut(RpcEvent)) -> anyhow::Result<TranscribeSegmentsResult> {
    use reqwest::multipart;
    use mime_guess::MimeGuess;
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

    // Try local whisper.cpp first if available (unless whisper-1 is explicitly selected)
    if !use_openai_directly && USE_LOCAL_WHISPER && is_whisper_cpp_available().await {
        emit(RpcEvent::Log {
            id: id.into(),
            message: "whisper.cpp detected, attempting local transcription...".into()
        });

        match transcribe_with_whisper_cpp(id, &p.audio, p.model.clone(), p.language.clone(), &mut emit).await {
            Ok(whisper_response) => {
                emit(RpcEvent::Log {
                    id: id.into(),
                    message: "Local whisper.cpp transcription successful".into()
                });

                let segments = whisper_to_caption_segments(&whisper_response, p.split_by_words);

                emit(RpcEvent::Log {
                    id: id.into(),
                    message: format!("Converted to {} caption segments (split_by_words={})",
                        segments.len(), p.split_by_words)
                });

                // Save to cache
                if let Err(e) = save_cached_whisper_response(&p.audio, &p, &whisper_response).await {
                    emit(RpcEvent::Log { id: id.into(), message: format!("Failed to cache local transcription: {}", e) });
                }

                // Generate JSON file and return result
                return create_transcription_result(id, &segments, &whisper_response, &p, temp_dir).await;
            }
            Err(e) => {
                let error_msg = if e.to_string().contains("No whisper models found") {
                    format!("No local whisper models available, falling back to OpenAI API. ({})", e)
                } else {
                    format!("Local whisper.cpp failed: {}, falling back to OpenAI API", e)
                };

                emit(RpcEvent::Log {
                    id: id.into(),
                    message: error_msg
                });
            }
        }
    }

    // Try local FFmpeg Whisper as fallback (unless whisper-1 is explicitly selected)
    if !use_openai_directly && USE_LOCAL_WHISPER && is_ffmpeg_whisper_available().await {
        emit(RpcEvent::Log {
            id: id.into(),
            message: "FFmpeg Whisper detected, attempting local transcription...".into()
        });

        match transcribe_with_ffmpeg_whisper(id, &p.audio, p.model.clone(), p.language.clone(), &mut emit).await {
            Ok(whisper_response) => {
                emit(RpcEvent::Log {
                    id: id.into(),
                    message: "Local FFmpeg Whisper transcription successful".into()
                });

                let segments = whisper_to_caption_segments(&whisper_response, p.split_by_words);

                // Save to cache
                if let Err(e) = save_cached_whisper_response(&p.audio, &p, &whisper_response).await {
                    emit(RpcEvent::Log { id: id.into(), message: format!("Failed to cache local transcription: {}", e) });
                }

                // Generate JSON file and return result
                return create_transcription_result(id, &segments, &whisper_response, &p, temp_dir).await;
            }
            Err(e) => {
                emit(RpcEvent::Log {
                    id: id.into(),
                    message: format!("Local FFmpeg Whisper failed: {}, falling back to API", e)
                });
            }
        }
    }

    emit(RpcEvent::Log {
        id: id.into(),
        message: "No local Whisper available, using OpenAI API".into()
    });

    // Fallback to OpenAI API
    let api_key = p.api_key.as_ref().ok_or_else(|| anyhow::anyhow!("OpenAI API key not provided"))?;
    // Always use whisper-1 for OpenAI API (local model names like "tiny" are not valid for the API)
    let model = "whisper-1".to_string();

    let bytes = fs::read(&p.audio).await?;
    let filename = std::path::Path::new(&p.audio).file_name().unwrap_or_default().to_string_lossy().to_string();
    let mime = MimeGuess::from_path(&p.audio).first_or_octet_stream();

    // build form for verbose_json with appropriate timestamp granularities
    let mut form = multipart::Form::new()
        .text("model", model.clone())
        .part("file", multipart::Part::bytes(bytes.clone()).file_name(filename.clone()).mime_str(mime.as_ref()).unwrap())
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

    let client = reqwest::Client::builder().user_agent("core/1.0.0").build()?;

    let resp = client.post("https://api.openai.com/v1/audio/transcriptions")
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
        emit(RpcEvent::Log { id: id.into(), message: format!("Failed to cache transcription: {}", e) });
    }

    create_transcription_result(id, &segments, &whisper_response, &p, temp_dir).await
}


fn is_digits(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

fn format_with_thousands(digits: String) -> String {
    // insert commas every 3 from right
    let mut out = String::new();
    let mut cnt = 0;
    for ch in digits.chars().rev() {
        if cnt > 0 && cnt % 3 == 0 { out.push(','); }
        out.push(ch);
        cnt += 1;
    }
    out.chars().rev().collect()
}

/// Merge currency symbols, thousand-groups, and decimals into single tokens.
/// Handles patterns like ["$", "225", "000"] → "$225,000" and ["19", ".", "99"] → "19.99"
/// Returns (text, start_ms, end_ms) tuples ready for CaptionSegment mapping.
fn merge_numbers_and_currency(
    words: &[WhisperWord],
    max_duration_ms: Option<u64>
) -> Vec<(String, u64, u64)> {
    let mut out = Vec::new();
    let mut i = 0usize;

    while i < words.len() {
        let cur = words[i].word.trim();
        // FIX: words[i].start is already in seconds, convert to ms properly
        let start_ms = (words[i].start * 1000.0) as u64;
        let mut end_ms   = (words[i].end   * 1000.0) as u64;

        if let Some(max_ms) = max_duration_ms {
            if start_ms > max_ms { break; }
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
                        end_ms = ((words[j].end * 1000.0) as u64).min(max_duration_ms.unwrap_or(u64::MAX));
                        j += 1;
                    } else { break; }
                }

                // optional decimal part: "." + 1–2 digits
                if j + 1 < words.len()
                    && words[j].word.trim() == "."
                    && is_digits(words[j + 1].word.trim())
                    && words[j + 1].word.trim().len() <= 2
                {
                    let decimal = words[j + 1].word.trim();
                    end_ms = ((words[j + 1].end * 1000.0) as u64).min(max_duration_ms.unwrap_or(u64::MAX));
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
                    end_ms = ((words[j].end * 1000.0) as u64).min(max_duration_ms.unwrap_or(u64::MAX));
                    j += 1;
                } else { break; }
            }

            // optional decimals
            if j + 1 < words.len()
                && words[j].word.trim() == "."
                && is_digits(words[j + 1].word.trim())
                && words[j + 1].word.trim().len() <= 2
            {
                let decimal = words[j + 1].word.trim();
                end_ms = ((words[j + 1].end * 1000.0) as u64).min(max_duration_ms.unwrap_or(u64::MAX));
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

pub fn whisper_to_caption_segments(response: &WhisperResponse, split_by_words: bool) -> Vec<CaptionSegment> {
    let max_duration_ms = response.duration.map(|d| (d * 1000.0) as u64);

    if split_by_words && response.words.is_some() {
        let words = response.words.as_ref().unwrap();
        let merged = merge_numbers_and_currency(words, max_duration_ms);

        merged.into_iter()
            .filter_map(|(text, start_ms, end_ms)| {
                if end_ms <= start_ms { return None; }
                Some(CaptionSegment {
                    start_ms,
                    end_ms,
                    text,
                    words: Vec::new(),
                })
            })
            .collect()
    } else if split_by_words && response.segments.is_some() {
        // Auto-split segments into words when word-level timestamps are not available
        let segments = response.segments.as_ref().unwrap();
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

                word_segments.push(CaptionSegment {
                    start_ms: word_start_ms,
                    end_ms: word_end_ms,
                    text: word.to_string(),
                    words: Vec::new(),
                });
            }
        }

        word_segments
    } else if let Some(segments) = &response.segments {
        // use segment-level timing
        segments.iter()
            .filter_map(|seg| {
                let start_ms = (seg.start * 1000.0) as u64;
                let end_ms = (seg.end * 1000.0) as u64;

                                // skip segments that are beyond the actual audio duration
                if let Some(max_ms) = max_duration_ms {
                    if start_ms > max_ms {
                        return None;
                    }
                }

                let final_end_ms = if let Some(max_ms) = max_duration_ms {
                    end_ms.min(max_ms)
                } else {
                    end_ms
                };

                // skip segments with very short duration (less than 50ms) - reduced threshold for debugging
                let duration_ms = final_end_ms.saturating_sub(start_ms);
                if duration_ms < 50 {
                    return None;
                }

                Some(CaptionSegment {
                    start_ms,
                    end_ms: final_end_ms,
                    text: seg.text.clone(),
                    words: Vec::new(), // srt-style segments don't include word timing
                })
            })
            .collect()
    } else {
        // fallback: create single segment from full text
        let duration = response.duration.unwrap_or(60.0) * 1000.0;
        vec![CaptionSegment {
            start_ms: 0,
            end_ms: duration as u64,
            text: response.text.clone(),
            words: Vec::new(),
        }]
    }
}


pub async fn get_cached_whisper_response(audio_path: &str, params: &TranscribeSegmentsParams) -> anyhow::Result<Option<WhisperResponse>> {
    let (audio_hash, params_hash) = compute_segments_cache_key(audio_path, params)?;
    let index = load_cache_index().await?;

    for entry in &index.entries {
        if entry.audio_hash == audio_hash && entry.params_hash == params_hash {
            if std::path::Path::new(&entry.response_path).exists() {
                let content = fs::read_to_string(&entry.response_path).await?;
                let response: WhisperResponse = serde_json::from_str(&content)?;
                return Ok(Some(response));
            }
        }
    }
    Ok(None)
}

pub async fn save_cached_whisper_response(audio_path: &str, params: &TranscribeSegmentsParams, response: &WhisperResponse) -> anyhow::Result<()> {
    let (audio_hash, params_hash) = compute_segments_cache_key(audio_path, params)?;
    let mut index = load_cache_index().await?;
    let cache_dir = get_cache_dir()?;
    let timestamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();

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
    index.entries.retain(|e| !(e.audio_hash == new_entry.audio_hash && e.params_hash == new_entry.params_hash));

    // add new entry
    index.entries.push(new_entry);

    // keep only 4 most recent entries (LRU eviction)
    if index.entries.len() > 4 {
        index.entries.sort_by_key(|e| e.timestamp);
        let to_remove = index.entries.drain(0..index.entries.len() - 4).collect::<Vec<_>>();

        // delete old cached files
        for entry in to_remove {
            let _ = fs::remove_file(&entry.response_path).await;
        }
    }

    save_cache_index(&index).await?;
    Ok(())
}


pub fn compute_segments_cache_key(audio_path: &str, params: &TranscribeSegmentsParams) -> anyhow::Result<(String, String)> {
    // hash audio file content
    let audio_bytes = std::fs::read(audio_path)?;
    let audio_hash = blake3::hash(&audio_bytes).to_hex().to_string();

    // hash relevant parameters (excluding video_file as it doesn't affect transcription)
    let params_for_hash = serde_json::json!({
        "model": params.model,
        "language": params.language,
        "split_by_words": params.split_by_words,
        "prompt": params.prompt,
    });
    let params_hash = blake3::hash(params_for_hash.to_string().as_bytes()).to_hex().to_string();

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
        Ok(serde_json::from_str(&content).unwrap_or(WhisperCacheIndex { entries: Vec::new() }))
    } else {
        Ok(WhisperCacheIndex { entries: Vec::new() })
    }
}

pub fn get_cache_dir() -> std::io::Result<PathBuf> {
    let mut cache_dir = std::env::temp_dir();
    cache_dir.push("capslap_whisper_cache");
    std::fs::create_dir_all(&cache_dir)?;
    Ok(cache_dir)
}
