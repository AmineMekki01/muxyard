//! Shared fixture compatibility and hostile JSON boundary tests.

use muxyard_contracts::{
    AgentStatus, ApiError, ControlEnvelope, ErrorCode, ExecutionEnvironment, MAX_CONTROL_BYTES,
    TaskState, decode_control,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

const REQUEST: &[u8] = include_bytes!("../../../tests/fixtures/contracts/request.json");
const RESPONSE: &[u8] = include_bytes!("../../../tests/fixtures/contracts/response.json");
const EVENT: &[u8] = include_bytes!("../../../tests/fixtures/contracts/event.json");
const TASK: &[u8] = include_bytes!("../../../tests/fixtures/contracts/task.json");

fn assert_roundtrip<T: DeserializeOwned + Serialize>(input: &[u8]) {
    let decoded: ControlEnvelope<T> = decode_control(input).unwrap();
    assert_eq!(
        serde_json::to_value(decoded).unwrap(),
        serde_json::from_slice::<Value>(input).unwrap()
    );
}

#[test]
fn typed_fixtures_roundtrip_with_exact_wire_keys_and_correlation() {
    assert_roundtrip::<ExecutionEnvironment>(REQUEST);
    assert_roundtrip::<ApiError>(RESPONSE);
    assert_roundtrip::<AgentStatus>(EVENT);
    assert_roundtrip::<TaskState>(TASK);
    let request = decode_control::<ExecutionEnvironment>(REQUEST).unwrap();
    let response = decode_control::<ApiError>(RESPONSE).unwrap();
    assert_eq!(request.id, response.id);
}

#[test]
fn shared_invalid_fixtures_return_stable_safe_errors() {
    #[derive(Deserialize)]
    struct Invalid {
        name: String,
        input: String,
        code: ErrorCode,
    }
    let fixtures: Vec<Invalid> = serde_json::from_str(include_str!(
        "../../../tests/fixtures/contracts/invalid.json"
    ))
    .unwrap();
    for fixture in fixtures {
        let error = decode_control::<TaskState>(fixture.input.as_bytes()).unwrap_err();
        assert_eq!(error.code, fixture.code, "{}", fixture.name);
        assert!(!error.message.contains("secret"));
    }
}

#[test]
fn byte_limit_is_inclusive_and_checked_before_json_parsing() {
    let mut input = TASK.to_vec();
    input.resize(MAX_CONTROL_BYTES, b' ');
    assert!(decode_control::<TaskState>(&input).is_ok());
    input.push(b' ');
    assert_eq!(
        decode_control::<TaskState>(&input).unwrap_err().code,
        ErrorCode::PayloadTooLarge
    );
    assert_eq!(
        decode_control::<TaskState>(&vec![0xff; MAX_CONTROL_BYTES + 1])
            .unwrap_err()
            .code,
        ErrorCode::PayloadTooLarge
    );
}

#[test]
fn nullable_payloads_still_require_the_data_field() {
    let mut message: Value = serde_json::from_slice(TASK).unwrap();
    message["data"] = Value::Null;
    assert!(decode_control::<Option<TaskState>>(&serde_json::to_vec(&message).unwrap()).is_ok());
    message.as_object_mut().unwrap().remove("data");
    assert_eq!(
        decode_control::<Option<TaskState>>(&serde_json::to_vec(&message).unwrap())
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
}

#[test]
fn malformed_versions_utf8_and_trailing_content_are_rejected() {
    for version in [json!(-1), json!(1.0), json!(null), json!(true)] {
        let mut message: Value = serde_json::from_slice(TASK).unwrap();
        message["v"] = version;
        assert_eq!(
            decode_control::<TaskState>(&serde_json::to_vec(&message).unwrap())
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
    }
    let mut trailing = TASK.to_vec();
    trailing.extend_from_slice(b"{}");
    for bytes in [&[0xff][..], &trailing, &b"[]"[..], &b""[..]] {
        assert_eq!(
            decode_control::<TaskState>(bytes).unwrap_err().code,
            ErrorCode::InvalidRequest
        );
    }
}

#[test]
fn all_execution_environments_remain_distinct_and_structurally_checked() {
    for environment in [
        json!({"kind": "native-mac"}),
        json!({"kind": "native-linux"}),
        json!({"kind": "native-windows"}),
        json!({"kind": "wsl", "distro": "Ubuntu-24.04"}),
    ] {
        let decoded: ExecutionEnvironment = serde_json::from_value(environment.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), environment);
    }
    for invalid in [
        json!({"kind": "wsl"}),
        json!({"kind": "native-windows", "distro": "Ubuntu"}),
        json!({"kind": "native-mac", "unexpected": true}),
        json!({"kind": "other-os"}),
    ] {
        assert!(serde_json::from_value::<ExecutionEnvironment>(invalid).is_err());
    }
}

#[test]
fn lifecycle_vocabulary_matches_the_frozen_contract() {
    for state in [
        "draft",
        "ready",
        "running",
        "blocked",
        "review",
        "approved",
        "integrated",
        "failed",
        "canceled",
    ] {
        let decoded: TaskState = serde_json::from_value(json!(state)).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), json!(state));
    }
    for status in [
        "starting",
        "working",
        "idle",
        "needs_input",
        "needs_approval",
        "turn_finished",
        "failed",
        "exited",
        "unknown",
    ] {
        let decoded: AgentStatus = serde_json::from_value(json!(status)).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), json!(status));
    }
    assert!(serde_json::from_value::<AgentStatus>(json!("approved")).is_err());
}

#[test]
fn every_error_code_serializes_without_changing_its_spelling() {
    for code in [
        "UNAUTHORIZED",
        "FORBIDDEN",
        "NOT_FOUND",
        "CONFLICT",
        "UNSUPPORTED_CAPABILITY",
        "STALE_REVISION",
        "HOST_OFFLINE",
        "RATE_LIMITED",
        "PROVIDER_ERROR",
        "INTERNAL",
        "UNSUPPORTED_VERSION",
        "INVALID_REQUEST",
        "PAYLOAD_TOO_LARGE",
    ] {
        let decoded: ErrorCode = serde_json::from_value(json!(code)).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), json!(code));
    }
    assert!(serde_json::from_value::<ErrorCode>(json!("UNKNOWN_FAILURE")).is_err());
}

#[test]
fn recursive_or_duplicate_fields_do_not_bypass_validation() {
    let recursive = format!(
        r#"{{"v":1,"id":"id","type":"request","method":"fixture","data":{}0{}}}"#,
        "[".repeat(130),
        "]".repeat(130)
    );
    assert_eq!(
        decode_control::<Value>(recursive.as_bytes())
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    let duplicate =
        br#"{"v":1,"id":"a","id":"b","type":"request","method":"fixture","data":"ready"}"#;
    assert_eq!(
        decode_control::<TaskState>(duplicate).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
}
