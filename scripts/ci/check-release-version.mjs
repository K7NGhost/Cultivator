import { readFileSync } from "node:fs";

const tag = process.env.GITHUB_REF_NAME ?? process.argv[2];
if (!/^v\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(tag ?? "")) {
  throw new Error("Release tags must use vMAJOR.MINOR.PATCH[-prerelease].");
}

const version = tag.slice(1);
const packageVersion = JSON.parse(readFileSync("package.json", "utf8")).version;
const tauriVersion = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8")).version;
const manifest = readFileSync("src-tauri/Cargo.toml", "utf8");
const packageSection = manifest.match(/\[package\]([\s\S]*?)(?=\n\[|$)/)?.[1];
const cargoVersion = packageSection?.match(/^version\s*=\s*"([^"]+)"/m)?.[1];

for (const [file, declared] of [
  ["package.json", packageVersion],
  ["src-tauri/tauri.conf.json", tauriVersion],
  ["src-tauri/Cargo.toml", cargoVersion],
]) {
  if (declared !== version) {
    throw new Error(`${tag} does not match ${file} version ${declared}.`);
  }
}

console.log(`Release ${tag} matches all application versions.`);
