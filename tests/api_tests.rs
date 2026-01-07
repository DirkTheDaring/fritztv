use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use tower::util::ServiceExt;
use fritztv::channels::Channel;

// We need to import the create_app logic.
// However, currently create_app is in main.rs and not pub mod.
// For integration testing, usually we expose `app()` via `lib.rs` or `main.rs` as a library.
// Since `main.rs` defines the binary target, we can't easily import from it in `tests/*.rs` integration tests
// unless we also define a [lib] in Cargo.toml that includes main.rs (bad practice) or move logic to lib.rs.
// For simplicity, I will refactor main.rs content to lib.rs later? 
// OR I'll assume the user is okay with me moving the logic to `src/lib.rs` and having `src/main.rs` just call it.

// Let's create `src/lib.rs` with the shared logic first in the next step.
// But first, let's just write what I WANT to write.

#[tokio::test]
async fn test_channels_api() {
    // This assumes we moved `create_app` to a library
    let channels = vec![
        Channel { name: "Test1".to_string(), url: "rtsp://1".to_string() },
        Channel { name: "Test2".to_string(), url: "rtsp://2".to_string() },
    ];
    
    let app = fritztv::create_app(
        channels,
        fritztv::transcoder::TuningMode::LowLatency,
        "udp".to_string(),
        4,
    );

    let response = app
        .oneshot(Request::builder().uri("/api/channels").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = http_body_util::BodyExt::collect(response.into_body()).await.unwrap().to_bytes();
    let channels: Vec<Channel> = serde_json::from_slice(&body).unwrap();

    assert_eq!(channels.len(), 2);
    assert_eq!(channels[0].name, "Test1");
    assert_eq!(channels[1].name, "Test2");
}
