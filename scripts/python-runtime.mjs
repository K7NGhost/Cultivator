import { createWriteStream } from "node:fs";
import { mkdir, rm, cp, readdir, stat } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { pipeline } from "node:stream/promises";
import { execFileSync } from "node:child_process";

const GITHUB_RELEASES_API =
  "https://api.github.com/repos/astral-sh/python-build-standalone/releases/latest";
const DEFAULT_PYTHON_VERSION = "3.12";

const REPO_ROOT = path.resolve(fileURLToPath(new URL("..", import.meta.url)));
const RUNTIME_ROOT = path.join(REPO_ROOT, "src-tauri", "python-runtime");
const DOWNLOAD_ROOT = path.join(REPO_ROOT, ".cache", "python-runtime");

export function getHostTarget() {
  const platform = process.platform;
  const arch = process.arch;

  if (platform === "win32" && arch === "x64") {
    return {
      key: "windows-x64",
      triple: "x86_64-pc-windows-msvc",
      pythonExecutable: ["python.exe"],
      libraryDirs: ["", "DLLs"],
    };
  }

  if (platform === "darwin" && arch === "x64") {
    return {
      key: "macos-x64",
      triple: "x86_64-apple-darwin",
      pythonExecutable: ["bin", "python3"],
      libraryDirs: ["lib"],
    };
  }

  if (platform === "darwin" && arch === "arm64") {
    return {
      key: "macos-arm64",
      triple: "aarch64-apple-darwin",
      pythonExecutable: ["bin", "python3"],
      libraryDirs: ["lib"],
    };
  }

  if (platform === "linux" && arch === "x64") {
    return {
      key: "linux-x64",
      triple: "x86_64-unknown-linux-gnu",
      pythonExecutable: ["bin", "python3"],
      libraryDirs: ["lib"],
    };
  }

  if (platform === "linux" && arch === "arm64") {
    return {
      key: "linux-arm64",
      triple: "aarch64-unknown-linux-gnu",
      pythonExecutable: ["bin", "python3"],
      libraryDirs: ["lib"],
    };
  }

  throw new Error(`Unsupported Python runtime target: ${platform}/${arch}`);
}

export function getRuntimePaths(target = getHostTarget()) {
  const runtimeDir = path.join(RUNTIME_ROOT, target.key);
  const pythonHome = runtimeDir;
  const pythonExecutable = path.join(runtimeDir, ...target.pythonExecutable);
  const libraryDirs = target.libraryDirs.map((segment) =>
    segment ? path.join(runtimeDir, segment) : runtimeDir,
  );

  return {
    runtimeDir,
    pythonHome,
    pythonExecutable,
    libraryDirs,
  };
}

export function createPythonRuntimeEnv(baseEnv = process.env) {
  const target = getHostTarget();
  const paths = getRuntimePaths(target);
  const separator = process.platform === "win32" ? ";" : ":";
  const loaderKey =
    process.platform === "darwin"
      ? "DYLD_LIBRARY_PATH"
      : process.platform === "linux"
        ? "LD_LIBRARY_PATH"
        : null;
  const cleanBaseEnv = Object.fromEntries(
    Object.entries(baseEnv).filter(
      ([key, value]) => key && !key.startsWith("=") && value !== undefined,
    ),
  );
  const pathKey =
    process.platform === "win32"
      ? Object.keys(cleanBaseEnv).find((key) => key.toLowerCase() === "path") ?? "Path"
      : "PATH";
  const env = {
    ...cleanBaseEnv,
    PYO3_PYTHON: paths.pythonExecutable,
    PYTHONHOME: paths.pythonHome,
    PYTHONDONTWRITEBYTECODE: "1",
    [pathKey]: [paths.runtimeDir, ...paths.libraryDirs, cleanBaseEnv[pathKey]]
      .filter(Boolean)
      .join(separator),
  };

  if (loaderKey) {
    env[loaderKey] = [paths.runtimeDir, ...paths.libraryDirs, cleanBaseEnv[loaderKey]]
      .filter(Boolean)
      .join(separator);
  }

  return env;
}

