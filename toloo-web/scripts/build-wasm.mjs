import { execFileSync } from "node:child_process";
import { cpSync, mkdirSync, rmSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const webDir = path.dirname(scriptDir);
const repoDir = path.dirname(webDir);
const generated = path.join(webDir, "src", "generated", "toloo-wasm");

execFileSync("sh", ["toloo-wasm/build.sh"], {
    cwd: repoDir,
    stdio: "inherit",
});

rmSync(generated, { recursive: true, force: true });
mkdirSync(generated, { recursive: true });
cpSync(path.join(repoDir, "toloo-wasm", "pkg"), generated, { recursive: true });
console.log(`copied wasm package to ${path.relative(webDir, generated)}`);
