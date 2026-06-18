import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  createPythonRuntimeEnv,
  setupPythonRuntime,
} from "./python-runtime.mjs";

const repoRoot = path.resolve(fileURLToPath(new URL("..", import.meta.url)));
const tauriCommand = resolveTauriCommand();
const args = process.argv.slice(2);
const isBuild = args[0] === "build";
const shouldEnablePythonPlugins = args[0] === "dev" || isBuild;

let env = process.env;
let tauriArgs = shouldEnablePythonPlugins ? ensurePythonFeature(args) : args;

if (isBuild) {
  const { target, paths, assetName } = await setupPythonRuntime();

  console.log(`Using Python runtime for ${target.key}`);
  console.log(`Source asset: ${assetName}`);
  console.log(`Runtime: ${paths.runtimeDir}`);

  env = createPythonRuntimeEnv();
}

const child = spawn(tauriCommand, tauriArgs, {
  env,
  shell: process.platform === "win32",
  stdio: "inherit",
});

child.on("exit", (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
    return;
  }

  process.exit(code ?? 1);
});

function ensurePythonFeature(values) {
  const existingFeatureIndex = values.findIndex(
    (value) => value === "--features" || value.startsWith("--features="),
  );

  if (existingFeatureIndex === -1) {
    return [...values, "--features", "python-plugins"];
  }

  const value = values[existingFeatureIndex];

  if (value.startsWith("--features=")) {
    if (value.includes("python-plugins")) {
      return values;
    }

    return values.map((item, index) =>
      index === existingFeatureIndex ? `${item},python-plugins` : item,
    );
  }

  const featureValue = values[existingFeatureIndex + 1];

  if (!featureValue || featureValue.includes("python-plugins")) {
    return values;
  }

  return values.map((item, index) =>
    index === existingFeatureIndex + 1 ? `${item},python-plugins` : item,
  );
}

function resolveTauriCommand() {
  const binDirectory = path.join(repoRoot, "node_modules", ".bin");
  const candidates =
    process.platform === "win32"
      ? ["tauri.exe", "tauri.cmd", "tauri.bunx"]
      : ["tauri"];

  for (const candidate of candidates) {
    const commandPath = path.join(binDirectory, candidate);

    if (existsSync(commandPath)) {
      return commandPath;
    }
  }

  return "tauri";
}
