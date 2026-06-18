import { type FormEvent, useState } from "react";
import { useNavigate } from "react-router-dom";
import {
  FileArchive,
  FolderPlus,
  FolderOpen,
  HelpCircle,
  Plus,
  Play,
  Settings,
} from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import {
  Menubar,
  MenubarContent,
  MenubarItem,
  MenubarMenu,
  MenubarSeparator,
  MenubarShortcut,
  MenubarTrigger,
} from "@/components/ui/menubar";
import { useCases } from "@/features/cases/case-provider";
import { DataSourceWizardDialog } from "@/features/datasources/DataSourceWizardDialog";
import { useEvidence } from "@/features/evidence/evidence-provider";
import {
  createPythonPlugin,
  openPythonApiGuide,
  openPythonPluginFolder,
  openPythonPluginFolderInVscode,
} from "@/features/plugins/pluginRepository";

const triggerClassName = "px-1.5 py-0.5 text-xs";
const itemClassName = "py-1 text-xs";

export function AppMenubar() {
  const navigate = useNavigate();
  const { isLoading, openDirectory, refreshDirectory, listing } = useEvidence();
  const { activeCase } = useCases();
  const [isDataSourceWizardOpen, setIsDataSourceWizardOpen] = useState(false);
  const [isOpeningPluginFolder, setIsOpeningPluginFolder] = useState(false);
  const [isOpeningPluginFolderInVscode, setIsOpeningPluginFolderInVscode] =
    useState(false);
  const [isOpeningPythonApiGuide, setIsOpeningPythonApiGuide] = useState(false);
  const [isCreatePluginOpen, setIsCreatePluginOpen] = useState(false);
  const [newPluginName, setNewPluginName] = useState("");
  const [createPluginError, setCreatePluginError] = useState<string | null>(null);
  const [isCreatingPlugin, setIsCreatingPlugin] = useState(false);

  async function handleOpenDirectory() {
    navigate("/files");
    await openDirectory();
  }

  async function handleOpenPluginFolder() {
    setIsOpeningPluginFolder(true);

    try {
      await openPythonPluginFolder();
    } catch (error) {
      console.error("Failed to open Python plugin folder", error);
    } finally {
      setIsOpeningPluginFolder(false);
    }
  }

  async function handleOpenPluginFolderInVscode() {
    setIsOpeningPluginFolderInVscode(true);

    try {
      await openPythonPluginFolderInVscode();
    } catch (error) {
      console.error("Failed to open Python plugin folder in VS Code", error);
    } finally {
      setIsOpeningPluginFolderInVscode(false);
    }
  }

  async function handleOpenPythonApiGuide() {
    setIsOpeningPythonApiGuide(true);

    try {
      await openPythonApiGuide();
    } catch (error) {
      console.error("Failed to open Python API guide", error);
    } finally {
      setIsOpeningPythonApiGuide(false);
    }
  }

  async function handleCreatePlugin(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setIsCreatingPlugin(true);
    setCreatePluginError(null);

    try {
      await createPythonPlugin(newPluginName);
      setNewPluginName("");
      setIsCreatePluginOpen(false);
      navigate("/plugins");
    } catch (error) {
      setCreatePluginError(
        error instanceof Error ? error.message : String(error),
      );
    } finally {
      setIsCreatingPlugin(false);
    }
  }

  return (
    <>
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
            <MenubarItem
              className={itemClassName}
              disabled={isLoading}
              onSelect={handleOpenDirectory}
            >
              <FolderOpen className="size-3.5" aria-hidden="true" />
              {isLoading ? "Opening directory..." : "Open directory"}
            </MenubarItem>
            <MenubarItem
              className={itemClassName}
              disabled={!activeCase}
              onSelect={() => {
                setIsDataSourceWizardOpen(true);
              }}
            >
              <FolderPlus className="size-3.5" aria-hidden="true" />
              Add datasource
            </MenubarItem>
            <MenubarItem
              className={itemClassName}
              disabled={!listing || isLoading}
              onSelect={() => {
                void refreshDirectory();
              }}
            >
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
            <MenubarItem
              className={itemClassName}
              disabled={isOpeningPluginFolder}
              onSelect={() => {
                void handleOpenPluginFolder();
              }}
            >
              <FolderOpen className="size-3.5" aria-hidden="true" />
              {isOpeningPluginFolder ? "Opening plugin folder..." : "Open plugin folder"}
            </MenubarItem>
            <MenubarItem
              className={itemClassName}
              disabled={isOpeningPluginFolderInVscode}
              onSelect={() => {
                void handleOpenPluginFolderInVscode();
              }}
            >
              <FolderOpen className="size-3.5" aria-hidden="true" />
              {isOpeningPluginFolderInVscode
                ? "Opening in VS Code..."
                : "Open plugin folder in VS Code"}
            </MenubarItem>
            <MenubarItem
              className={itemClassName}
              disabled={isOpeningPythonApiGuide}
              onSelect={() => {
                void handleOpenPythonApiGuide();
              }}
            >
              <HelpCircle className="size-3.5" aria-hidden="true" />
              {isOpeningPythonApiGuide ? "Opening API guide..." : "Python API guide"}
            </MenubarItem>
            <MenubarItem
              className={itemClassName}
              onSelect={() => {
                setCreatePluginError(null);
                setIsCreatePluginOpen(true);
              }}
            >
              <Plus className="size-3.5" aria-hidden="true" />
              Create new Python plugin
            </MenubarItem>
            <MenubarSeparator />
            <MenubarItem className={itemClassName} disabled>
              <Play className="size-3.5" aria-hidden="true" />
              Run selected script
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
      <DataSourceWizardDialog
        activeCase={activeCase}
        open={isDataSourceWizardOpen}
        onOpenChange={setIsDataSourceWizardOpen}
      />
      <Dialog open={isCreatePluginOpen} onOpenChange={setIsCreatePluginOpen}>
        <DialogContent className="gap-3 p-4 sm:max-w-md">
          <DialogHeader className="gap-1">
            <DialogTitle className="text-base">Create Python plugin</DialogTitle>
            <DialogDescription className="text-xs">
              Creates a plugin folder with plugin.toml and plugin.py.
            </DialogDescription>
          </DialogHeader>
          <form className="space-y-3" onSubmit={handleCreatePlugin}>
            <div className="grid grid-cols-[96px_1fr] items-center gap-2">
              <span className="text-xs text-muted-foreground">Name</span>
              <Input
                autoFocus
                className="h-8 text-xs"
                value={newPluginName}
                onChange={(event) => {
                  setNewPluginName(event.target.value);
                  setCreatePluginError(null);
                }}
                placeholder="SQLite extractor"
              />
            </div>
            {createPluginError ? (
              <p className="text-xs text-destructive">{createPluginError}</p>
            ) : null}
            <DialogFooter className="gap-2">
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="h-8 text-xs"
                onClick={() => {
                  setIsCreatePluginOpen(false);
                }}
              >
                Cancel
              </Button>
              <Button
                type="submit"
                size="sm"
                className="h-8 text-xs"
                disabled={isCreatingPlugin || newPluginName.trim().length === 0}
              >
                {isCreatingPlugin ? "Creating..." : "Create"}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </>
  );
}
