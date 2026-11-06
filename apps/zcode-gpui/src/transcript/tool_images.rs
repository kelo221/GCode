//! Images in tool results: inline base64 from the row display
//! (`node_repl_images`, `media` of CUA/MCP results; toolDisplay.ts) and image
//! files read by the Read tool. Decoding runs in the background through the
//! Files pane's decoder and is cached by source.

use crate::files::image_preview::{ImagePreview, decode};
use crate::files::preview::{Preview, read_preview};
use crate::shared::theme::{BORDER, MUTED, ui_size};
use crate::shared::theme_colors::color as rgb;
use base64::Engine;
use gpui::{AnyElement, Context, ElementId, ObjectFit, StyledImage, div, img, prelude::*, px};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ImageRef {
    Inline {
        mime: String,
        data: String,
    },
    File(PathBuf),
    /// An image attached to a sent message, read back through the CLI.
    Attachment {
        session: String,
        reference: String,
        mime: String,
    },
}

impl ImageRef {
    fn key(&self) -> String {
        match self {
            Self::Inline { data, .. } => format!(
                "b64:{}:{:08x}",
                data.len(),
                crc32fast::hash(data.as_bytes())
            ),
            Self::File(p) => format!("file:{}", p.display()),
            Self::Attachment {
                session, reference, ..
            } => format!("att:{session}:{reference}"),
        }
    }
}

fn inline_items(list: Option<&Value>, data_key: &str, out: &mut Vec<ImageRef>) {
    for item in list.and_then(Value::as_array).into_iter().flatten() {
        let mime = item.get("mimeType").and_then(Value::as_str).unwrap_or("");
        if let Some(data) = item.get(data_key).and_then(Value::as_str)
            && mime.starts_with("image/")
        {
            out.push(ImageRef::Inline {
                mime: mime.to_string(),
                data: data.to_string(),
            });
        }
    }
}

/// Image sources of a finished tool call.
pub(crate) fn image_refs(
    tool: &str,
    status: &str,
    input: Option<&Value>,
    display: Option<&Value>,
) -> Vec<ImageRef> {
    let mut out = Vec::new();
    if let Some(d) = display {
        inline_items(d.get("images"), "base64", &mut out);
        inline_items(d.get("media"), "data", &mut out);
    }
    if tool == "Read"
        && status == "success"
        && let Some(path) = input
            .and_then(|v| v.get("file_path"))
            .and_then(Value::as_str)
            .map(PathBuf::from)
        && path.is_absolute()
        && crate::files::image_preview::format(&path).is_some()
    {
        out.push(ImageRef::File(path));
    }
    out
}

fn mime_format(mime: &str) -> Option<image::ImageFormat> {
    match mime.to_ascii_lowercase().as_str() {
        "image/png" => Some(image::ImageFormat::Png),
        "image/jpeg" | "image/jpg" => Some(image::ImageFormat::Jpeg),
        "image/gif" => Some(image::ImageFormat::Gif),
        "image/webp" => Some(image::ImageFormat::WebP),
        "image/bmp" => Some(image::ImageFormat::Bmp),
        _ => None,
    }
}

fn load(r: ImageRef) -> Result<ImagePreview, String> {
    let preview = match r {
        ImageRef::Inline { mime, data } => {
            let format =
                mime_format(&mime).ok_or_else(|| format!("Unsupported image type {mime}"))?;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(data.as_bytes())
                .map_err(|_| "Image data is not valid base64".to_string())?;
            decode(bytes, format)
        }
        ImageRef::File(path) => read_preview(&path),
        ImageRef::Attachment { .. } => return Err("Not an image".into()),
    };
    preview_result(preview)
}

/// Decode bytes fetched from the desktop or the CLI (`remote::desktop_images`).
fn decode_fetched(r: &ImageRef, bytes: Vec<u8>) -> Result<ImagePreview, String> {
    let format = match r {
        ImageRef::File(path) => crate::files::image_preview::format(path),
        ImageRef::Attachment { mime, .. } => mime_format(mime),
        ImageRef::Inline { .. } => None,
    }
    .or_else(|| image::guess_format(&bytes).ok())
    .ok_or_else(|| "Unsupported image type".to_string())?;
    preview_result(decode(bytes, format))
}

