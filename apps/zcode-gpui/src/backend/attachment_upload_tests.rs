use crate::app::store::AppState;
use crate::backend::attachment_upload::{PreparedImage, UploadState};
use crate::composer::attachment::{
    ATTACHMENT_UPLOAD_CHUNK_BYTES, AttachmentRef, calculate_sha256, chunk_payload,
};
use gpui::{AppContext, ImageFormat, TestAppContext};
use serde_json::{Value, json};
use std::sync::mpsc::{Receiver, channel};

fn setup(cx: &mut TestAppContext) -> (gpui::Entity<AppState>, String, Receiver<String>) {
    let state = cx.new(AppState::for_test);
    let (tx, rx) = channel();
    let key = state.update(cx, |s, _| {
        let key = s.workspaces[0].key.clone();
        s.workspaces[0].inbound = Some(tx);
        s.workspaces[0].started = true;
        s.active_workspace = Some(key.clone());
        s.active = Some("parent".into());
        s.conversations
            .entry("parent".into())
            .or_default()
            .subscribed = true;
        s.draft = false;
        key
    });
    (state, key, rx)
}

fn next(rx: &Receiver<String>) -> Value {
    serde_json::from_str(&rx.try_recv().expect("upload request")).unwrap()
}

fn prepare(
    state: &gpui::Entity<AppState>,
    key: &str,
    bytes: &[u8],
    cx: &mut TestAppContext,
) -> String {
    state.update(cx, |s, cx| {
        let replacement = s.composer.read(cx).replacement_generation;
        s.start_image_upload(bytes.to_vec(), ImageFormat::Jpeg, replacement, cx);
        let id = s
            .ws(key)
            .unwrap()
            .image_uploads
            .keys()
            .next()
            .unwrap()
            .clone();
        s.finish_image_preparation(
            key,
            &id,
            Ok(PreparedImage {
                chunks: chunk_payload(bytes).unwrap(),
                checksum: calculate_sha256(bytes),
            }),
            cx,
        );
        id
    })
}

#[gpui::test]
fn image_upload_orders_bounded_chunks_and_commits_only_the_ready_ref(cx: &mut TestAppContext) {
    let (state, key, rx) = setup(cx);
    let bytes = vec![7; ATTACHMENT_UPLOAD_CHUNK_BYTES + 1];
    let upload = prepare(&state, &key, &bytes, cx);
    let begin = next(&rx);
    assert_eq!(begin["method"], "v4/attachment/begin");
    assert_eq!(begin["params"]["sessionId"], "parent");
    assert_eq!(begin["params"]["totalBytes"], bytes.len());
    assert_eq!(begin["params"]["totalChunks"], 2);
    assert_eq!(begin["params"]["checksum"], calculate_sha256(&bytes));
    assert_eq!(begin["params"]["mime"], "image/jpeg");
    assert_eq!(begin["params"]["fileName"], "clipboard.jpg");
    state.update(cx, |s, cx| {
        s.composer.update(cx, |c, _| c.content = "draft".into());
        s.submit_composer(cx);
        assert_eq!(s.composer.read(cx).text(), "draft");
        assert!(s.composer.read(cx).attachments.is_empty());
        s.handle_response(
            &key,
            begin["id"].as_u64().unwrap(),
            Some(json!({"uploadId":upload,"state":"staging","nextChunkIndex":0})),
            None,
            cx,
        );
    });
    for index in 0..2 {
        let chunk = next(&rx);
        assert_eq!(chunk["method"], "v4/attachment/chunk");
        assert_eq!(chunk["params"]["chunkIndex"], index);
        assert!(chunk.to_string().len() < 1024 * 1024);
        state.update(cx, |s, cx| {
            s.handle_response(
                &key,
                chunk["id"].as_u64().unwrap(),
                Some(json!({"uploadId":upload,"nextChunkIndex":index + 1})),
                None,
                cx,
            )
        });
    }
    let commit = next(&rx);
    assert_eq!(commit["method"], "v4/attachment/commit");
    state.update(cx, |s, cx| {
        s.handle_response(
            &key,
            commit["id"].as_u64().unwrap(),
            Some(json!({"ref":"zcode-artifact://parent/image"})),
            None,
            cx,
        );
        assert!(s.ws(&key).unwrap().image_uploads.is_empty());
        assert!(s.composer.read(cx).temp_owned.is_empty());
        assert_eq!(
            s.composer.read(cx).attachments[0].reference,
            "zcode-artifact://parent/image"
        );
        s.submit_composer(cx);
    });
    let send = next(&rx);
    assert_eq!(send["params"]["type"], "sendText");
    assert_eq!(
        send["params"]["payload"]["attachments"][0]["ref"],
        "zcode-artifact://parent/image"
    );
}

