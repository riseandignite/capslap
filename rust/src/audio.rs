use crate::rpc::RpcEvent;
use crate::types::{ExtractAudioParams, ExtractAudioResult};
use crate::video::probe;
use std::path::PathBuf;
use tokio::process::Command as TokioCommand;

pub async fn extract_audio(
    id: &str,
    p: ExtractAudioParams,
    mut emit: impl FnMut(RpcEvent),
) -> anyhow::Result<ExtractAudioResult> {
    let out = p.out.unwrap_or_else(|| {
        let mut pb = PathBuf::from(&p.input);
        pb.set_extension("m4a");
        pb.to_string_lossy().to_string()
    });

    let target_codec = p.codec.unwrap_or_else(|| "aac".to_string());

    // Probe input to determine if we can use stream copy
    // A part of the input is always re-encoded: a stream copy cuts at packets.
    let use_copy = p.range.is_none()
        && if let Ok(probe_result) = probe(id, &p.input, &mut emit).await {
            if let Some(audio_codec) = &probe_result.audio_codec {
                let codec_lower = audio_codec.to_lowercase();
                match target_codec.as_str() {
                    "aac" => codec_lower == "aac",
                    "mp3" => codec_lower == "mp3",
                    "m4a" => codec_lower == "aac", // m4a container typically uses AAC
                    _ => false,
                }
            } else {
                false
            }
        } else {
            false
        };

    let audio_codec = if use_copy {
        emit(RpcEvent::Log {
            id: id.into(),
            message: "Using stream copy for audio extraction (no re-encoding needed)".into(),
        });
        "copy"
    } else {
        emit(RpcEvent::Log {
            id: id.into(),
            message: format!("Re-encoding audio to {}", target_codec),
        });
        &target_codec
    };

    let ffmpeg_path = crate::whisper::find_ffmpeg_binary()
        .await
        .map_err(|e| anyhow::anyhow!("FFmpeg not found: {}", e))?;
    let mut cmd = TokioCommand::new(ffmpeg_path);
    cmd.kill_on_drop(true);
    cmd.arg("-y");
    // Seeking before -i is exact when the audio is decoded anyway.
    if let Some(range) = &p.range {
        cmd.arg("-ss").arg(format!("{:.3}", range.start_sec));
    }
    cmd.arg("-i").arg(&p.input);
    if let Some(range) = &p.range {
        cmd.arg("-t").arg(format!("{:.3}", range.duration_sec));
    }
    cmd.arg("-vn").arg("-acodec").arg(audio_codec);

    // Add explicit bitrate/sampling options when re-encoding
    if !use_copy {
        if target_codec == "aac" {
            cmd.arg("-b:a").arg("160k"); // Explicit AAC bitrate for quality
        } else if target_codec == "pcm_s16le" || target_codec == "wav" {
            cmd.arg("-ar").arg("16000").arg("-ac").arg("1");
        }
    }

    cmd.arg(&out);

    let status = cmd.status().await?;
    if !status.success() {
        return Err(anyhow::anyhow!("ffmpeg audio extraction failed"));
    }
    Ok(ExtractAudioResult { audio: out })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::AudioRange;

    /// 16 kHz mono 16-bit: 32 000 bytes a second, after a 44-byte header.
    fn seconds_of(wav: &std::path::Path) -> f64 {
        (std::fs::metadata(wav).unwrap().len() as f64 - 44.0) / 32_000.0
    }

    async fn extract(input: &std::path::Path, out: &std::path::Path, range: Option<AudioRange>) {
        extract_audio(
            "t",
            ExtractAudioParams {
                input: input.to_string_lossy().to_string(),
                codec: Some("pcm_s16le".to_string()),
                out: Some(out.to_string_lossy().to_string()),
                range,
            },
            |_| {},
        )
        .await
        .expect("extraction works");
    }

    fn five_second_tone(dir: &std::path::Path) -> std::path::PathBuf {
        let wav = dir.join("episode.wav");
        let made = std::process::Command::new(crate::video::get_ffmpeg_path_sync())
            .args(["-v", "error", "-y", "-f", "lavfi", "-i", "sine=duration=5"])
            .arg(&wav)
            .status()
            .expect("ffmpeg runs");
        assert!(made.success());
        wav
    }

    #[tokio::test]
    async fn a_range_extracts_only_that_part() {
        let dir = tempfile::tempdir().unwrap();
        let wav = five_second_tone(dir.path());
        let whole = dir.path().join("whole.wav");
        let part = dir.path().join("part.wav");

        extract(&wav, &whole, None).await;
        extract(
            &wav,
            &part,
            Some(AudioRange {
                start_sec: 1.0,
                duration_sec: 2.0,
            }),
        )
        .await;

        assert!((seconds_of(&whole) - 5.0).abs() < 0.1);
        assert!((seconds_of(&part) - 2.0).abs() < 0.1);
    }

    #[tokio::test]
    async fn a_range_past_the_end_stops_at_the_end() {
        let dir = tempfile::tempdir().unwrap();
        let wav = five_second_tone(dir.path());
        let part = dir.path().join("tail.wav");
        extract(
            &wav,
            &part,
            Some(AudioRange {
                start_sec: 4.0,
                duration_sec: 60.0,
            }),
        )
        .await;
        assert!((seconds_of(&part) - 1.0).abs() < 0.1);
    }
}
