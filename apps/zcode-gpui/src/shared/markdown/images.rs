//! Local image files embedded in Markdown (`![alt](path)`), shown as
//! thumbnails under assistant replies. Only absolute paths and `file://` URLs
//! are taken; remote URLs are never fetched on the model's behalf.
//!
//! Spec: PARITY.md "Images".

use pulldown_cmark::{Event, Options, Parser, Tag};
use std::borrow::Cow;
use std::path::PathBuf;

/// Distinct local image files referenced by Markdown image syntax, in order.
pub(crate) fn image_targets(text: &str) -> Vec<PathBuf> {
    let text = windows_image_paths_to_slashes(text);
    let mut out: Vec<PathBuf> = Vec::new();
    for event in Parser::new_ext(&text, Options::all()) {
        if let Event::Start(Tag::Image { dest_url, .. }) = event
            && let Some(path) = local_path(&dest_url)
            && path.is_absolute()
            && crate::files::image_preview::format(&path).is_some()
            && !out.contains(&path)
        {
            out.push(path);
        }
    }
    out
}

fn local_path(dest: &str) -> Option<PathBuf> {
    let dest = dest.trim();
    // `C:/x.png` would parse as a URL with scheme `c`, so only `file:` goes
    // through the URL parser.
    if dest.len() > 5 && dest[..5].eq_ignore_ascii_case("file:") {
        return url::Url::parse(dest).ok()?.to_file_path().ok();
    }
    if dest.contains("://") {
        return None;
    }
    Some(PathBuf::from(dest))
}

/// `text` with `\` replaced by `/` inside Windows image destinations
/// (`![x](C:\a\b.png)`, `![x](<\\srv\share\b.png>)`).
///
/// 修复原因：CommonMark 把 `\` 后跟 ASCII 标点视为转义，`C:\Users\V\.cache\x.png`
/// 解析后变成 `C:\Users\V.cache\x.png`（`\.` 丢了反斜杠），缩略图指向不存在的文件，
/// 只剩一个点不开的 `[image: …]` 文字。Windows 路径里 `/` 与 `\` 等价且不会被转义，
/// 所以在解析前把图片目标里的反斜杠换成 `/`。只改以盘符或 `\\` 开头的图片目标，
/// 代码块、行内代码、普通文字和链接保持原样。
pub(crate) fn windows_image_paths_to_slashes(text: &str) -> Cow<'_, str> {
    if !text.contains("![") || !text.contains('\\') {
        return Cow::Borrowed(text);
    }
    let mut slashes: Vec<usize> = Vec::new();
    for (event, range) in Parser::new_ext(text, Options::all()).into_offset_iter() {
        let Event::Start(Tag::Image { .. }) = event else {
            continue;
        };
        let source = &text[range.clone()];
        // Inline images only (`![alt](dest)`); reference images keep their text.
        let Some(open) = source.ends_with(')').then(|| source.rfind("](")).flatten() else {
            continue;
        };
        let dest_start = range.start + open + 2;
        let dest = &text[dest_start..range.end - 1];
        let lead = dest.len() - dest.trim_start().len();
        let (path_start, angle) = match dest[lead..].strip_prefix('<') {
            Some(_) => (dest_start + lead + 1, true),
            None => (dest_start + lead, false),
        };
        let path = &text[path_start..range.end - 1];
        if !is_windows_absolute(path) {
            continue;
        }
        for (i, b) in path.bytes().enumerate() {
            if (angle && b == b'>') || (!angle && b.is_ascii_whitespace()) {
                break;
            }
            if b == b'\\' {
                slashes.push(path_start + i);
            }
        }
    }
    if slashes.is_empty() {
        return Cow::Borrowed(text);
    }
    let mut bytes = text.as_bytes().to_vec();
    for i in slashes {
        bytes[i] = b'/';
    }
    // Only ASCII `\` bytes were swapped for ASCII `/`, so this stays UTF-8.
    Cow::Owned(String::from_utf8(bytes).unwrap_or_else(|_| text.to_string()))
}

fn is_windows_absolute(path: &str) -> bool {
    let b = path.as_bytes();
    path.starts_with("\\\\")
        || (b.len() > 2
            && b[0].is_ascii_alphabetic()
            && b[1] == b':'
            && matches!(b[2], b'\\' | b'/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn windows_paths_and_file_urls() {
        let md = "a ![shot](C:/tmp/shot.png) b ![x](file:///C:/My%20Pics/x.JPG) \
                  ![again](C:/tmp/shot.png)";
        assert_eq!(
            image_targets(md),
            vec![
                PathBuf::from("C:/tmp/shot.png"),
                PathBuf::from(r"C:\My Pics\x.JPG")
            ]
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn unix_paths_and_file_urls() {
        let md = "![a](/tmp/a.png) ![b](file:///tmp/My%20Pics/b.webp)";
        assert_eq!(
            image_targets(md),
            vec![
                PathBuf::from("/tmp/a.png"),
                PathBuf::from("/tmp/My Pics/b.webp")
            ]
        );
    }

    #[test]
    fn backslash_image_paths_survive_markdown_escapes() {
        // `\.cache` used to parse as `.cache`, losing the separator.
        let md = r"![dropped](C:\Users\V\.cache\cc-tmp\213-dropped.png) and ![u](<\\srv\my share\a.png>)";
        assert_eq!(
            windows_image_paths_to_slashes(md),
            "![dropped](C:/Users/V/.cache/cc-tmp/213-dropped.png) and ![u](<//srv/my share/a.png>)"
        );
    }

    #[cfg(windows)]
    #[test]
    fn backslash_image_path_is_a_thumbnail_target() {
        let md = r#"![d](C:\Users\V\.cache\x.png "title")"#;
        assert_eq!(
            image_targets(md),
            vec![PathBuf::from("C:/Users/V/.cache/x.png")]
        );
    }

    #[test]
    fn leaves_code_links_text_and_relative_images_alone() {
        let md = "`![c](C:\\a\\.b.png)` [l](C:\\a\\.b.png) C:\\a\\.b ![r](a\\.b.png)\n\n\
                  ```\n![c](C:\\a\\.b.png)\n```\n";
        assert!(matches!(
            windows_image_paths_to_slashes(md),
            Cow::Borrowed(_)
        ));
    }

    #[test]
    fn skips_relative_remote_non_image_links_and_code() {
        let abs = if cfg!(windows) { "C:/tmp" } else { "/tmp" };
        let md = format!(
            "![r](shot.png) ![w](https://example.com/a.png) ![d]({abs}/notes.md) \
             [link]({abs}/link.png)\n\n```\n![c]({abs}/code.png)\n```\n\n`![i]({abs}/i.png)`"
        );
        assert!(image_targets(&md).is_empty());
    }
}
