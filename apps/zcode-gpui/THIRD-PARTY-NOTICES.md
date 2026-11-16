# Third-party notices for zcode-gpui

This file covers the Rust dependencies of the native GPUI frontend
(`apps/zcode-gpui`). It complements the repository-wide
`THIRD-PARTY-NOTICES.md`, which covers the Node/Electron side.

Dependencies are used through their public APIs; no source code is copied
from the Zed application repository (which is GPL-3.0 — its app sources are
intentionally **not** a dependency or a code source for this frontend). `gpui`
is consumed as the Apache-2.0-published crate from the Zed repository, pinned
to the revision that `ely-gpui-component` targets so both build one shared
gpui; the `gpui` **crate** is Apache-2.0, unlike the GPL-3.0 Zed app sources.

| Crate | Version | License | Upstream |
| --- | --- | --- | --- |
| gpui / gpui_platform | git rev `1a28cff` (zed, 2026-09-27) | Apache-2.0 | https://github.com/zed-industries/zed |
| ely-gpui-component | rev `2f8b2f6`, vendored in `vendor/ely-gpui-component` | MIT OR Apache-2.0 | https://github.com/ZacharyZhang-NY/Ely-GPUI-Components (no longer reachable) |
| syntect | 5.3.0 | MIT | https://github.com/trishume/syntect |
| pulldown-cmark | 0.13.4 | MIT | https://github.com/pulldown-cmark/pulldown-cmark |
| portable-pty | 0.9.0 | MIT | https://github.com/wezterm/wezterm |
| alacritty_terminal | 0.26.0 | Apache-2.0 | https://github.com/alacritty/alacritty |
| windows | 0.62 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| serde / serde_json | 1.x | MIT OR Apache-2.0 | https://github.com/serde-rs |
| uuid | 1.x | Apache-2.0 OR MIT | https://github.com/uuid-rs/uuid |
| futures | 0.3 | MIT OR Apache-2.0 | https://github.com/rust-lang/futures-rs |
| base64 | 0.22 | MIT OR Apache-2.0 | https://github.com/marshallpierce/rust-base64 |
| crc32fast | 1.x | MIT OR Apache-2.0 | https://github.com/srijs/rust-crc32fast |
| ignore | 0.4 | Unlicense OR MIT | https://github.com/BurntSushi/ripgrep |
| notify | 8.x | CC0-1.0 | https://github.com/notify-rs/notify |

## ely-gpui-component (MIT OR Apache-2.0)

Vendored unchanged in `vendor/ely-gpui-component` (see its `VENDORED.md`) and used
through its public API; the component library embeds
assets that are redistributed with the binary via `ely_gpui_component::Assets`:
font families Inter, JetBrains Mono, IBM Plex Sans and Noto Sans Hebrew
(licensed under the SIL Open Font License 1.1) and Lucide icons (ISC). Their
license texts ship under `vendor/ely-gpui-component/assets/`, next to the crate's
`LICENSE-MIT` and `LICENSE-APACHE`.

## Apache-2.0 notice (gpui, alacritty_terminal)
Licensed under the Apache License, Version 2.0 (the "License"); you may not
use these files except in compliance with the License. You may obtain a copy
of the License at http://www.apache.org/licenses/LICENSE-2.0. The full text
ships with each crate (LICENSE-APACHE) and with the Rust dependency sources
in the local Cargo registry.

## MIT notice (syntect, pulldown-cmark, portable-pty, and dual-licensed crates)

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, subject to the following conditions: the above
copyright notice and this permission notice shall be included in all copies
or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

Copyright holders are listed in each crate's `Cargo.toml` / LICENSE file in
the Cargo registry (e.g. Zed Industries, Inc. for gpui; trishume for syntect;
Raphael Cohn and contributors for pulldown-cmark; Wez Furlong for
portable-pty; the Alacritty Project contributors for alacritty_terminal).

## Browser extension (`apps/zcode-browser-extension`)

The unpacked extension shipped as `browser-extension/` bundles the injected
script of `playwright-core` 1.59.1 (Apache-2.0, Copyright Microsoft
Corporation). Its NOTICE and LICENSE texts are written into
`browser-extension/NOTICE.md` by the extension build. The extension and
`zcode-browser-host` follow the general design of OpenAI's Codex browser
extension and native host; that code is proprietary and none of it is copied.

## Voice sidecar (zcode-voice, GPL-3.0-or-later)

Agent voice (`VOICE.md`) runs in the separate program `zcode-voice`, built from
`apps/zcode-voice`. It statically links eSpeak NG (GPL-3.0-or-later) and
uses kokoro-tiny, ONNX Runtime and DirectML. zcode-gpui does not link it; it
only starts the process and exchanges newline-delimited JSON over stdin/stdout.
Its license and dependency list ship as `zcode-voice-NOTICE.md`
(`apps/zcode-voice/NOTICE.md`).

## Code search backend (fff, MIT)

The compiled backend (`scripts/build_backend.ts`) embeds `@ff-labs/fff-bun`
0.11.0 and its platform library (`@ff-labs/fff-bin-*`, `fff_c`), from
https://github.com/dmtrKovalenko/fff (MIT, Copyright Dmitry Kovalenko). It
powers the GPUI-only `FindFiles` and `SearchCode` agent tools; the MIT notice
above applies.

## Codemode sandbox (pi-codemode, quickjs-wasi, QuickJS, MIT)

The compiled backend embeds `@earendil-works/pi-codemode` 1.5.2, from
https://github.com/earendil-works/pi (MIT, Copyright Earendil Works), which
runs the `codemode` agent tool's scripts. Its QuickJS runtime is
`quickjs-wasi` 3.6.2, from https://github.com/vercel-labs/quickjs-wasi (MIT,
Copyright Vercel, Inc.); `quickjs.wasm` ships beside the backend in
`node_modules/quickjs-wasi/` with that package's `LICENSE`. The wasm build
contains QuickJS-ng (https://github.com/quickjs-ng/quickjs, MIT). The codemode
tool description is adapted from pi's `packages/coding-agent/docs/codemode.md`.
The MIT notice above applies.

## Transitive dependencies

Transitive crate dependencies (e.g. vte, unicode-width, parking_lot, regex)
inherit the notices of their own crates, all published under MIT and/or
Apache-2.0; run `cargo license` inside `apps/zcode-gpui` for the complete
generated list.
