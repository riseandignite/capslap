use core::captions;
use core::rpc::{new_id, RpcError, RpcEvent, RpcRequest, RpcResponse};
use std::io::{self, BufRead, Write};

// Shared cancellation map: request_id -> cancellation_sender
type CancelMap = std::sync::Arc<
    std::sync::Mutex<std::collections::HashMap<String, tokio::sync::broadcast::Sender<()>>>,
>;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Install panic hook to diagnose silent crashes
    std::panic::set_hook(Box::new(|info| {
        let msg = match info.payload().downcast_ref::<&'static str>() {
            Some(s) => *s,
            None => match info.payload().downcast_ref::<String>() {
                Some(s) => &**s,
                None => "Box<Any>",
            },
        };
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown".to_string());
        let log_path = "/tmp/capslap-panic.log";
        let _ = std::fs::write(log_path, format!("Panic occurred at {}: {}", location, msg));
    }));

    let stdin = io::stdin();
    let mut tasks = tokio::task::JoinSet::new();
    let cancel_map: CancelMap =
        std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        let req: Result<RpcRequest, _> = serde_json::from_str(&line);
        match req {
            Ok(r) => {
                let cancel_map = cancel_map.clone();
                // Spawn each request as a concurrent task
                tasks.spawn(async move { handle_request(r, cancel_map).await });
            }
            Err(e) => {
                let err =
                    serde_json::json!({ "id": new_id(), "error": format!("Bad request: {}", e) });
                println!("{}", err);
                let _ = io::stdout().flush();
            }
        }
    }

    // Wait for all tasks to complete (though this won't be reached in normal operation)
    while tasks.join_next().await.is_some() {}
    Ok(())
}

fn emit_event(ev: RpcEvent) {
    let mut out = io::stdout().lock();
    let _ = serde_json::to_writer(&mut out, &ev);
    let _ = out.write_all(b"\n");
    let _ = out.flush();
}

fn write_ok<T: serde::Serialize>(id: &str, value: &T) {
    let resp = RpcResponse {
        id: id.to_string(),
        result: value,
    };
    let mut out = io::stdout().lock();
    let _ = serde_json::to_writer(&mut out, &resp);
    let _ = out.write_all(b"\n");
    let _ = out.flush();
}

fn write_err(id: &str, e: String) {
    let err = RpcError {
        id: id.to_string(),
        error: e,
    };
    let mut out = io::stdout().lock();
    let _ = serde_json::to_writer(&mut out, &err);
    let _ = out.write_all(b"\n");
    let _ = out.flush();
}

