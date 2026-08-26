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
          if (normalized.includes("/src/context/") || /\/src\/components\/(Common|Settings|Layout|Terminal|RightSidebar|JournaldViewer|Tools|FileManager|SendBar)\//.test(normalized)) {
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
