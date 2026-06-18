import { setupPythonRuntime } from "./python-runtime.mjs";

const { target, paths, assetName } = await setupPythonRuntime();

console.log(`Installed Python runtime for ${target.key}`);
console.log(`Source asset: ${assetName}`);
console.log(`Runtime: ${paths.runtimeDir}`);
console.log(`PYO3_PYTHON: ${paths.pythonExecutable}`);
