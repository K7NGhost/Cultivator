import { appendFileSync } from "node:fs";
import { createPythonRuntimeEnv, setupPythonRuntime } from "../python-runtime.mjs";

if (!process.env.GITHUB_ENV || !process.env.GITHUB_PATH) {
  throw new Error("This script requires the GitHub Actions environment files.");
}

const { target, paths } = await setupPythonRuntime();
const runtimeEnv = createPythonRuntimeEnv();
const keys = [
  "PYO3_PYTHON",
  "PYTHONHOME",
  "PYTHONDONTWRITEBYTECODE",
  "LD_LIBRARY_PATH",
  "DYLD_LIBRARY_PATH",
];

for (const key of keys) {
  const value = runtimeEnv[key];
  if (value === undefined) continue;
  if (/[\r\n]/.test(value)) {
    throw new Error(`Unexpected newline in Python runtime variable ${key}.`);
  }
  appendFileSync(process.env.GITHUB_ENV, `${key}=${value}\n`);
}

for (const directory of new Set([paths.runtimeDir, ...paths.libraryDirs])) {
  if (/[\r\n]/.test(directory)) {
    throw new Error("Unexpected newline in Python runtime path.");
  }
  appendFileSync(process.env.GITHUB_PATH, `${directory}\n`);
}

console.log(`Configured bundled Python for ${target.key}.`);
