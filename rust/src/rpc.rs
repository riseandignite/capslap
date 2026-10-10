use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug)]
pub struct RpcRequest {
    pub id: String,     // Unique identifier to match requests with responses
    pub method: String, // What operation to perform (e.g., "extractAudio", "probe")
    #[serde(default)] // If params is missing in JSON, use default (empty JSON object)
    pub params: serde_json::Value, // The input data needed for the operation
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RpcResponse<T> {
    pub id: String, // Same ID as the request, so Electron knows which request this responds to
    pub result: T,  // The actual result data (varies by operation)
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RpcError {
    pub id: String,    // Same ID as the request that failed
    pub error: String, // Human-readable error message explaining what went wrong
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "event", rename_all = "camelCase")] // JSON will have an "event" field indicating the type
pub enum RpcEvent {
    // Progress updates for long operations (0.0 to 1.0 completion)
    Progress {
        id: String,     // ID of the operation being tracked
        status: String, // Human-readable status message ("Extracting audio...")
        progress: f32,  // Completion percentage (0.0 = 0%, 1.0 = 100%)
    },
    // Log messages for debugging or information
    Log {
        id: String,      // ID of the operation
        message: String, // The log message content
    },
}

pub fn new_id() -> String {
    Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================
    // RpcRequest serialization tests
    // ============================================

    #[test]
    fn test_rpc_request_serialization() {
        let req = RpcRequest {
            id: "test-123".to_string(),
            method: "probe".to_string(),
            params: serde_json::json!({"path": "/test.mp4"}),
        };

        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("test-123"));
        assert!(json.contains("probe"));
        assert!(json.contains("/test.mp4"));
    }

    #[test]
    fn test_rpc_request_deserialization() {
        let json = r#"{"id":"req-456","method":"transcribe","params":{"audio":"/audio.wav"}}"#;
        let req: RpcRequest = serde_json::from_str(json).unwrap();

        assert_eq!(req.id, "req-456");
        assert_eq!(req.method, "transcribe");
        assert_eq!(req.params["audio"], "/audio.wav");
    }

    #[test]
    fn test_rpc_request_default_params() {
        // If params is missing, it should default to null/empty
        let json = r#"{"id":"req-789","method":"ping"}"#;
        let req: RpcRequest = serde_json::from_str(json).unwrap();

        assert_eq!(req.id, "req-789");
        assert_eq!(req.method, "ping");
        assert!(req.params.is_null());
    }

    #[test]
    fn test_rpc_request_roundtrip() {
        let original = RpcRequest {
            id: "roundtrip-test".to_string(),
            method: "export".to_string(),
            params: serde_json::json!({
                "input": "/video.mp4",
                "format": "9:16",
                "quality": 18
            }),
        };

        let json = serde_json::to_string(&original).unwrap();
        let parsed: RpcRequest = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.id, original.id);
        assert_eq!(parsed.method, original.method);
        assert_eq!(parsed.params["format"], "9:16");
    }

    // ============================================
    // RpcResponse serialization tests
    // ============================================

    #[test]
    fn test_rpc_response_serialization() {
        let resp = RpcResponse {
            id: "resp-123".to_string(),
            result: serde_json::json!({"duration": 10.5, "width": 1920}),
        };

        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("resp-123"));
        assert!(json.contains("10.5"));
        assert!(json.contains("1920"));
    }

    #[test]
    fn test_rpc_response_deserialization() {
        let json = r#"{"id":"resp-456","result":{"success":true}}"#;
        let resp: RpcResponse<serde_json::Value> = serde_json::from_str(json).unwrap();

        assert_eq!(resp.id, "resp-456");
        assert_eq!(resp.result["success"], true);
    }

    #[test]
    fn test_rpc_response_typed_result() {
        #[derive(Serialize, Deserialize, Debug, PartialEq)]
        struct ProbeResult {
            duration: f64,
            has_audio: bool,
        }

        let resp = RpcResponse {
            id: "typed-resp".to_string(),
            result: ProbeResult {
                duration: 120.5,
                has_audio: true,
            },
        };

        let json = serde_json::to_string(&resp).unwrap();
        let parsed: RpcResponse<ProbeResult> = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.result.duration, 120.5);
        assert!(parsed.result.has_audio);
    }

    // ============================================
    // RpcError serialization tests
    // ============================================

    #[test]
    fn test_rpc_error_serialization() {
        let err = RpcError {
            id: "err-123".to_string(),
            error: "File not found".to_string(),
        };

        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("err-123"));
        assert!(json.contains("File not found"));
    }

    #[test]
    fn test_rpc_error_deserialization() {
        let json = r#"{"id":"err-456","error":"Invalid format"}"#;
        let err: RpcError = serde_json::from_str(json).unwrap();

        assert_eq!(err.id, "err-456");
        assert_eq!(err.error, "Invalid format");
    }

    // ============================================
    // RpcEvent serialization tests
    // ============================================

    #[test]
    fn test_rpc_event_progress_serialization() {
        let event = RpcEvent::Progress {
            id: "op-1".to_string(),
            status: "Processing...".to_string(),
            progress: 0.5,
        };

        let json = serde_json::to_string(&event).unwrap();
        // Should use camelCase for event type
        assert!(json.contains("\"event\":\"progress\""));
        assert!(json.contains("0.5"));
        assert!(json.contains("Processing"));
    }

    #[test]
    fn test_rpc_event_log_serialization() {
        let event = RpcEvent::Log {
            id: "op-2".to_string(),
            message: "Starting export".to_string(),
        };

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"event\":\"log\""));
        assert!(json.contains("Starting export"));
    }

    #[test]
    fn test_rpc_event_progress_deserialization() {
        let json = r#"{"event":"progress","id":"op-3","status":"Encoding...","progress":0.75}"#;
        let event: RpcEvent = serde_json::from_str(json).unwrap();

        match event {
            RpcEvent::Progress {
                id,
                status,
                progress,
            } => {
                assert_eq!(id, "op-3");
                assert_eq!(status, "Encoding...");
                assert!((progress - 0.75).abs() < 0.001);
            }
            _ => panic!("Expected Progress event"),
        }
    }

    #[test]
    fn test_rpc_event_log_deserialization() {
        let json = r#"{"event":"log","id":"op-4","message":"Done!"}"#;
        let event: RpcEvent = serde_json::from_str(json).unwrap();

        match event {
            RpcEvent::Log { id, message } => {
                assert_eq!(id, "op-4");
                assert_eq!(message, "Done!");
            }
            _ => panic!("Expected Log event"),
        }
    }

    // ============================================
    // new_id tests
    // ============================================

    #[test]
    fn test_new_id_is_uuid_format() {
        let id = new_id();
        // UUID v4 format: 8-4-4-4-12 = 36 characters
        assert_eq!(id.len(), 36);
        assert_eq!(id.matches('-').count(), 4);
    }

    #[test]
    fn test_new_id_is_unique() {
        let id1 = new_id();
        let id2 = new_id();
        let id3 = new_id();

        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_new_id_is_valid_uuid() {
        let id = new_id();
        // Should be parseable as a UUID
        let parsed = uuid::Uuid::parse_str(&id);
        assert!(parsed.is_ok());
    }
}
