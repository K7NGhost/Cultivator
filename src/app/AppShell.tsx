import type { CSSProperties } from "react";
import { useEffect, useState } from "react";
import { useLocation, useNavigate } from "react-router-dom";

import { AppMenubar } from "@/app/AppMenubar";
import { AppSidebar } from "@/app/AppSidebar";
import { StartupLoadingOverlay } from "@/app/StartupLoadingOverlay";
import { loadSidebarOpenState, saveSidebarOpenState } from "@/app/sidebarState";
import { ModeToggle } from "@/components/mode-toggle";
import {
  SidebarInset,
  SidebarProvider,
  SidebarTrigger,
} from "@/components/ui/sidebar";
import { ArtifactsPage } from "@/features/artifacts/pages/ArtifactsPage";
import { CasePage } from "@/features/cases/pages/CasePage";
import { CaseProvider, useCases } from "@/features/cases";
import { EvidenceProvider } from "@/features/evidence";
import { FilesPage } from "@/features/files/pages/FilesPage";
import { MediaPage } from "@/features/media/pages/MediaPage";
import { DronePage } from "@/features/drone/pages/DronePage";
import { MapPage } from "@/features/mapping/pages/MapPage";
import { TimelinePage } from "@/features/timeline/pages/TimelinePage";
import { PluginsPage } from "@/features/plugins/pages/PluginsPage";
import { usePluginJobMonitor } from "@/features/plugins/hooks/usePluginJobMonitor";
import { SearchPage } from "@/features/search/pages/SearchPage";
import { PlaceholderPage } from "@/features/workspace";
import { SettingsPage } from "@/features/settings/SettingsPage";
import { initializePerformanceSettings } from "@/features/settings/performanceSettings";

const workspaceRoutes = [
  { path: "/drone", element: <DronePage /> },
  { path: "/map", element: <MapPage /> },
  { path: "/settings", element: <SettingsPage /> },
  {
    path: "/case",
    element: <CasePage />,
  },
  {
    path: "/files",
    element: <FilesPage />,
  },
  {
    path: "/search",
    element: <SearchPage />,
  },
  {
    path: "/media",
    element: <MediaPage />,
  },
  {
    path: "/plugins",
    element: <PluginsPage />,
  },
  {
    path: "/artifacts",
    element: <ArtifactsPage />,
  },
  {
    path: "/timeline",
    element: <TimelinePage />,
  },
  {
    path: "/reports",
    element: (
      <PlaceholderPage
        title="Reports"
        description="Findings, exports, evidence summaries, and review packages."
      />
    ),
  },
] as const;

export function AppShell() {
  return (
    <CaseProvider>
      <EvidenceProvider>
        <AppShellContent />
      </EvidenceProvider>
    </CaseProvider>
  );
}

function AppShellContent() {
  useEffect(() => { void initializePerformanceSettings().catch((error: unknown) => console.error("Could not restore memory settings", error)); }, []);
  const [isSidebarOpen, setIsSidebarOpen] = useState(loadSidebarOpenState);
  const { activeCase } = useCases();
  usePluginJobMonitor(activeCase);

  function updateSidebarOpenState(isOpen: boolean) {
    setIsSidebarOpen(isOpen);
    saveSidebarOpenState(isOpen);
  }

  return (
    <SidebarProvider
      open={isSidebarOpen}
      onOpenChange={updateSidebarOpenState}
      className="h-svh overflow-hidden"
      style={
        {
          "--sidebar-width": "13rem",
          "--sidebar-width-icon": "2.75rem",
        } as CSSProperties
      }
    >
      <AppSidebar />
      <SidebarInset className="min-h-0 min-w-0 overflow-hidden">
        <header className="flex h-8 shrink-0 items-center border-b bg-background px-2">
          <SidebarTrigger className="mr-1 size-6" />
          <AppMenubar />
          <div className="ml-auto max-w-64 truncate text-[11px] text-muted-foreground">
            {activeCase ? `Case: ${activeCase.name}` : "No active case"}
          </div>
          <ModeToggle />
        </header>
        <main className="min-h-0 flex-1 overflow-hidden">
          <PersistentWorkspace />
        </main>
      </SidebarInset>
      <StartupLoadingOverlay />
    </SidebarProvider>
  );
}

function PersistentWorkspace() {
  const location = useLocation();
  const navigate = useNavigate();
  const activePath =
    location.pathname === "/" ? "/files" : normalizeWorkspacePath(location.pathname);
  const [visitedPaths, setVisitedPaths] = useState(() => new Set([activePath]));

  useEffect(() => {
    if (location.pathname === "/" || activePath !== location.pathname) {
      navigate(activePath, { replace: true });
    }
  }, [activePath, location.pathname, navigate]);

  useEffect(() => {
    setVisitedPaths((currentPaths) => {
      if (currentPaths.has(activePath)) {
        return currentPaths;
      }

      return new Set([...currentPaths, activePath]);
    });
  }, [activePath]);

  return (
    <>
      {workspaceRoutes.map((route) => (
        visitedPaths.has(route.path) &&
        <section
          key={route.path}
          className={route.path === activePath ? "h-full min-h-0" : "hidden"}
          aria-hidden={route.path !== activePath}
        >
          {route.element}
        </section>
      ))}
    </>
  );
}

function normalizeWorkspacePath(pathname: string) {
  return workspaceRoutes.some((route) => route.path === pathname)
    ? pathname
    : "/files";
}
