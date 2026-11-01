import { defineConfig } from "vite";
import solid from "vite-plugin-solid";

export default defineConfig({
    plugins: [solid()],
    server: {
        port: 5173,
        fs: { allow: [".."] },
    },
    build: {
        target: "es2022",
    },
});
