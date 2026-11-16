// codemode 的 worker 线程入口（docs/design/v2/tool/21-codemode.md）。
// 编译后的 backend exe 无法按包内相对 URL 启动 pi-codemode 自带的 worker.js，所以
// apps/zcode-gpui/scripts/build_backend.ts 把本文件作为第二个 entrypoint 嵌入，运行时以
// "./codemode-worker.js" 启动（core/src/tool/codemode/sandbox.ts）。本文件必须与 main.ts 同目录。
import "@earendil-works/pi-codemode/worker";
