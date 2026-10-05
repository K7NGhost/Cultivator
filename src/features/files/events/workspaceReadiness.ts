export const filesWorkspaceReadyEvent = "cultivator:files-workspace-ready";

export type FilesWorkspaceReadyDetail = {
  caseId: string;
  datasourceCount: number;
  error?: string;
};

const latestFilesWorkspaceReadiness = new Map<
  string,
  FilesWorkspaceReadyDetail
>();

export function notifyFilesWorkspaceReady(detail: FilesWorkspaceReadyDetail) {
  latestFilesWorkspaceReadiness.set(detail.caseId, detail);
  window.dispatchEvent(
    new CustomEvent<FilesWorkspaceReadyDetail>(filesWorkspaceReadyEvent, {
      detail,
    }),
  );
}

export function getFilesWorkspaceReadiness(caseId: string) {
  return latestFilesWorkspaceReadiness.get(caseId) ?? null;
}
