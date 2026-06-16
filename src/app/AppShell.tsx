import type { CSSProperties } from "react";
import { Outlet } from "react-router-dom";

import { AppMenubar } from "@/app/AppMenubar";
import { AppSidebar } from "@/app/AppSidebar";
import { ModeToggle } from "@/components/mode-toggle";
import {
  SidebarInset,
  SidebarProvider,
  SidebarTrigger,
} from "@/components/ui/sidebar";
import { EvidenceProvider } from "@/features/evidence/evidence-provider";

export function AppShell() {
  return (
    <EvidenceProvider>
      <SidebarProvider
        defaultOpen
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
            <div className="ml-auto text-[11px] text-muted-foreground">
              Directory mode
            </div>
            <ModeToggle />
          </header>
          <main className="min-h-0 flex-1 overflow-hidden">
            <Outlet />
          </main>
        </SidebarInset>
      </SidebarProvider>
    </EvidenceProvider>
  );
}
