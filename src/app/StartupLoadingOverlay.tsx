import { useEffect, useState } from "react";
import { Database, FolderTree, LoaderCircle } from "lucide-react";
import { useLocation } from "react-router-dom";

import { Button } from "@/components/ui/button";
import {
  filesWorkspaceReadyEvent,
  getFilesWorkspaceReadiness,
  type FilesWorkspaceReadyDetail,
} from "@/app/startupEvents";
import { useCases } from "@/features/cases/case-provider";
import { listDataSources } from "@/features/datasources/dataSourceRepository";

type StartupState =
  | { stage: "cases" }
  | { stage: "datasources"; caseName: string }
  | { stage: "workspace"; caseName: string; datasourceCount: number }
  | { stage: "error"; message: string }
  | { stage: "ready" };

export function StartupLoadingOverlay() {
  const location = useLocation();
  const { activeCase, error: caseError, isLoading: areCasesLoading } = useCases();
  const [state, setState] = useState<StartupState>({ stage: "cases" });

  useEffect(() => {
    if (areCasesLoading) {
      setState({ stage: "cases" });
      return;
    }

    if (caseError) {
      setState({ stage: "error", message: caseError });
      return;
    }

    if (!activeCase) {
      setState({ stage: "ready" });
      return;
    }

    let isCurrent = true;
    let firstFrame = 0;
    let secondFrame = 0;

    const finishAfterPaint = () => {
      firstFrame = window.requestAnimationFrame(() => {
        secondFrame = window.requestAnimationFrame(() => {
          if (isCurrent) {
            setState({ stage: "ready" });
          }
        });
      });
    };

    const applyWorkspaceReadiness = (detail: FilesWorkspaceReadyDetail) => {
      if (!isCurrent || detail.caseId !== activeCase.id) {
        return;
      }

      if (detail.error) {
        setState({ stage: "error", message: detail.error });
        return;
      }

      setState({
        stage: "workspace",
        caseName: activeCase.name,
        datasourceCount: detail.datasourceCount,
      });
      finishAfterPaint();
    };

    const handleWorkspaceReady = (event: Event) => {
      if (!(event instanceof CustomEvent)) {
        return;
      }

      applyWorkspaceReadiness(event.detail as FilesWorkspaceReadyDetail);
    };

    window.addEventListener(filesWorkspaceReadyEvent, handleWorkspaceReady);

    setState({ stage: "datasources", caseName: activeCase.name });
    listDataSources(activeCase.databasePath, activeCase.id)
      .then((dataSources) => {
        if (!isCurrent) {
          return;
        }

        setState({
          stage: "workspace",
          caseName: activeCase.name,
          datasourceCount: dataSources.length,
        });
        if (
          dataSources.length === 0 ||
          (location.pathname !== "/" && location.pathname !== "/files")
        ) {
          finishAfterPaint();
          return;
        }

        const existingReadiness = getFilesWorkspaceReadiness(activeCase.id);
        if (existingReadiness) {
          applyWorkspaceReadiness(existingReadiness);
        }
      })
      .catch((caughtError) => {
        if (isCurrent) {
          setState({
            stage: "error",
            message:
              caughtError instanceof Error
                ? caughtError.message
                : String(caughtError),
          });
        }
      });

    return () => {
      isCurrent = false;
      window.removeEventListener(filesWorkspaceReadyEvent, handleWorkspaceReady);
      window.cancelAnimationFrame(firstFrame);
      window.cancelAnimationFrame(secondFrame);
    };
  }, [activeCase, areCasesLoading, caseError, location.pathname]);

  if (state.stage === "ready") {
    return null;
  }

  const isError = state.stage === "error";
  const title = getTitle(state);
  const detail = getDetail(state);

  return (
    <div
      className="fixed inset-0 z-[10000] flex items-center justify-center bg-background/95 backdrop-blur-sm"
      role={isError ? "alert" : "status"}
      aria-live="polite"
      aria-busy={!isError}
    >
      <section className="w-[min(28rem,calc(100vw-2rem))] overflow-hidden rounded-md border bg-card shadow-2xl">
        <div className="flex items-center gap-3 border-b px-4 py-3">
          <div className="flex size-9 shrink-0 items-center justify-center rounded-md border bg-muted">
            {isError ? (
              <Database className="size-4 text-destructive" aria-hidden="true" />
            ) : state.stage === "workspace" ? (
              <FolderTree className="size-4 text-primary" aria-hidden="true" />
            ) : (
              <LoaderCircle
                className="size-4 animate-spin text-primary"
                aria-hidden="true"
              />
            )}
          </div>
          <div className="min-w-0">
            <h1 className="truncate text-sm font-semibold">{title}</h1>
            <p className="mt-0.5 break-words text-xs text-muted-foreground">
              {detail}
            </p>
          </div>
        </div>

        {!isError ? (
          <div className="h-1 overflow-hidden bg-muted">
            <div className="h-full w-1/3 animate-[startup-loading_1.15s_ease-in-out_infinite] bg-primary" />
          </div>
        ) : (
          <div className="flex justify-end px-4 py-3">
            <Button
              type="button"
              size="sm"
              variant="outline"
              onClick={() => setState({ stage: "ready" })}
            >
              Continue
            </Button>
          </div>
        )}
      </section>
    </div>
  );
}

function getTitle(state: StartupState) {
  switch (state.stage) {
    case "cases":
      return "Opening Cultivator";
    case "datasources":
      return "Loading datasources";
    case "workspace":
      return "Preparing workspace";
    case "error":
      return "Startup could not finish";
    case "ready":
      return "Ready";
  }
}

function getDetail(state: StartupState) {
  switch (state.stage) {
    case "cases":
      return "Restoring the case catalog and active case.";
    case "datasources":
      return `Opening ${state.caseName} and reading its datasource records, paths, and assigned plugins.`;
    case "workspace":
      return `Loaded ${state.datasourceCount.toLocaleString()} datasource${
        state.datasourceCount === 1 ? "" : "s"
      } for ${state.caseName}. Reading datasource paths and building the file tree.`;
    case "error":
      return state.message;
    case "ready":
      return "The workspace is ready.";
  }
}
