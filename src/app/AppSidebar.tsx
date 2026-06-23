import { Link, useLocation } from "react-router-dom";
import {
  Archive,
  Clock3,
  Database,
  FileSearch,
  FolderTree,
  Images,
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
  SidebarSeparator,
} from "@/components/ui/sidebar";
import { useCases } from "@/features/cases/case-provider";

const navItems = [
  { label: "Case", path: "/case", icon: ShieldCheck },
  { label: "Files", path: "/files", icon: FolderTree },
  { label: "Search", path: "/search", icon: Search },
  { label: "Media", path: "/media", icon: Images },
  { label: "Plugins", path: "/plugins", icon: Plug },
  { label: "Artifacts", path: "/artifacts", icon: FileSearch },
  { label: "Timeline", path: "/timeline", icon: Clock3 },
  { label: "Reports", path: "/reports", icon: Archive },
];

export function AppSidebar() {
  const location = useLocation();
  const { activeCase, isLoading } = useCases();

  return (
    <Sidebar collapsible="icon" className="border-r">
      <SidebarHeader className="gap-1 p-1.5">
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton size="sm" className="h-8" tooltip="Cultivator">
              <Database className="size-3.5" aria-hidden="true" />
              <span className="text-xs font-semibold">Cultivator</span>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarHeader>
      <SidebarSeparator />
      <SidebarContent className="gap-1">
        <SidebarGroup className="p-1.5">
          <SidebarGroupLabel className="h-6 px-1.5 text-[11px]">
            Workspace
          </SidebarGroupLabel>
          <SidebarGroupContent>
            <SidebarMenu className="gap-0.5">
              {navItems.map((item) => {
                const Icon = item.icon;
                const isActive =
                  location.pathname === item.path ||
                  (item.path === "/files" && location.pathname === "/");

                return (
                  <SidebarMenuItem key={item.path}>
                    <SidebarMenuButton
                      asChild
                      size="sm"
                      isActive={isActive}
                      tooltip={item.label}
                      className="h-7 px-1.5 text-xs"
                    >
                      <Link to={item.path}>
                        <Icon className="size-3.5" aria-hidden="true" />
                        <span>{item.label}</span>
                      </Link>
                    </SidebarMenuButton>
                  </SidebarMenuItem>
                );
              })}
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarContent>
      <SidebarFooter className="p-1.5">
        <div className="rounded-sm border bg-background px-2 py-1.5 text-[11px] group-data-[collapsible=icon]:hidden">
          <div className="font-medium">Active case</div>
          <div className="truncate text-muted-foreground">
            {isLoading ? "Loading..." : activeCase?.name ?? "None"}
          </div>
        </div>
      </SidebarFooter>
      <SidebarRail />
    </Sidebar>
  );
}
