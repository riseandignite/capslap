use crate::{types::{CaptionSegment, WhisperResponse, WhisperCacheEntry, WhisperCacheIndex, TranscribeSegmentsParams, TranscribeSegmentsResult, WhisperWord}};
use blake3;
use tokio::fs;
use std::path::PathBuf;
use crate::rpc::RpcEvent;

pub async fn transcribe_segments(id: &str, p: TranscribeSegmentsParams, emit: impl FnMut(RpcEvent)) -> anyhow::Result<TranscribeSegmentsResult> {
    transcribe_segments_with_temp(id, p, None, emit).await
}

pub async fn transcribe_segments_with_temp(id: &str, p: TranscribeSegmentsParams, temp_dir: Option<&std::path::PathBuf>, mut emit: impl FnMut(RpcEvent)) -> anyhow::Result<TranscribeSegmentsResult> {
    use reqwest::multipart;
    use mime_guess::MimeGuess;
    use tokio::fs;

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

    let api_key = p.api_key.as_ref().ok_or_else(|| anyhow::anyhow!("OpenAI API key not provided"))?;
    let model = p.model.clone().unwrap_or_else(|| "whisper-1".to_string());

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

        // save to cache
    if let Err(e) = save_cached_whisper_response(&p.audio, &p, &whisper_response).await {
        emit(RpcEvent::Log { id: id.into(), message: format!("failed to cache transcription: {}", e) });
    }

    // generate JSON file path based on temp directory (or video file location if no temp dir)
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

    // create JSON export data
    let json_data = serde_json::json!({
        "segments": segments,
        "fullText": whisper_response.text,
        "duration": whisper_response.duration,
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

    Ok(TranscribeSegmentsResult {
        segments,
        full_text: whisper_response.text,
        duration: whisper_response.duration,
        json_file: json_path,
    })
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
