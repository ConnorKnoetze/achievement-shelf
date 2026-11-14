import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";

const page = (name: string) => fileURLToPath(new URL(name, import.meta.url));

export default defineConfig({
  clearScreen: false,
  server: { port: 1420, strictPort: true, watch: { ignored: ["**/src-tauri/**"] } },
  // The shelf, trophy picker, and settings are separate windows, each with its own page.
  build: {
    rollupOptions: {
      input: { main: page("index.html"), picker: page("picker.html"), settings: page("settings.html") },
    },
  },
});
