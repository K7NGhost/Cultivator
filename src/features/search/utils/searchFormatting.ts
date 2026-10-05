export function formatElapsedTime(milliseconds: number | null) {
  if (milliseconds === null) {
    return "-";
  }

  if (milliseconds < 1000) {
    return `${milliseconds} ms`;
  }

  return `${(milliseconds / 1000).toFixed(2)} s`;
}

export function formatFileScanCount(
  scannedFiles: number | null,
  totalFiles: number | null,
  isSearching: boolean,
  totalComplete: boolean,
) {
  if (scannedFiles === null || totalFiles === null) {
    return isSearching ? "Starting..." : "-";
  }

  if (isSearching && !totalComplete) {
    return `${totalFiles.toLocaleString()} counted`;
  }

  if (isSearching && scannedFiles === totalFiles) {
    return `Scanning ${totalFiles.toLocaleString()}`;
  }

  return `${scannedFiles.toLocaleString()} / ${totalFiles.toLocaleString()}`;
}

