import { render } from "solid-js/web";
import App from "./App";
import { initWasm } from "./lib/wasm";
import "./styles.css";

const root = document.getElementById("root");
if (!root) throw new Error("missing #root");

try {
    await initWasm();
    render(() => <App />, root);
} catch (error) {
    root.innerHTML = "";
    const message = document.createElement("pre");
    message.className = "boot-failure";
    message.textContent = `Toloo failed to start.\n\n${
        error instanceof Error ? error.message : String(error)
    }`;
    root.appendChild(message);
}
