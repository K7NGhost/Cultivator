import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import * as React from "react";

export type EvidenceEntryKind = "datasource" | "directory" | "file";

export type EvidenceTreeNode = {
  id: string;
  name: string;
  path: string;
  kind: EvidenceEntryKind;
  files: number;
  size?: number;
  modifiedMs?: number;
  childCount?: number;
  children?: EvidenceTreeNode[];
};

export type EvidenceDirectoryEntry = {
  id: string;
  name: string;
  path: string;
  kind: EvidenceEntryKind;
  size?: number;
  modifiedMs?: number;
  childCount?: number;
};

export type EvidenceDirectoryListing = {
  rootPath: string;
  rootName: string;
  tree: EvidenceTreeNode;
  entries: EvidenceDirectoryEntry[];
};

type EvidenceContextValue = {
  listing: EvidenceDirectoryListing | null;
  error: string | null;
  isLoading: boolean;
  openDirectory: () => Promise<void>;
  refreshDirectory: () => Promise<void>;
};

const EvidenceContext = React.createContext<EvidenceContextValue | null>(null);

export function EvidenceProvider({ children }: { children: React.ReactNode }) {
  const [listing, setListing] =
    React.useState<EvidenceDirectoryListing | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  const [isLoading, setIsLoading] = React.useState(false);

  const loadDirectory = React.useCallback(async (path: string) => {
    setIsLoading(true);
    setError(null);

    try {
      const nextListing = await invoke<EvidenceDirectoryListing>(
        "list_directory",
        { path },
      );
      setListing(nextListing);
    } catch (caughtError) {
      setError(
        caughtError instanceof Error
          ? caughtError.message
          : String(caughtError),
      );
    } finally {
      setIsLoading(false);
    }
  }, []);

  const openDirectory = React.useCallback(async () => {
    const selectedPath = await open({
      directory: true,
      multiple: false,
      title: "Open Evidence Directory",
    });

    if (typeof selectedPath !== "string") {
      return;
    }

    await loadDirectory(selectedPath);
  }, [loadDirectory]);

  const refreshDirectory = React.useCallback(async () => {
    if (!listing) {
      return;
    }

    await loadDirectory(listing.rootPath);
  }, [listing, loadDirectory]);

  const value = React.useMemo(
    () => ({
      listing,
      error,
      isLoading,
      openDirectory,
      refreshDirectory,
    }),
    [error, isLoading, listing, openDirectory, refreshDirectory],
  );

  return (
    <EvidenceContext.Provider value={value}>
      {children}
    </EvidenceContext.Provider>
  );
}

export function useEvidence() {
  const context = React.useContext(EvidenceContext);

  if (!context) {
    throw new Error("useEvidence must be used within EvidenceProvider");
  }

  return context;
}
