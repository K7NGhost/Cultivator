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
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useCases } from "@/features/cases/case-provider";
import { DataSourceWizardDialog } from "@/features/datasources/DataSourceWizardDialog";
import { useEvidence } from "@/features/evidence/evidence-provider";
import {
  getStoredCreatePluginMode,
  storeCreatePluginMode,
} from "@/features/plugins/createPluginPreferences";
import {
  isSafePluginFolderName,
  isSafePluginOrganizationPath,
} from "@/features/plugins/pluginManifest";
import {
  createPythonPlugin,
  openPythonApiGuide,
  openPythonPluginFolder,
  openPythonPluginFolderInVscode,
} from "@/features/plugins/pluginRepository";

const triggerClassName = "px-1.5 py-0.5 text-xs";
const itemClassName = "py-1 text-xs";

type CreatePluginMode = "manual" | "automatic";
type CreatePluginTarget =
  | "ios"
  | "android"
  | "windows"
  | "macos"
  | "infotainment"
  | "other";
type CreatePluginRunMode = "each_file" | "path_glob" | "path_regex";

const createPluginTargets: Array<{ value: CreatePluginTarget; label: string }> = [
  { value: "infotainment", label: "Infotainment" },
  { value: "android", label: "Android" },
  { value: "ios", label: "iOS" },
  { value: "windows", label: "Windows" },
  { value: "macos", label: "macOS" },
  { value: "other", label: "Other" },
];

