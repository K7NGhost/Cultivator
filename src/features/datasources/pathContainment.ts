/**
 * Path comparisons for picking evidence sources. Windows paths compare
 * case-insensitively and with either slash, so both are normalized away.
 */
function comparablePath(path: string) {
  return path.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();
}

/** Whether `path` is `folder` itself or somewhere inside it. */
export function isSameOrInside(path: string, folder: string) {
  const child = comparablePath(path);
  const parent = comparablePath(folder);

  return child === parent || child.startsWith(`${parent}/`);
}

/**
 * The selected sources that contain the case folder. Adding one would make
 * Cultivator treat its own case database and exports as evidence.
 */
export function sourcesContainingCaseFolder(sourcePaths: string[], caseFolderPath: string) {
  return sourcePaths.filter((sourcePath) => isSameOrInside(caseFolderPath, sourcePath));
}