fn preview_result(preview: Preview) -> Result<ImagePreview, String> {
    match preview {
        Preview::Image(img) => Ok(img),
        Preview::Error(e) => Err(e.label().to_string()),
        Preview::TooLarge(_) => Err("Image is too large to preview".to_string()),
        _ => Err("Not an image".to_string()),
    }
}

/// Box a thumbnail fits inside, keeping the image's aspect ratio. One image
/// gets the large box; several share a row as smaller, equal-height tiles.
pub(crate) fn thumb_box(count: usize) -> (f32, f32) {
    if count > 1 {
        (240., 160.)
    } else {
        (320., 240.)
    }
}

#[derive(Default)]
pub(crate) struct ToolImageCache {
    images: HashMap<String, Option<Result<ImagePreview, String>>>,
    /// The image open in the full-window viewer (`image_viewer.rs`).
    pub(crate) viewer: Option<crate::transcript::image_viewer::Viewer>,
}

impl ToolImageCache {
    #[cfg(test)]
    pub(crate) fn insert_loaded(&mut self, key: &str, preview: ImagePreview) {
        self.images.insert(key.to_string(), Some(Ok(preview)));
    }

    pub(crate) fn loaded(&self, key: &str) -> Option<&ImagePreview> {
        match self.images.get(key) {
            Some(Some(Ok(preview))) => Some(preview),
            _ => None,
        }
    }
}

/// `(width, height)` scaled to fit `max_w × max_h` without enlarging.
pub(crate) fn fit(width: u32, height: u32, max_w: f32, max_h: f32) -> (f32, f32) {
    let (w, h) = (width.max(1) as f32, height.max(1) as f32);
    let scale = (max_w / w).min(max_h / h).min(1.);
    (w * scale, h * scale)
}