const createPluginRunModes: Array<{
  value: CreatePluginRunMode;
  label: string;
}> = [
  { value: "each_file", label: "Each file" },
  { value: "path_glob", label: "Path glob" },
  { value: "path_regex", label: "Path regex" },
];

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
  const [createPluginMode, setCreatePluginMode] = useState<CreatePluginMode>(
    getStoredCreatePluginMode,
  );
  const [newPluginId, setNewPluginId] = useState("");
  const [newPluginName, setNewPluginName] = useState("");
  const [newPluginOrganizationFolder, setNewPluginOrganizationFolder] =
    useState("");
  const [newPluginAuthor, setNewPluginAuthor] = useState("");
  const [newPluginVersion, setNewPluginVersion] = useState("1.0.0");
  const [newPluginDescription, setNewPluginDescription] = useState("");
  const [newPluginType, setNewPluginType] = useState("other");
  const [newPluginTarget, setNewPluginTarget] =
    useState<CreatePluginTarget>("infotainment");
  const [newPluginRunMode, setNewPluginRunMode] =
    useState<CreatePluginRunMode>("each_file");
  const [newPluginPathGlob, setNewPluginPathGlob] = useState("");
  const [newPluginPathRegex, setNewPluginPathRegex] = useState("");
  const [newPluginEntry, setNewPluginEntry] = useState("plugin.py");
  const [newPluginFunction, setNewPluginFunction] = useState("run");
  const [newPluginTomlDetails, setNewPluginTomlDetails] = useState("");
  const [createPluginError, setCreatePluginError] = useState<string | null>(null);
  const [isCreatingPlugin, setIsCreatingPlugin] = useState(false);

  function resetCreatePluginForm() {
    setNewPluginId("");
    setNewPluginName("");
    setNewPluginOrganizationFolder("");
    setNewPluginAuthor("");
    setNewPluginVersion("1.0.0");
    setNewPluginDescription("");
    setNewPluginType("other");
    setNewPluginTarget("infotainment");
    setNewPluginRunMode("each_file");
    setNewPluginPathGlob("");
    setNewPluginPathRegex("");
    setNewPluginEntry("plugin.py");
    setNewPluginFunction("run");
    setNewPluginTomlDetails("");
    setCreatePluginError(null);
  }

  function handleCreatePluginModeChange(value: string) {
    const nextMode = value === "automatic" ? "automatic" : "manual";

    setCreatePluginMode(nextMode);
    storeCreatePluginMode(nextMode);
    setCreatePluginError(null);
  }

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
    const pluginName = newPluginName.trim();
    const pluginId = newPluginId.trim();
    const pluginType = newPluginType.trim();
    const pluginEntry = newPluginEntry.trim();
    const pluginFunction = newPluginFunction.trim();
    const pluginTomlDetails = newPluginTomlDetails.trim();
    const organizationFolder = newPluginOrganizationFolder.trim();

    if (!isSafePluginOrganizationPath(organizationFolder)) {
      setCreatePluginError(
        "Organization folders must be relative paths using letters, numbers, spaces, '.', '_', and '-'.",
      );
      return;
    }

    if (createPluginMode === "manual") {
      if (
        !pluginId ||
        !pluginName ||
        !pluginType ||
        !pluginEntry ||
        !pluginFunction
      ) {
        setCreatePluginError(
          "Plugin id, name, type, entry, and function are required.",
        );
        return;
      }

      if (newPluginRunMode === "path_glob" && !newPluginPathGlob.trim()) {
        setCreatePluginError("Path glob mode requires at least one path glob.");
        return;
      }

      if (newPluginRunMode === "path_regex" && !newPluginPathRegex.trim()) {
        setCreatePluginError("Path regex mode requires a path regex.");
        return;
      }
    } else {
      if (!pluginName) {
        setCreatePluginError("Plugin folder name is required.");
        return;
      }

      if (!isSafePluginFolderName(pluginName)) {
        setCreatePluginError(
          "Plugin folder name may only contain letters, numbers, '.', '_', and '-'.",
        );
        return;
      }

      if (!pluginTomlDetails) {
        setCreatePluginError("Plugin TOML details are required.");
        return;
      }
    }

    setIsCreatingPlugin(true);
    setCreatePluginError(null);

    try {
      await createPythonPlugin(
        createPluginMode === "manual"
          ? {
              organizationFolder: organizationFolder || undefined,
              manifest: {
                id: pluginId,
                name: pluginName,
                author: newPluginAuthor.trim() || "Unknown",
                version: newPluginVersion.trim() || "1.0.0",
                description: newPluginDescription.trim(),
                type: pluginType,
                target: newPluginTarget,
                mode: newPluginRunMode,
                pathGlob: newPluginPathGlob
                  .split(/\r?\n|,/)
                  .map((pathGlob) => pathGlob.trim())
                  .filter(Boolean),
                pathRegex: newPluginPathRegex.trim() || undefined,
                entry: pluginEntry,
                function: pluginFunction,
              },
            }
          : {
              folderName: pluginName,
              organizationFolder: organizationFolder || undefined,
              manifestToml: pluginTomlDetails,
            },
      );
      resetCreatePluginForm();
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
      <Dialog
        open={isCreatePluginOpen}
        onOpenChange={(isOpen) => {
          setIsCreatePluginOpen(isOpen);

          if (!isOpen) {
            resetCreatePluginForm();
          }
        }}
      >
        <DialogContent className="w-[calc(100vw-2rem)] max-w-3xl rounded-sm p-0">
          <DialogHeader className="border-b px-3 py-2">
            <DialogTitle className="text-sm">Create Python Plugin</DialogTitle>
            <DialogDescription className="text-xs">
              Create plugin.py and plugin.toml from manual fields or a folder name plus pasted TOML.
            </DialogDescription>
          </DialogHeader>
          <form onSubmit={handleCreatePlugin}>
            <div className="max-h-[min(34rem,calc(100vh-12rem))] overflow-auto px-3 py-3">
              <label className="mb-3 block space-y-1 text-xs">
                <span className="text-muted-foreground">
                  Organization folder (optional)
                </span>
                <Input
                  className="h-8 rounded-sm font-mono text-xs"
                  value={newPluginOrganizationFolder}
                  placeholder="VLEAPP/Ford"
                  onChange={(event) => {
                    setNewPluginOrganizationFolder(event.target.value);
                    setCreatePluginError(null);
                  }}
                />
                <span className="block text-[10px] text-muted-foreground">
                  Nested folders are created automatically.
                </span>
              </label>
              <Tabs
                value={createPluginMode}
                onValueChange={handleCreatePluginModeChange}
              >
                <TabsList className="h-8 rounded-sm">
                  <TabsTrigger value="manual" className="h-7 rounded-sm text-xs">
                    Manual Entry
                  </TabsTrigger>
                  <TabsTrigger
                    value="automatic"
                    className="h-7 rounded-sm text-xs"
                  >
                    Automatic Entry
                  </TabsTrigger>
                </TabsList>

                <TabsContent value="manual" className="mt-3 space-y-3">
                  <div className="grid gap-2 sm:grid-cols-2">
                    <label className="space-y-1 text-xs">
                      <span className="text-muted-foreground">Plugin ID</span>
                      <Input
                        autoFocus
                        className="h-8 rounded-sm text-xs"
                        value={newPluginId}
                        placeholder="ford-phonebook"
                        onChange={(event) => {
                          setNewPluginId(event.target.value);
                          setCreatePluginError(null);
                        }}
                      />
                    </label>
                    <label className="space-y-1 text-xs">
                      <span className="text-muted-foreground">Name</span>
                      <Input
                        className="h-8 rounded-sm text-xs"
                        value={newPluginName}
                        placeholder="Ford Phonebook"
                        onChange={(event) => {
                          setNewPluginName(event.target.value);
                          setCreatePluginError(null);
                        }}
                      />
                    </label>
                    <label className="space-y-1 text-xs sm:col-span-2">
                      <span className="text-muted-foreground">Description</span>
                      <Input
                        className="h-8 rounded-sm text-xs"
                        value={newPluginDescription}
                        placeholder="Extracts infotainment phonebook records."
                        onChange={(event) =>
                          setNewPluginDescription(event.target.value)
                        }
                      />
                    </label>
                    <label className="space-y-1 text-xs">
                      <span className="text-muted-foreground">Author</span>
                      <Input
                        className="h-8 rounded-sm text-xs"
                        value={newPluginAuthor}
                        placeholder="Your Name"
                        onChange={(event) => {
                          setNewPluginAuthor(event.target.value);
                          setCreatePluginError(null);
                        }}
                      />
                    </label>
                    <label className="space-y-1 text-xs">
                      <span className="text-muted-foreground">Version</span>
                      <Input
                        className="h-8 rounded-sm text-xs"
                        value={newPluginVersion}
                        placeholder="1.0.0"
                        onChange={(event) => {
                          setNewPluginVersion(event.target.value);
                          setCreatePluginError(null);
                        }}
                      />
                    </label>
                    <label className="space-y-1 text-xs">
                      <span className="text-muted-foreground">Type</span>
                      <Input
                        className="h-8 rounded-sm text-xs"
                        value={newPluginType}
                        placeholder="contacts"
                        onChange={(event) => setNewPluginType(event.target.value)}
                      />
                    </label>
                    <label className="space-y-1 text-xs">
                      <span className="text-muted-foreground">Entry file</span>
                      <Input
                        className="h-8 rounded-sm text-xs"
                        value={newPluginEntry}
                        placeholder="plugin.py"
                        onChange={(event) => setNewPluginEntry(event.target.value)}
                      />
                    </label>
                    <label className="space-y-1 text-xs">
                      <span className="text-muted-foreground">Function</span>
                      <Input
                        className="h-8 rounded-sm text-xs"
                        value={newPluginFunction}
                        placeholder="run"
                        onChange={(event) =>
                          setNewPluginFunction(event.target.value)
                        }
                      />
                    </label>
                  </div>

                  <div className="space-y-1">
                    <div className="text-xs text-muted-foreground">Target</div>
                    <div className="flex flex-wrap gap-1">
                      {createPluginTargets.map((target) => (
                        <Button
                          key={target.value}
                          type="button"
                          variant={
                            newPluginTarget === target.value
                              ? "secondary"
                              : "outline"
                          }
                          size="xs"
                          className="h-7 rounded-sm px-2 text-[11px]"
                          onClick={() => setNewPluginTarget(target.value)}
                        >
                          {target.label}
                        </Button>
                      ))}
                    </div>
                  </div>

                  <div className="space-y-1">
                    <div className="text-xs text-muted-foreground">Mode</div>
                    <div className="flex flex-wrap gap-1">
                      {createPluginRunModes.map((mode) => (
                        <Button
                          key={mode.value}
                          type="button"
                          variant={
                            newPluginRunMode === mode.value
                              ? "secondary"
                              : "outline"
                          }
                          size="xs"
                          className="h-7 rounded-sm px-2 text-[11px]"
                          onClick={() => setNewPluginRunMode(mode.value)}
                        >
                          {mode.label}
                        </Button>
                      ))}
                    </div>
                  </div>

                  <div className="grid gap-2 sm:grid-cols-2">
                    <label className="space-y-1 text-xs">
                      <span className="text-muted-foreground">Path glob</span>
                      <textarea
                        className="min-h-20 w-full resize-y rounded-sm border bg-transparent px-2 py-1.5 font-mono text-xs outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50"
                        value={newPluginPathGlob}
                        placeholder={"*/phonebook.db\n*/contacts*.json"}
                        onChange={(event) =>
                          setNewPluginPathGlob(event.target.value)
                        }
                      />
                    </label>
                    <label className="space-y-1 text-xs">
                      <span className="text-muted-foreground">Path regex</span>
                      <textarea
                        className="min-h-20 w-full resize-y rounded-sm border bg-transparent px-2 py-1.5 font-mono text-xs outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50"
                        value={newPluginPathRegex}
                        placeholder="(?i).*phonebook.*"
                        onChange={(event) =>
                          setNewPluginPathRegex(event.target.value)
                        }
                      />
                    </label>
                  </div>
                </TabsContent>

                <TabsContent value="automatic" className="mt-3 space-y-2">
                  <label className="grid grid-cols-[7rem_minmax(0,1fr)] items-center gap-2 text-xs">
                    <span className="text-muted-foreground">Folder name</span>
                    <Input
                      className="h-8 rounded-sm text-xs"
                      value={newPluginName}
                      onChange={(event) => {
                        setNewPluginName(event.target.value);
                        setCreatePluginError(null);
                      }}
                    />
                  </label>
                  <label className="space-y-1 text-xs">
                    <span className="text-muted-foreground">TOML Details</span>
                    <textarea
                      className="min-h-72 w-full resize-y rounded-sm border bg-transparent px-2 py-1.5 font-mono text-xs outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50"
                      value={newPluginTomlDetails}
                      onChange={(event) => {
                        setNewPluginTomlDetails(event.target.value);
                        setCreatePluginError(null);
                      }}
                      spellCheck={false}
                    />
                  </label>
                </TabsContent>
              </Tabs>
            </div>
            {createPluginError ? (
              <div className="border-t px-3 py-2 text-xs text-destructive">
                {createPluginError}
              </div>
            ) : null}
            <DialogFooter className="border-t px-3 py-2">
              <Button
                type="button"
                variant="outline"
                size="xs"
                className="h-7 rounded-sm px-2 text-xs"
                disabled={isCreatingPlugin}
                onClick={() => {
                  setIsCreatePluginOpen(false);
                  resetCreatePluginForm();
                }}
              >
                Cancel
              </Button>
              <Button
                type="submit"
                size="xs"
                className="h-7 rounded-sm px-2 text-xs"
                disabled={isCreatingPlugin}
              >
                {isCreatingPlugin ? "Creating" : "Create"}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </>
  );
}