async fn handle_request(r: RpcRequest, cancel_map: CancelMap) {
    let id = r.id.clone();
    let mut emit = emit_event;

    // Setup cancellation token for this request
    let (tx, mut rx) = tokio::sync::broadcast::channel(1);
    {
        let mut map = cancel_map.lock().unwrap();
        map.insert(id.clone(), tx);
    }

    // Ensure cleanup of cancellation token on drop
    struct CleanupGuard {
        id: String,
        map: CancelMap,
    }
    impl Drop for CleanupGuard {
        fn drop(&mut self) {
            let mut map = self.map.lock().unwrap();
            map.remove(&self.id);
        }
    }
    let _guard = CleanupGuard {
        id: id.clone(),
        map: cancel_map.clone(),
    };

    match r.method.as_str() {
        "ping" => write_ok(&id, &serde_json::json!({"ok": true})),
        "cancel" => {
            // New cancel method
            if let Some(target_id) = r.params.as_str() {
                let map = cancel_map.lock().unwrap();
                if let Some(tx) = map.get(target_id) {
                    let _ = tx.send(()); // Send cancellation signal
                    write_ok(&id, &serde_json::json!({ "cancelled": true }));
                } else {
                    write_err(&id, format!("Task with id {} not found", target_id));
                }
            } else {
                write_err(
                    &id,
                    "Invalid params for cancel, expected string id".to_string(),
                );
            }
        }
        "generateCaptions" => {
            match serde_json::from_value::<core::types::GenerateCaptionsParams>(r.params) {
                Ok(p) => {
                    tokio::select! {
                        res = captions::generate_captions(&id, p, &mut emit) => {
                            match res {
                                Ok(v) => write_ok(&id, &v),
                                Err(e) => write_err(&id, e.to_string()),
                            }
                        }
                        _ = rx.recv() => {
                            write_err(&id, "Cancelled".to_string());
                        }
                    }
                }
                Err(e) => write_err(&id, format!("Invalid params for generateCaptions: {}", e)),
            }
        }
        "downloadModel" => {
            match serde_json::from_value::<core::types::DownloadModelParams>(r.params) {
                Ok(p) => {
                    tokio::select! {
                       res = core::whisper::download_model_rpc(&id, p, &mut emit) => {
                            match res {
                               Ok(v) => write_ok(&id, &v),
                               Err(e) => write_err(&id, e.to_string()),
                           }
                       }
                       _ = rx.recv() => {
                            write_err(&id, "Cancelled".to_string());
                       }
                    }
                }
                Err(e) => write_err(&id, format!("Invalid params for downloadModel: {}", e)),
            }
        }
        "checkModelExists" => match serde_json::from_value::<String>(r.params) {
            Ok(model_name) => match core::whisper::check_model_exists(&model_name) {
                Ok(exists) => write_ok(&id, &exists),
                Err(e) => write_err(&id, e.to_string()),
            },
            Err(e) => write_err(&id, format!("Invalid params for checkModelExists: {}", e)),
        },
        "extractFirstFrame" => {
            match serde_json::from_value::<core::types::ExtractFirstFrameParams>(r.params) {
                Ok(p) => match core::video::extract_first_frame(&p.video_path) {
                    Ok(base64_img) => write_ok(
                        &id,
                        &core::types::ExtractFirstFrameResult {
                            image_data: base64_img,
                        },
                    ),
                    Err(e) => write_err(&id, e.to_string()),
                },
                Err(e) => write_err(&id, format!("Invalid params for extractFirstFrame: {}", e)),
            }
        }
        "transcribe" => {
            match serde_json::from_value::<core::types::GenerateCaptionsParams>(r.params) {
                Ok(p) => {
                    let range = core::types::AudioRange::from_ms(
                        p.transcribe_start_ms,
                        p.transcribe_end_ms,
                    );
                    tokio::select! {
                        res = captions::extract_and_transcribe_with_server(
                            &id,
                            &p.input_video,
                            p.split_by_words,
                            p.model,
                            p.language,
                            p.api_key,
                            p.prompt,
                            p.whisper_base_url,
                            range,
                            &mut emit
                        ) => {
                            match res {
                                Ok((probe, audio, transcription)) => {
                                    write_ok(&id, &serde_json::json!({
                                        "probeResult": probe,
                                        "audioFile": audio,
                                        "transcription": transcription
                                    }));
                                },
                                Err(e) => write_err(&id, e.to_string()),
                            }
                        }
                        _ = rx.recv() => {
                            write_err(&id, "Cancelled".to_string());
                        }
                    }
                }
                Err(e) => write_err(&id, format!("Invalid params for transcribe: {}", e)),
            }
        }
        "burn" => match serde_json::from_value::<core::types::BurnCaptionsParams>(r.params) {
            Ok(p) => {
                tokio::select! {
                    res = captions::burn_captions_with_segments(&id, p, &mut emit) => {
                        match res {
                            Ok(v) => write_ok(&id, &v),
                            Err(e) => write_err(&id, e.to_string()),
                        }
                    }
                    _ = rx.recv() => {
                        write_err(&id, "Cancelled".to_string());
                    }
                }
            }
            Err(e) => write_err(&id, format!("Invalid params for burn: {}", e)),
        },
        "previewLayout" => {
            match serde_json::from_value::<core::types::PreviewLayoutParams>(r.params) {
                Ok(p) => match captions::generate_preview_layout(p) {
                    Ok(v) => write_ok(&id, &v),
                    Err(e) => write_err(&id, e.to_string()),
                },
                Err(e) => write_err(&id, format!("Invalid params for previewLayout: {}", e)),
            }
        }
        "saveCaptions" => {
            match serde_json::from_value::<core::types::SaveCaptionsParams>(r.params) {
                Ok(p) => match captions::save_captions(p) {
                    Ok(_) => write_ok(&id, &serde_json::json!({ "ok": true })),
                    Err(e) => write_err(&id, e.to_string()),
                },
                Err(e) => write_err(&id, format!("Invalid params for saveCaptions: {}", e)),
            }
        }
        "loadCaptions" => {
            match serde_json::from_value::<core::types::LoadCaptionsParams>(r.params) {
                Ok(p) => match captions::load_captions(p) {
                    Ok(v) => write_ok(&id, &v),
                    Err(e) => write_err(&id, e.to_string()),
                },
                Err(e) => write_err(&id, format!("Invalid params for loadCaptions: {}", e)),
            }
        }
        "autoPlaceCaptions" => {
            match serde_json::from_value::<core::types::AutoPlaceParams>(r.params) {
                Ok(p) => match core::placement::auto_place_captions(p).await {
                    Ok(v) => write_ok(&id, &v),
                    Err(e) => write_err(&id, e.to_string()),
                },
                Err(e) => write_err(&id, format!("Invalid params for autoPlaceCaptions: {}", e)),
            }
        }
        "generatePreviewFrame" => {
            match serde_json::from_value::<core::types::PreviewFrameParams>(r.params) {
                Ok(p) => match captions::generate_preview_frame(p).await {
                    Ok(v) => write_ok(&id, &v),
                    Err(e) => write_err(&id, e.to_string()),
                },
                Err(e) => write_err(
                    &id,
                    format!("Invalid params for generatePreviewFrame: {}", e),
                ),
            }
        }
        _ => write_err(&id, format!("Unknown method: {}", r.method)),
    }
}