impl crate::app::root::RootView {
    fn start_tool_image(&mut self, r: ImageRef, cx: &mut Context<Self>) {
        let key = r.key();
        if self.tool_images.images.contains_key(&key) {
            return;
        }
        self.tool_images.images.insert(key.clone(), None);
        let remote = remote_source(&r, &self.state, cx);
        let state = self.state.downgrade();
        cx.spawn(async move |this, cx| {
            use crate::remote::desktop_images::fetch;
            let result = match remote {
                None => cx.background_spawn(async move { load(r) }).await,
                Some(Err(e)) => Err(e),
                Some(Ok((ws, source, cache))) => match fetch(state, ws, source, cache, cx).await {
                    Ok(bytes) => {
                        cx.background_spawn(async move { decode_fetched(&r, bytes) })
                            .await
                    }
                    Err(e) => Err(e),
                },
            };
            this.update(cx, |v, cx| {
                v.tool_images.images.insert(key, Some(result));
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// `row_id` keeps element ids unique when one file is read many times.
    pub(crate) fn tool_images_view(
        &self,
        row_id: u64,
        refs: Vec<ImageRef>,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        self.images_row(row_id, refs, false, cx)
    }

    /// `end` right-aligns the thumbnails (under a user message bubble).
    pub(crate) fn images_row(
        &self,
        row_id: u64,
        refs: Vec<ImageRef>,
        end: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if refs.is_empty() {
            return None;
        }
        let mut row = div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_wrap()
            .gap_2()
            .when(end, |el| el.justify_end());
        let (max_w, max_h) = thumb_box(refs.len());
        for r in refs {
            let key = r.key();
            match self.tool_images.images.get(&key) {
                None => {
                    // Render cannot mutate; claim and start the load on the next turn.
                    let r = r.clone();
                    cx.spawn(async move |this, cx| {
                        this.update(cx, |v, cx| v.start_tool_image(r, cx)).ok();
                    })
                    .detach();
                    row = row.child(note("Loading image…"));
                }
                Some(None) => row = row.child(note("Loading image…")),
                Some(Some(Err(e))) => row = row.child(note(e)),
                Some(Some(Ok(preview))) => {
                    let id = ElementId::Name(format!("tool-img-{row_id}-{key}").into());
                    let (w, h) = fit(preview.width, preview.height, max_w, max_h);
                    row = row.child(
                        div()
                            .id(id)
                            .cursor_pointer()
                            .rounded_md()
                            .border_1()
                            .border_color(rgb(BORDER))
                            .overflow_hidden()
                            .child(
                                img(gpui::ImageSource::Render(preview.image.clone()))
                                    .w(px(w))
                                    .h(px(h))
                                    .object_fit(ObjectFit::Contain),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.open_image_viewer(key.clone(), cx);
                            })),
                    );
                }
            }
        }
        Some(row.into_any_element())
    }
}

type RemoteSource = (
    String,
    crate::remote::desktop_images::RemoteImage,
    Option<PathBuf>,
);

/// Where an image's bytes come from when this process cannot read them
/// itself: `None` loads locally; the phone asks the desktop for files, and
/// both ask their CLI for sent attachments.
fn remote_source(
    r: &ImageRef,
    state: &gpui::Entity<crate::app::store::AppState>,
    cx: &gpui::App,
) -> Option<Result<RemoteSource, String>> {
    use crate::remote::desktop_images::{RemoteImage, cache_dir};
    let state = state.read(cx);
    let phone = state.phone_client.enabled;
    let source = match r {
        ImageRef::File(path) if phone => RemoteImage::DesktopFile(path.clone()),
        ImageRef::Attachment {
            session, reference, ..
        } => RemoteImage::Attachment {
            session: session.clone(),
            reference: reference.clone(),
        },
        _ => return None,
    };
    Some(
        state
            .active_ws_key()
            .map(|ws| (ws, source, phone.then(cache_dir)))
            .ok_or_else(|| "Not connected".to_string()),
    )
}

fn note(text: &str) -> gpui::Div {
    div()
        .text_size(px(ui_size(12.)))
        .text_color(rgb(MUTED))
        .child(text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn display_images_and_media_are_collected() {
        let d = json!({ "kind": "node_repl_images",
            "images": [ { "base64": "AAAA", "mimeType": "image/png" } ],
            "media": [ { "mimeType": "image/jpeg", "data": "BBBB" },
                       { "mimeType": "audio/wav", "data": "CCCC" } ] });
        let refs = image_refs("NodeRepl", "success", None, Some(&d));
        assert_eq!(refs.len(), 2);
        assert_eq!(
            refs[1],
            ImageRef::Inline {
                mime: "image/jpeg".into(),
                data: "BBBB".into()
            }
        );
    }

    #[test]
    fn read_of_absolute_image_path_only_when_successful() {
        let path = if cfg!(windows) {
            "C:/tmp/shot.png"
        } else {
            "/tmp/shot.png"
        };
        let input = json!({ "file_path": path });
        assert_eq!(image_refs("Read", "success", Some(&input), None).len(), 1);
        assert!(image_refs("Read", "running", Some(&input), None).is_empty());
        let text = json!({ "file_path": "/tmp/notes.md" });
        assert!(image_refs("Read", "success", Some(&text), None).is_empty());
        let rel = json!({ "file_path": "shot.png" });
        assert!(image_refs("Read", "success", Some(&rel), None).is_empty());
    }

    #[test]
    fn thumbnails_keep_the_aspect_ratio_and_never_enlarge() {
        assert_eq!(fit(1080, 2400, 320., 240.), (108., 240.));
        assert_eq!(fit(1920, 1080, 320., 240.), (320., 180.));
        assert_eq!(fit(100, 50, 320., 240.), (100., 50.));
    }

    #[test]
    fn several_images_get_smaller_tiles_that_fit_two_per_bubble_row() {
        assert_eq!(thumb_box(1), (320., 240.));
        let (w, h) = thumb_box(2);
        assert_eq!((w, h), (240., 160.));
        // Two widest tiles plus the 8 px gap fit the 576 px user column.
        assert!(2. * w + 8. <= 576.);
    }

    #[test]
    fn invalid_inline_data_reports_an_error() {
        let r = ImageRef::Inline {
            mime: "image/png".into(),
            data: "!!".into(),
        };
        assert!(load(r).is_err());
        let r = ImageRef::Inline {
            mime: "image/tiff".into(),
            data: "AAAA".into(),
        };
        assert!(load(r).unwrap_err().contains("Unsupported"));
    }
}
