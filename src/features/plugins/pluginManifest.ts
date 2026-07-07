export function isSafePluginFolderName(folderName: string) {
  return /^[A-Za-z0-9._-]+$/.test(folderName);
}
