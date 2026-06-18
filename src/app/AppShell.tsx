import type { CSSProperties } from "react";
import { useEffect, useState } from "react";
import { useLocation, useNavigate } from "react-router-dom";

import { AppMenubar } from "@/app/AppMenubar";
import { AppSidebar } from "@/app/AppSidebar";
import { loadSidebarOpenState, saveSidebarOpenState } from "@/app/sidebarState";
import { ModeToggle } from "@/components/mode-toggle";
import {
  SidebarInset,
  SidebarProvider,
  SidebarTrigger,
} from "@/components/ui/sidebar";
import { ArtifactsPage } from "@/features/artifacts/ArtifactsPage";
import { CasePage } from "@/features/cases/CasePage";
import { CaseProvider, useCases } from "@/features/cases/case-provider";
import { EvidenceProvider } from "@/features/evidence/evidence-provider";
import { FilesPage } from "@/features/files/FilesPage";
import { PluginsPage } from "@/features/plugins/PluginsPage";
import { SearchPage } from "@/features/search/SearchPage";
import { PlaceholderPage } from "@/features/workspace/PlaceholderPage";

const workspaceRoutes = [
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
    path: "/plugins",
    element: <PluginsPage />,
  },
  {
    path: "/artifacts",
    element: <ArtifactsPage />,
  },
  {
    path: "/timeline",
    element: (
      <PlaceholderPage
        title="Timeline"
        description="Timestamps correlated from file metadata and plugin outputs."
      />
    ),
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
  const [isSidebarOpen, setIsSidebarOpen] = useState(loadSidebarOpenState);
  const { activeCase } = useCases();

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
    </SidebarProvider>
  );
}

function PersistentWorkspace() {
  const location = useLocation();
  const navigate = useNavigate();
  const activePath =
    location.pathname === "/" ? "/files" : normalizeWorkspacePath(location.pathname);

  useEffect(() => {
    if (location.pathname === "/" || activePath !== location.pathname) {
      navigate(activePath, { replace: true });
    }
  }, [activePath, location.pathname, navigate]);

  return (
    <>
      {workspaceRoutes.map((route) => (
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
