import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { copyFileSync, existsSync, mkdirSync, readFileSync } from "node:fs";
import { resolve, dirname } from "node:path";

const LOGO_SRC = resolve(__dirname, "src/assets/icons/logo.png");

// 将 logo.png 复制到 dist/ 作为 favicon.png（构建时）
function copyFavicon(outDir: string) {
  const dest = resolve(outDir, "favicon.png");
  if (existsSync(LOGO_SRC)) {
    if (!existsSync(dirname(dest))) mkdirSync(dirname(dest), { recursive: true });
    copyFileSync(LOGO_SRC, dest);
  }
}

// https://vitejs.dev/config/
export default defineConfig(async () => ({
  plugins: [
    react(),
    {
      name: "favicon-from-logo",
      // 开发服务器：将 /favicon.png 映射到 src/assets/icons/logo.png
      configureServer(server) {
        server.middlewares.use("/favicon.png", (_req, res) => {
          if (existsSync(LOGO_SRC)) {
            res.setHeader("Content-Type", "image/png");
            res.end(readFileSync(LOGO_SRC));
          } else {
            res.statusCode = 404;
            res.end();
          }
        });
      },
      // 构建：复制 logo.png → dist/favicon.png
      closeBundle() {
        // vite 默认输出到 dist/
        copyFavicon(resolve(__dirname, "dist"));
      },
    },
    {
      // 开发模式预加载 logo.png，使其与 JS bundle 并行下载
      // 避免首屏 logo 图标因网络请求延迟而晚于其他 UI 元素出现
      name: "preload-logo",
      apply: "serve",
      transformIndexHtml() {
        return [
          {
            tag: "link",
            attrs: {
              rel: "preload",
              as: "image",
              href: "/src/assets/icons/logo.png",
            },
            injectTo: "head" as const,
          },
        ];
      },
    },
  ],
  // Prevent vite from obscuring Rust errors
  clearScreen: false,
  build: {
    rollupOptions: {
      output: {
        // Split dependency families while keeping the strongly-connected application UI
        // in one chunk. This avoids circular manual chunks and stays below Vite's 500KB limit.
        manualChunks(id) {
          const normalized = id.replace(/\\/g, "/");
          // Settings is a low-frequency, one-way UI surface: it consumes shared context/common
          // components, while the core layout does not import Settings back. Keep it separate so
          // daily-driver UI stays under Vite's 500KB warning budget without hiding the warning.
          if (/\/src\/components\/Settings\//.test(normalized)) {
            return "ui-settings";
          }
          // Protocol/data engineering tools are lazy-loaded from SessionRightSidebar and are
          // intentionally isolated from the daily-driver UI chunk. Keep their pure parsers and
          // tool-specific hook with the same chunk so opening the sidebar pays one coherent load.
          // FileManager is an SSH-side feature and is lazy-loaded from SessionRightSidebar.
          // Keep its main implementation out of ui-core so non-SSH/daily-driver startup does
          // not pay for SFTP browsing code. Dialogs remain their own on-demand chunks.
          if (
            /\/src\/components\/FileManager\/(FilePropertiesModal|FilePreviewModal|ConflictResolutionModal|DeleteConfirmationDialog)\.tsx$/.test(normalized)
          ) {
            return undefined;
          }
          if (/\/src\/components\/FileManager\//.test(normalized)) {
            return "ui-file-manager";
          }
          if (
            /\/src\/components\/Tools\//.test(normalized)
            || /\/src\/protocols\//.test(normalized)
            || /\/src\/hooks\/useToolInputHistory\.ts$/.test(normalized)
            || /\/src\/utils\/(bitops|byteInput|checksum|dataInspector|encoding|engineering|protocolParsing|toolResult)\.ts$/.test(normalized)
          ) {
            return "ui-engineering";
          }
          if (normalized.includes("/src/context/") || /\/src\/components\/(Common|Layout|Terminal|RightSidebar|JournaldViewer|SendBar)\//.test(normalized)) {
            return "ui-core";
          }
          if (!normalized.includes("node_modules")) return undefined;
          if (normalized.includes("@xterm/")) return "xterm";
          if (normalized.includes("framer-motion") || normalized.includes("motion-dom") || normalized.includes("motion-utils")) return "motion";
          if (normalized.includes("react") || normalized.includes("scheduler")) return "react";
          if (normalized.includes("i18next")) return "i18n";
          if (normalized.includes("@tauri-apps/")) return "tauri";
          return undefined;
        },
      },
    },
  },
  server: {
    port: 5173,
    strictPort: true,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
}));
