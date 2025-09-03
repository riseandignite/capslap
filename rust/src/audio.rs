use crate::rpc::RpcEvent;
use crate::types::{ExtractAudioParams, ExtractAudioResult};
use crate::video::probe;
use std::path::PathBuf;
use tokio::process::Command as TokioCommand;

pub async fn extract_audio(id: &str, p: ExtractAudioParams, mut emit: impl FnMut(RpcEvent)) -> anyhow::Result<ExtractAudioResult> {
    let out = p.out.unwrap_or_else(|| {
        let mut pb = PathBuf::from(&p.input);
        pb.set_extension("m4a");
        pb.to_string_lossy().to_string()
    });

    let target_codec = p.codec.unwrap_or_else(|| "aac".to_string());

    // Probe input to determine if we can use stream copy
    let use_copy = if let Ok(probe_result) = probe(id, &p.input, &mut emit).await {
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
            message: "Using stream copy for audio extraction (no re-encoding needed)".into()
        });
        "copy"
    } else {
        emit(RpcEvent::Log {
            id: id.into(),
            message: format!("Re-encoding audio to {}", target_codec).into()
        });
        &target_codec
    };

    let mut cmd = TokioCommand::new("ffmpeg");
    cmd.arg("-y")
       .arg("-i").arg(&p.input)
       .arg("-vn")
       .arg("-acodec").arg(audio_codec);

    // Add explicit bitrate only when re-encoding
    if !use_copy && target_codec == "aac" {
        cmd.arg("-b:a").arg("160k");   // Explicit AAC bitrate for quality
    }

    cmd.arg(&out);

    let status = cmd.status().await?;
    if !status.success() {
        return Err(anyhow::anyhow!("ffmpeg audio extraction failed"));
    }
    Ok(ExtractAudioResult { audio: out })
}
