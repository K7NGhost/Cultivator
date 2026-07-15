export function isSafePluginFolderName(folderName: string) {
  return /^[A-Za-z0-9._-]+$/.test(folderName);
}

export function isSafePluginOrganizationPath(folderPath: string) {
  const normalized = folderPath.trim().replace(/\\/g, "/");

  if (!normalized) {
    return true;
  }

  if (normalized.startsWith("/") || /^[A-Za-z]:/.test(normalized)) {
    return false;
  }

  return normalized.split("/").every(
    (segment) =>
      Boolean(segment) &&
      segment !== "." &&
      segment !== ".." &&
      /^[A-Za-z0-9 ._-]+$/.test(segment),
  );
}
