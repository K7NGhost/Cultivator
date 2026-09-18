import { Link, useLocation } from "react-router-dom";
import {
  Archive,
  ChevronRight,
  Clock3,
  Database,
  FileSearch,
  FolderTree,
  Images,
  PanelLeftClose,
  PanelLeftOpen,
  Plug,
  Search,
  ShieldCheck,
} from "lucide-react";

import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarRail,
  useSidebar,
} from "@/components/ui/sidebar";
import { useCases } from "@/features/cases/case-provider";
import { cn } from "@/lib/utils";

const navGroups = [
  {
    label: "Workspace",
    items: [
      { label: "Case overview", path: "/case", icon: ShieldCheck },
      { label: "Files", path: "/files", icon: FolderTree },
      { label: "Search", path: "/search", icon: Search },
      { label: "Media", path: "/media", icon: Images },
    ],
  },
  {
    label: "Analysis",
    items: [
      { label: "Plugins", path: "/plugins", icon: Plug },
      { label: "Artifacts", path: "/artifacts", icon: FileSearch },
      { label: "Timeline", path: "/timeline", icon: Clock3 },
    ],
  },
  {
    label: "Reporting",
    items: [{ label: "Reports", path: "/reports", icon: Archive }],
  },
];

export function AppSidebar() {
  const location = useLocation();
  const { activeCase, isLoading } = useCases();
  const { state, toggleSidebar, isMobile, setOpenMobile } = useSidebar();
  const isCollapsed = state === "collapsed" && !isMobile;
  const caseName = isLoading ? "Loading case…" : activeCase?.name ?? "No case open";
  const caseDetail = isLoading
    ? "Please wait"
    : activeCase?.reference || (activeCase ? "View case details" : "Create or open a case");

  function handleNavigate() {
    if (isMobile) setOpenMobile(false);
  }

  return (
    <Sidebar collapsible="icon" className="border-sidebar-border">
      <SidebarHeader className="border-b border-sidebar-border p-1.5">
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton
              asChild
              tooltip="Cultivator · Case overview"
              className="h-11 gap-2.5 rounded-sm px-2 group-data-[collapsible=icon]:my-1.5"
            >
              <Link to="/case" onClick={handleNavigate} aria-label="Cultivator — Case overview">
                <span className="flex size-7 shrink-0 items-center justify-center rounded-sm bg-sidebar-primary text-sidebar-primary-foreground group-data-[collapsible=icon]:size-4 group-data-[collapsible=icon]:rounded-none group-data-[collapsible=icon]:bg-transparent group-data-[collapsible=icon]:text-sidebar-foreground">
                  <Database className="size-4" aria-hidden="true" />
                </span>
                <span className="flex min-w-0 flex-col gap-0.5 group-data-[collapsible=icon]:hidden">
                  <span className="text-sm font-semibold tracking-tight">Cultivator</span>
                  <span className="text-[11px] text-muted-foreground">Evidence workspace</span>
                </span>
              </Link>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarHeader>

      <SidebarContent className="gap-1 py-2 group-data-[collapsible=icon]:gap-2">
        <nav aria-label="Workspace navigation">
          {navGroups.map((group, groupIndex) => (
            <SidebarGroup
              key={group.label}
              className={cn(
                "px-1.5 py-1",
                groupIndex > 0 && "mt-2 group-data-[collapsible=icon]:border-t group-data-[collapsible=icon]:border-sidebar-border group-data-[collapsible=icon]:pt-2",
              )}
            >
              <SidebarGroupLabel className="mb-1 h-5 px-2 text-[10px] font-semibold uppercase tracking-widest text-muted-foreground group-data-[collapsible=icon]:hidden">
                {group.label}
              </SidebarGroupLabel>
              <SidebarGroupContent>
                <SidebarMenu className="gap-0.5">
                  {group.items.map((item) => {
                    const Icon = item.icon;
                    const isActive = location.pathname === item.path ||
                      (item.path === "/files" && location.pathname === "/");

                    return (
                      <SidebarMenuItem key={item.path}>
                        <SidebarMenuButton
                          asChild
                          size="sm"
                          isActive={isActive}
                          tooltip={item.label}
                          className="relative h-8 gap-2.5 rounded-sm px-2 text-xs text-muted-foreground transition-colors before:absolute before:inset-y-2 before:left-0 before:w-0.5 before:rounded-full before:bg-transparent data-[active=true]:bg-sidebar-accent data-[active=true]:font-semibold data-[active=true]:text-sidebar-foreground data-[active=true]:before:bg-sidebar-primary"
                        >
                          <Link
                            to={item.path}
                            onClick={handleNavigate}
                            aria-label={item.label}
                            aria-current={isActive ? "page" : undefined}
                          >
                            <Icon className={cn("size-4", isActive && "text-sidebar-primary dark:text-white")} aria-hidden="true" />
                            <span className="group-data-[collapsible=icon]:hidden">{item.label}</span>
                          </Link>
                        </SidebarMenuButton>
                      </SidebarMenuItem>
                    );
                  })}
                </SidebarMenu>
              </SidebarGroupContent>
            </SidebarGroup>
          ))}
        </nav>
      </SidebarContent>

      <SidebarFooter className="gap-1 border-t border-sidebar-border p-1.5">
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton
              asChild
              tooltip={caseName}
              className="h-auto min-h-16 gap-2 rounded-sm px-2 py-2 group-data-[collapsible=icon]:min-h-0"
            >
              <Link to="/case" onClick={handleNavigate} aria-label={`${caseName} — Case overview`}>
                <ShieldCheck className="size-4 text-muted-foreground" aria-hidden="true" />
                <span className="flex min-w-0 flex-1 flex-col gap-0.5 group-data-[collapsible=icon]:hidden">
                  <span className="text-[10px] font-medium uppercase tracking-wider text-muted-foreground">Active case</span>
                  <span className="truncate text-xs font-semibold" title={caseName}>{caseName}</span>
                  <span className="truncate text-[11px] text-muted-foreground" title={caseDetail}>{caseDetail}</span>
                </span>
                <ChevronRight className="ml-auto size-3 text-muted-foreground group-data-[collapsible=icon]:hidden" aria-hidden="true" />
              </Link>
            </SidebarMenuButton>
          </SidebarMenuItem>
          <SidebarMenuItem>
            <SidebarMenuButton
              size="sm"
              onClick={toggleSidebar}
              tooltip={isCollapsed ? "Expand sidebar" : "Collapse sidebar"}
              aria-label={isCollapsed ? "Expand sidebar" : "Collapse sidebar"}
              className="h-7 rounded-sm px-2 text-[11px] text-muted-foreground"
            >
              {isCollapsed ? <PanelLeftOpen aria-hidden="true" /> : <PanelLeftClose aria-hidden="true" />}
              <span className="group-data-[collapsible=icon]:hidden">Collapse sidebar</span>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarFooter>
      <SidebarRail />
    </Sidebar>
  );
}