#[gpui::test]
fn image_upload_errors_are_generic_retry_is_explicit_and_cancel_retires_pending(
    cx: &mut TestAppContext,
) {
    let (state, key, rx) = setup(cx);
    let upload = prepare(&state, &key, b"source", cx);
    let begin = next(&rx);
    state.update(cx, |s, cx| {
        s.handle_response(
            &key,
            begin["id"].as_u64().unwrap(),
            Some(json!({"uploadId":upload,"state":"staging","nextChunkIndex":0})),
            None,
            cx,
        );
    });
    let chunk = next(&rx);
    state.update(cx, |s, cx| {
        s.handle_response(
            &key,
            chunk["id"].as_u64().unwrap(),
            None,
            Some(json!({"message":"secret-image-error-sentinel"})),
            cx,
        );
        assert!(matches!(
            s.ws(&key).unwrap().image_uploads[&upload].state,
            UploadState::Failed
        ));
        assert!(!s.ws(&key).unwrap().status.contains("sentinel"));
        assert!(s.errors.iter().all(|error| !error.contains("sentinel")));
        assert_eq!(
            &*s.ws(&key).unwrap().image_uploads[&upload].bytes,
            b"source"
        );
        s.retry_image_upload(&key, &upload, cx);
        let new_id = s
            .ws(&key)
            .unwrap()
            .image_uploads
            .keys()
            .next()
            .unwrap()
            .clone();
        assert_ne!(new_id, upload);
        s.cancel_image_upload(&key, &new_id, cx);
        assert!(s.ws(&key).unwrap().image_uploads.is_empty());
    });
    assert_eq!(next(&rx)["method"], "v4/attachment/abort");
    assert_eq!(next(&rx)["method"], "v4/attachment/abort");
    assert!(rx.try_recv().is_err());
}

#[gpui::test]
fn image_upload_rejects_wrong_progress_and_stale_owner_completion(cx: &mut TestAppContext) {
    let (state, key, rx) = setup(cx);
    let upload = prepare(&state, &key, b"source", cx);
    let begin = next(&rx);
    state.update(cx, |s, cx| {
        s.handle_response(
            &key,
            begin["id"].as_u64().unwrap(),
            Some(json!({"uploadId":upload,"state":"staging","nextChunkIndex":1})),
            None,
            cx,
        );
        assert!(matches!(
            s.ws(&key).unwrap().image_uploads[&upload].state,
            UploadState::Failed
        ));
        s.cancel_image_upload(&key, &upload, cx);
    });
    while rx.try_recv().is_ok() {}
    let upload = prepare(&state, &key, b"new source", cx);
    let begin = next(&rx);
    state.update(cx, |s, cx| {
        s.active = Some("other-parent".into());
        s.composer.update(cx, |c, _| c.set_text("new owner"));
        s.handle_response(
            &key,
            begin["id"].as_u64().unwrap(),
            Some(json!({"uploadId":upload,"state":"staging","nextChunkIndex":0})),
            None,
            cx,
        );
        assert!(s.ws(&key).unwrap().image_uploads.is_empty());
        assert_eq!(s.composer.read(cx).text(), "new owner");
        assert!(s.composer.read(cx).attachments.is_empty());
    });
    assert_eq!(next(&rx)["method"], "v4/attachment/abort");
    assert!(rx.try_recv().is_err());
}

#[gpui::test]
fn image_upload_reconnect_and_failed_enqueue_do_not_block_new_input(cx: &mut TestAppContext) {
    let (state, key, rx) = setup(cx);
    let upload = prepare(&state, &key, b"source", cx);
    let begin = next(&rx);
    state.update(cx, |s, cx| {
        s.ws_mut(&key).unwrap().invalidate_connection();
        s.handle_response(
            &key,
            begin["id"].as_u64().unwrap(),
            Some(json!({"uploadId":upload,"state":"staging","nextChunkIndex":0})),
            None,
            cx,
        );
        assert!(s.ws(&key).unwrap().image_uploads.is_empty());
        assert!(s.ws(&key).unwrap().pending.is_empty());
        assert!(s.composer.read(cx).attachments.is_empty());
    });
    drop(rx);
    let upload = prepare(&state, &key, b"retryable", cx);
    state.update(cx, |s, cx| {
        assert!(matches!(
            s.ws(&key).unwrap().image_uploads[&upload].state,
            UploadState::Failed
        ));
        assert!(s.ws(&key).unwrap().pending.is_empty());
        s.cancel_image_upload(&key, &upload, cx);
        assert!(!s.uploads_block_submission(cx));
        s.composer.update(cx, |c, cx| {
            c.add_attachment(
                AttachmentRef {
                    reference: "picked-local-path".into(),
                    file_name: "picked.png".into(),
                    mime: "image/png".into(),
                    bytes: 1,
                    preview_ref: None,
                },
                cx,
            )
        });
        assert_eq!(
            s.composer.read(cx).attachments[0].reference,
            "picked-local-path"
        );
    });
}
