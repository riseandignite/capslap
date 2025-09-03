use core::rpc::{RpcRequest, RpcResponse, RpcError, RpcEvent, new_id};
use core::captions;
use std::io::{self, BufRead, Write};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let stdin = io::stdin();
    let mut tasks = tokio::task::JoinSet::new();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() { continue; }

        let req: Result<RpcRequest, _> = serde_json::from_str(&line);
        match req {
            Ok(r) => {
                // Spawn each request as a concurrent task
                tasks.spawn(async move {
                    handle_request(r).await
                });
            }
            Err(e) => {
                let err = serde_json::json!({ "id": new_id(), "error": format!("Bad request: {}", e) });
                println!("{}", err);
                let _ = io::stdout().flush();
            }
        }
    }

    // Wait for all tasks to complete (though this won't be reached in normal operation)
    while let Some(_) = tasks.join_next().await {}
    Ok(())
}

async fn handle_request(r: RpcRequest) {
    let id = r.id.clone();

    // Emit progress/log events — no captured stdout handle.
    let mut emit = |ev: RpcEvent| {
        println!("{}", serde_json::to_string(&ev).unwrap());
        let _ = io::stdout().flush();
    };

    let write_ok = |value: serde_json::Value| {
        let resp = RpcResponse { id: id.clone(), result: value };
        println!("{}", serde_json::to_string(&resp).unwrap());
        let _ = io::stdout().flush();
    };

    let write_err = |e: String| {
        let err = RpcError { id: id.clone(), error: e };
        println!("{}", serde_json::to_string(&err).unwrap());
        let _ = io::stdout().flush();
    };

    match r.method.as_str() {
        "ping" => write_ok(serde_json::json!({"ok": true})),
        "generateCaptions" => {
            let p: core::types::GenerateCaptionsParams = serde_json::from_value(r.params).unwrap();
            match captions::generate_captions(&id, p, &mut emit).await {
                Ok(v) => write_ok(serde_json::to_value(v).unwrap()),
                Err(e) => write_err(e.to_string()),
            }
        }
        _ => write_err("Unknown method".into()),
    }
}
