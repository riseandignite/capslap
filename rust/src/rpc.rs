use serde::{Deserialize, Serialize};
use uuid::Uuid;


#[derive(Serialize, Deserialize, Debug)]
pub struct RpcRequest {
    pub id: String,                    // Unique identifier to match requests with responses
    pub method: String,                // What operation to perform (e.g., "extractAudio", "probe")
    #[serde(default)]                  // If params is missing in JSON, use default (empty JSON object)
    pub params: serde_json::Value,     // The input data needed for the operation
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RpcResponse<T> {
    pub id: String,     // Same ID as the request, so Electron knows which request this responds to
    pub result: T,      // The actual result data (varies by operation)
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RpcError {
    pub id: String,      // Same ID as the request that failed
    pub error: String,   // Human-readable error message explaining what went wrong
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "event", rename_all = "camelCase")]  // JSON will have an "event" field indicating the type
pub enum RpcEvent {
    // Progress updates for long operations (0.0 to 1.0 completion)
    Progress {
        id: String,       // ID of the operation being tracked
        status: String,   // Human-readable status message ("Extracting audio...")
        progress: f32     // Completion percentage (0.0 = 0%, 1.0 = 100%)
    },
    // Log messages for debugging or information
    Log {
        id: String,       // ID of the operation
        message: String   // The log message content
    },
}

pub fn new_id() -> String {
    Uuid::new_v4().to_string()
}