export async function setupPythonRuntime() {
  const target = getHostTarget();
  const paths = getRuntimePaths(target);

  if (
    process.env.CULTIVATOR_REFRESH_PYTHON_RUNTIME !== "1" &&
    (await pathExists(paths.pythonExecutable))
  ) {
    return {
      target,
      paths,
      assetName: "existing local runtime",
    };
  }

  const release = await fetchLatestRelease();
  const asset = selectRuntimeAsset(release.assets, target);
  const archivePath = path.join(DOWNLOAD_ROOT, asset.name);
  const extractRoot = path.join(DOWNLOAD_ROOT, `${target.key}-extract`);

  await mkdir(DOWNLOAD_ROOT, { recursive: true });
  await mkdir(RUNTIME_ROOT, { recursive: true });
  await downloadAsset(asset.browser_download_url, archivePath);
  await rm(extractRoot, { recursive: true, force: true });
  await mkdir(extractRoot, { recursive: true });

  execFileSync("tar", ["-xzf", archivePath, "-C", extractRoot], {
    stdio: "inherit",
  });

  const extractedPythonRoot = await findExtractedPythonRoot(extractRoot);

  await rm(paths.runtimeDir, { recursive: true, force: true });
  await cp(extractedPythonRoot, paths.runtimeDir, { recursive: true });

  return {
    target,
    paths,
    assetName: asset.name,
  };
}

async function fetchLatestRelease() {
  const response = await fetch(GITHUB_RELEASES_API, {
    headers: {
      Accept: "application/vnd.github+json",
      "User-Agent": "cultivator-python-runtime-installer",
    },
  });

  if (!response.ok) {
    throw new Error(
      `Failed to read python-build-standalone release metadata: ${response.status} ${response.statusText}`,
    );
  }

  return response.json();
}

function selectRuntimeAsset(assets, target) {
  const pythonVersion =
    process.env.CULTIVATOR_PYTHON_VERSION ?? DEFAULT_PYTHON_VERSION;
  const candidates = assets
    .filter((asset) => asset.name.endsWith(".tar.gz"))
    .filter((asset) => asset.name.startsWith(`cpython-${pythonVersion}.`))
    .filter((asset) => asset.name.includes(target.triple))
    .filter((asset) => asset.name.includes("install_only"))
    .sort((left, right) => scoreAsset(right.name) - scoreAsset(left.name));

  const selected = candidates[0];

  if (!selected) {
    throw new Error(
      `No CPython ${pythonVersion} install_only python-build-standalone asset found for ${target.triple}.`,
    );
  }

  return selected;
}

function scoreAsset(name) {
  let score = 0;

  if (name.includes("stripped")) score += 10;
  if (name.includes("pgo+lto")) score += 5;
  if (name.includes("debug")) score -= 20;
  if (name.includes("freethreaded")) score -= 5;

  return score;
}

async function downloadAsset(url, archivePath) {
  const response = await fetch(url, {
    headers: {
      "User-Agent": "cultivator-python-runtime-installer",
    },
  });

  if (!response.ok || !response.body) {
    throw new Error(`Failed to download Python runtime: ${response.statusText}`);
  }

  await pipeline(response.body, createWriteStream(archivePath));
}

async function findExtractedPythonRoot(extractRoot) {
  const entries = await readdir(extractRoot);
  const directPythonRoot = path.join(extractRoot, "python");

  if (await pathExists(directPythonRoot)) {
    return directPythonRoot;
  }

  for (const entry of entries) {
    const candidate = path.join(extractRoot, entry, "python");
    if (await pathExists(candidate)) {
      return candidate;
    }
  }

  throw new Error(`Could not find extracted python directory in ${extractRoot}`);
}

async function pathExists(value) {
  try {
    await stat(value);
    return true;
  } catch {
    return false;
  }
}
