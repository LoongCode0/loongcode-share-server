import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// dev 时 /api 代理到本地 share-server（cargo run，默认 8787）。
export default defineConfig({
  plugins: [react()],
  server: { proxy: { "/api": "http://127.0.0.1:8787" } },
});
