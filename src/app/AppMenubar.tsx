import { useNavigate } from "react-router-dom";
import {
  FileArchive,
  FolderOpen,
  HelpCircle,
  Play,
  Settings,
} from "lucide-react";

import {
  Menubar,
  MenubarContent,
  MenubarItem,
  MenubarMenu,
  MenubarSeparator,
  MenubarShortcut,
  MenubarTrigger,
} from "@/components/ui/menubar";

const triggerClassName = "px-1.5 py-0.5 text-xs";
const itemClassName = "py-1 text-xs";

export function AppMenubar() {
  const navigate = useNavigate();

  return (
    <Menubar className="h-7 gap-0.5 rounded-none border-0 bg-transparent p-0 shadow-none">
      <MenubarMenu>
        <MenubarTrigger className={triggerClassName}>Case</MenubarTrigger>
        <MenubarContent>
          <MenubarItem className={itemClassName} onSelect={() => navigate("/case")}>
            New case
            <MenubarShortcut>Ctrl+N</MenubarShortcut>
          </MenubarItem>
          <MenubarItem className={itemClassName} onSelect={() => navigate("/case")}>
            Open case
            <MenubarShortcut>Ctrl+O</MenubarShortcut>
          </MenubarItem>
          <MenubarSeparator />
          <MenubarItem className={itemClassName} disabled>
            Save snapshot
          </MenubarItem>
        </MenubarContent>
      </MenubarMenu>

      <MenubarMenu>
        <MenubarTrigger className={triggerClassName}>Evidence</MenubarTrigger>
        <MenubarContent>
          <MenubarItem className={itemClassName} onSelect={() => navigate("/files")}>
            <FolderOpen className="size-3.5" aria-hidden="true" />
            Open directory
          </MenubarItem>
          <MenubarItem className={itemClassName} disabled>
            Add logical source
          </MenubarItem>
          <MenubarItem className={itemClassName} disabled>
            Refresh index
          </MenubarItem>
        </MenubarContent>
      </MenubarMenu>

      <MenubarMenu>
        <MenubarTrigger className={triggerClassName}>Plugins</MenubarTrigger>
        <MenubarContent>
          <MenubarItem className={itemClassName} onSelect={() => navigate("/plugins")}>
            Plugin registry
          </MenubarItem>
          <MenubarItem className={itemClassName} disabled>
            <Play className="size-3.5" aria-hidden="true" />
            Run selected script
          </MenubarItem>
          <MenubarItem className={itemClassName} disabled>
            New extractor script
          </MenubarItem>
        </MenubarContent>
      </MenubarMenu>

      <MenubarMenu>
        <MenubarTrigger className={triggerClassName}>Analysis</MenubarTrigger>
        <MenubarContent>
          <MenubarItem className={itemClassName} onSelect={() => navigate("/artifacts")}>
            Extracted artifacts
          </MenubarItem>
          <MenubarItem className={itemClassName} onSelect={() => navigate("/timeline")}>
            Timeline
          </MenubarItem>
          <MenubarItem className={itemClassName} onSelect={() => navigate("/reports")}>
            <FileArchive className="size-3.5" aria-hidden="true" />
            Export report
          </MenubarItem>
        </MenubarContent>
      </MenubarMenu>

      <MenubarMenu>
        <MenubarTrigger className={triggerClassName}>View</MenubarTrigger>
        <MenubarContent>
          <MenubarItem className={itemClassName} disabled>
            Toggle right inspector
          </MenubarItem>
          <MenubarItem className={itemClassName} disabled>
            Toggle status bar
          </MenubarItem>
          <MenubarSeparator />
          <MenubarItem className={itemClassName} disabled>
            <Settings className="size-3.5" aria-hidden="true" />
            Preferences
          </MenubarItem>
        </MenubarContent>
      </MenubarMenu>

      <MenubarMenu>
        <MenubarTrigger className={triggerClassName}>Help</MenubarTrigger>
        <MenubarContent>
          <MenubarItem className={itemClassName} disabled>
            <HelpCircle className="size-3.5" aria-hidden="true" />
            Documentation
          </MenubarItem>
          <MenubarItem className={itemClassName} disabled>
            About Cultivator
          </MenubarItem>
        </MenubarContent>
      </MenubarMenu>
    </Menubar>
  );
}
