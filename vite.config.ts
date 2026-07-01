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
    // xterm.js + framer-motion 总体积超过默认 500KB 限制，提高阈值
    chunkSizeWarningLimit: 800,
  },
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
}));
