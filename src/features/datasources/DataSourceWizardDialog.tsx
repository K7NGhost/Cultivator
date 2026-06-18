import { open } from "@tauri-apps/plugin-dialog";
import { CheckCircle2, Files, FolderOpen, X } from "lucide-react";
import { useMemo, useState } from "react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Stepper,
  StepperContent,
  StepperIndicator,
  StepperItem,
  StepperNav,
  StepperPanel,
  StepperTrigger,
  StepperDescription,
  StepperSeparator,
  StepperTitle,
} from "@/components/reui/stepper";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Separator } from "@/components/ui/separator";
import type { CaseRecord } from "@/features/cases/types";
import { createDataSource } from "@/features/datasources/dataSourceRepository";
import { dataSourcePlugins } from "@/features/datasources/pluginCatalog";
import type {
  DataSourcePlugin,
  DataSourcePluginType,
  DataSourceType,
} from "@/features/datasources/types";
import { cn } from "@/lib/utils";

type DataSourceWizardDialogProps = {
  activeCase: CaseRecord | null;
  open: boolean;
  onOpenChange: (isOpen: boolean) => void;
};

type WizardStep = 1 | 2 | 3 | 4;

const dataSourceTypes: Array<{
  type: DataSourceType;
  label: string;
  description: string;
}> = [
  {
    type: "logicalFiles",
    label: "Logical files",
    description: "Add selected folders and files from an extracted source.",
  },
];

const pluginTypeFilters: Array<{
  type: DataSourcePluginType | "all";
  label: string;
}> = [
  { type: "all", label: "All" },
  { type: "android", label: "Android" },
  { type: "ios", label: "iOS" },
  { type: "windows", label: "Windows" },
  { type: "macos", label: "macOS" },
  { type: "infotainment", label: "Infotainment" },
  { type: "other", label: "Other" },
];

const steps: Array<{ step: WizardStep; title: string; description: string }> = [
  { step: 1, title: "Name", description: "Label the datasource" },
  { step: 2, title: "Type", description: "Choose the source type" },
  { step: 3, title: "Source", description: "Select folders and files" },
  { step: 4, title: "Plugins", description: "Queue parser plugins" },
];

function getDefaultPluginIds() {
  return ["file-metadata", "keyword-scanner", "string-extractor"];
}

function normalizeSelectedPaths(selectedPaths: string | string[] | null) {
  if (!selectedPaths) {
    return [];
  }

  return Array.isArray(selectedPaths) ? selectedPaths : [selectedPaths];
}

function getPathName(path: string) {
  const normalizedPath = path.replace(/\\/g, "/");
  const pathParts = normalizedPath.split("/").filter(Boolean);
  return pathParts[pathParts.length - 1] ?? normalizedPath;
}

function getPluginTypeLabel(type: DataSourcePluginType) {
  return (
    pluginTypeFilters.find((pluginTypeFilter) => pluginTypeFilter.type === type)
      ?.label ?? "Other"
  );
}

export function DataSourceWizardDialog({
  activeCase,
  open: isOpen,
  onOpenChange,
}: DataSourceWizardDialogProps) {
  const [step, setStep] = useState<WizardStep>(1);
  const [name, setName] = useState("");
  const [type, setType] = useState<DataSourceType>("logicalFiles");
  const [paths, setPaths] = useState<string[]>([]);
  const [pluginFilter, setPluginFilter] = useState("");
  const [pluginTypeFilter, setPluginTypeFilter] = useState<
    DataSourcePluginType | "all"
  >("all");
  const [activePluginId, setActivePluginId] = useState("file-metadata");
  const [selectedPluginIds, setSelectedPluginIds] = useState<string[]>(
    getDefaultPluginIds(),
  );
  const [error, setError] = useState<string | null>(null);
  const [isSaving, setIsSaving] = useState(false);
  const selectedPlugins = useMemo(() => {
    return dataSourcePlugins.filter((plugin) =>
      selectedPluginIds.includes(plugin.id),
    );
  }, [selectedPluginIds]);
  const visiblePlugins = useMemo(() => {
    const normalizedFilter = pluginFilter.trim().toLowerCase();

    return dataSourcePlugins.filter((plugin) => {
      if (pluginTypeFilter !== "all" && plugin.type !== pluginTypeFilter) {
        return false;
      }

      if (!normalizedFilter) {
        return true;
      }

      return (
        plugin.name.toLowerCase().includes(normalizedFilter) ||
        plugin.description.toLowerCase().includes(normalizedFilter) ||
        getPluginTypeLabel(plugin.type).toLowerCase().includes(normalizedFilter)
      );
    });
  }, [pluginFilter, pluginTypeFilter]);
  const activePlugin =
    dataSourcePlugins.find((plugin) => plugin.id === activePluginId) ??
    visiblePlugins[0] ??
    null;
  const canContinue =
    (step === 1 && name.trim().length > 0) ||
    step === 2 ||
    (step === 3 && paths.length > 0) ||
    step === 4;

  function resetWizard() {
    setStep(1);
    setName("");
    setType("logicalFiles");
    setPaths([]);
    setPluginFilter("");
    setPluginTypeFilter("all");
    setActivePluginId("file-metadata");
    setSelectedPluginIds(getDefaultPluginIds());
    setError(null);
    setIsSaving(false);
  }

  function updateType(nextType: DataSourceType) {
    setType(nextType);
    setSelectedPluginIds(getDefaultPluginIds());
  }

  function addSelectedPaths(nextPaths: string[]) {
    setPaths((currentPaths) =>
      Array.from(new Set([...currentPaths, ...nextPaths])),
    );
  }

  async function selectFiles() {
    const selectedPaths = await open({
      directory: false,
      multiple: true,
      title: "Choose files",
    });

    addSelectedPaths(normalizeSelectedPaths(selectedPaths));
  }

  async function selectFolders() {
    const selectedPaths = await open({
      directory: true,
      multiple: true,
      title: "Choose folders",
    });

    addSelectedPaths(normalizeSelectedPaths(selectedPaths));
  }

  function removePath(pathToRemove: string) {
    setPaths((currentPaths) =>
      currentPaths.filter((currentPath) => currentPath !== pathToRemove),
    );
  }

  function togglePlugin(plugin: DataSourcePlugin, isChecked: boolean) {
    setSelectedPluginIds((currentPluginIds) => {
      if (isChecked) {
        return Array.from(new Set([...currentPluginIds, plugin.id]));
      }

      return currentPluginIds.filter((pluginId) => pluginId !== plugin.id);
    });
  }

  async function finishWizard() {
    setError(null);

    if (!activeCase) {
      setError("Create or select a case before adding a datasource.");
      return;
    }

    if (!name.trim()) {
      setError("Datasource name is required.");
      setStep(1);
      return;
    }

    if (paths.length === 0) {
      setError("Select at least one datasource path.");
      setStep(3);
      return;
    }

    setIsSaving(true);

    try {
      await createDataSource({
        caseDatabasePath: activeCase.databasePath,
        caseId: activeCase.id,
        name,
        type,
        paths,
        plugins: selectedPlugins,
      });
      resetWizard();
      onOpenChange(false);
    } catch (caughtError) {
      setError(
        caughtError instanceof Error
          ? caughtError.message
          : String(caughtError),
      );
    } finally {
      setIsSaving(false);
    }
  }

  return (
    <Dialog
      open={isOpen}
      onOpenChange={(nextOpen) => {
        onOpenChange(nextOpen);

        if (!nextOpen) {
          resetWizard();
        }
      }}
    >
      <DialogContent className="max-w-[calc(100%-2rem)] gap-0 p-0 sm:max-w-5xl">
        <DialogHeader className="border-b px-3 py-2">
          <DialogTitle className="text-sm">Add Datasource</DialogTitle>
          <DialogDescription className="text-xs">
            {activeCase
              ? `Case: ${activeCase.name}`
              : "Create or select a case before adding a datasource."}
          </DialogDescription>
        </DialogHeader>

        <Stepper
          value={step}
          onValueChange={(value) => setStep(value as WizardStep)}
          orientation="vertical"
          className="grid h-[min(42rem,calc(100vh-8rem))] min-h-0 grid-cols-[18rem_minmax(0,1fr)]"
          indicators={{
            completed: <CheckCircle2 className="size-3.5" />,
          }}
        >
          <aside className="min-h-0 border-r bg-muted/30 p-4">
            <StepperNav className="w-full gap-2">
              {steps.map((wizardStep, index) => (
                <StepperItem
                  key={wizardStep.step}
                  step={wizardStep.step}
                  className="relative items-start justify-start"
                >
                  <StepperTrigger className="grid w-full grid-cols-[1.75rem_1fr] items-start gap-3 rounded-sm px-2 py-2.5 text-left hover:bg-background/70 data-[state=active]:bg-background data-[state=active]:shadow-sm">
                    <StepperIndicator className="size-7 text-xs">
                      {wizardStep.step}
                    </StepperIndicator>
                    <div className="min-w-0 pt-0.5">
                      <StepperTitle className="text-xs">
                        {wizardStep.title}
                      </StepperTitle>
                      <StepperDescription className="mt-1 text-[11px] leading-4">
                        {wizardStep.description}
                      </StepperDescription>
                    </div>
                  </StepperTrigger>
                  {index < steps.length - 1 && (
                    <StepperSeparator className="absolute left-[1.375rem] top-10 -order-1 m-0 h-4 -translate-x-1/2" />
                  )}
                </StepperItem>
              ))}
            </StepperNav>
          </aside>

          <section className="min-h-0 min-w-0 p-3">
            <div className="mb-3 flex items-center justify-between text-xs">
              <div className="font-medium">
                {steps.find((item) => item.step === step)?.title}
              </div>
              <div className="text-muted-foreground">
                Step {step} of {steps.length}
              </div>
            </div>

            {error && (
              <div className="mb-2 rounded-sm border border-destructive/40 bg-destructive/10 px-2 py-1.5 text-xs text-destructive">
                {error}
              </div>
            )}

            <StepperPanel>
              <StepperContent value={1}>
                <div className="space-y-2">
                  <div className="text-xs font-medium uppercase text-muted-foreground">
                    Datasource Name
                  </div>
                  <Input
                    className="h-8 text-xs"
                    autoFocus
                    value={name}
                    placeholder="Example: Samsung phone extraction"
                    onChange={(event) => setName(event.target.value)}
                  />
                </div>
              </StepperContent>

              <StepperContent value={2}>
                <div className="space-y-2">
                  <div className="text-xs font-medium uppercase text-muted-foreground">
                    Datasource Type
                  </div>
                  <div className="grid grid-cols-2 gap-2">
                    {dataSourceTypes.map((dataSourceType) => {
                      const isSelected = type === dataSourceType.type;

                      return (
                        <button
                          key={dataSourceType.type}
                          type="button"
                          className={cn(
                            "min-h-24 rounded-sm border p-2 text-left text-xs hover:bg-accent",
                            isSelected && "border-primary bg-accent",
                          )}
                          onClick={() => updateType(dataSourceType.type)}
                        >
                          <Files className="mb-2 size-4" aria-hidden="true" />
                          <div className="font-medium">
                            {dataSourceType.label}
                          </div>
                          <div className="mt-1 text-[11px] text-muted-foreground">
                            {dataSourceType.description}
                          </div>
                        </button>
                      );
                    })}
                  </div>
                </div>
              </StepperContent>

              <StepperContent value={3}>
                <div className="space-y-2">
                  <div className="text-xs font-medium uppercase text-muted-foreground">
                    Select Datasource
                  </div>
                  <div className="flex flex-wrap gap-1">
                    <Button
                      type="button"
                      variant="outline"
                      size="xs"
                      className="h-8 rounded-sm px-2 text-xs"
                      onClick={() => {
                        void selectFiles();
                      }}
                    >
                      <Files className="size-3.5" aria-hidden="true" />
                      Add files
                    </Button>
                    <Button
                      type="button"
                      variant="outline"
                      size="xs"
                      className="h-8 rounded-sm px-2 text-xs"
                      onClick={() => {
                        void selectFolders();
                      }}
                    >
                      <FolderOpen className="size-3.5" aria-hidden="true" />
                      Add folders
                    </Button>
                  </div>
                  <div className="h-48 overflow-auto rounded-sm border">
                    {paths.length > 0 ? (
                      <div className="divide-y">
                        {paths.map((sourcePath) => (
                          <div
                            key={sourcePath}
                            className="grid grid-cols-[1fr_auto] items-center gap-2 px-2 py-1.5 text-xs"
                          >
                            <div className="min-w-0">
                              <div className="truncate font-medium">
                                {getPathName(sourcePath)}
                              </div>
                              <div className="truncate text-[11px] text-muted-foreground">
                                {sourcePath}
                              </div>
                            </div>
                            <Button
                              type="button"
                              variant="ghost"
                              size="icon"
                              className="size-6 rounded-sm"
                              aria-label={`Remove ${getPathName(sourcePath)}`}
                              onClick={() => removePath(sourcePath)}
                            >
                              <X className="size-3.5" aria-hidden="true" />
                            </Button>
                          </div>
                        ))}
                      </div>
                    ) : (
                      <div className="grid h-full place-items-center px-3 text-center text-xs text-muted-foreground">
                        No folders or files selected.
                      </div>
                    )}
                  </div>
                </div>
              </StepperContent>

              <StepperContent value={4}>
                <div className="grid h-[32rem] min-h-0 grid-cols-[minmax(0,1fr)_18rem] gap-2">
                  <div className="flex min-h-0 min-w-0 flex-col gap-2">
                    <div className="flex items-center justify-between gap-2">
                      <div className="text-xs font-medium uppercase text-muted-foreground">
                        Plugins
                      </div>
                      <Badge
                        variant="secondary"
                        className="h-5 rounded-sm text-[11px]"
                      >
                        {selectedPlugins.length} selected
                      </Badge>
                    </div>
                    <Input
                      className="h-8 text-xs"
                      value={pluginFilter}
                      placeholder="Filter plugins"
                      onChange={(event) => setPluginFilter(event.target.value)}
                    />
                    <div className="flex flex-wrap gap-1">
                      {pluginTypeFilters.map((pluginType) => {
                        const isSelected =
                          pluginTypeFilter === pluginType.type;

                        return (
                          <Button
                            key={pluginType.type}
                            type="button"
                            variant={isSelected ? "secondary" : "outline"}
                            size="xs"
                            className="h-7 rounded-sm px-2 text-[11px]"
                            onClick={() => setPluginTypeFilter(pluginType.type)}
                          >
                            {pluginType.label}
                          </Button>
                        );
                      })}
                    </div>
                    <div className="min-h-0 flex-1 overflow-auto rounded-sm border">
                      {visiblePlugins.length > 0 ? (
                        <div className="divide-y">
                          {visiblePlugins.map((plugin) => {
                            const isSelected = selectedPluginIds.includes(
                              plugin.id,
                            );
                            const isActive = activePlugin?.id === plugin.id;

                            return (
                              <label
                                key={plugin.id}
                                className={cn(
                                  "grid cursor-pointer grid-cols-[auto_1fr] items-start gap-2 px-2 py-1.5 text-xs hover:bg-accent",
                                  isActive && "bg-accent",
                                )}
                                onClick={() => setActivePluginId(plugin.id)}
                              >
                                <Checkbox
                                  checked={isSelected}
                                  onCheckedChange={(checked) => {
                                    togglePlugin(plugin, checked === true);
                                  }}
                                />
                                <span className="min-w-0">
                                  <span className="block truncate font-medium">
                                    {plugin.name}
                                  </span>
                                  <span className="flex min-w-0 items-center gap-1">
                                    <Badge
                                      variant="secondary"
                                      className="h-4 shrink-0 rounded-sm px-1 text-[10px]"
                                    >
                                      {getPluginTypeLabel(plugin.type)}
                                    </Badge>
                                    <span className="block truncate text-[11px] text-muted-foreground">
                                      {plugin.description}
                                    </span>
                                  </span>
                                </span>
                              </label>
                            );
                          })}
                        </div>
                      ) : (
                        <div className="grid h-full place-items-center px-3 text-center text-xs text-muted-foreground">
                          No plugins match the filter.
                        </div>
                      )}
                    </div>
                  </div>

                  <div className="flex min-h-0 min-w-0 flex-col rounded-sm border">
                    <div className="border-b px-2 py-1.5">
                      <div className="text-xs font-medium uppercase text-muted-foreground">
                        Options
                      </div>
                    </div>
                    {activePlugin ? (
                      <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-auto p-2 text-xs">
                        <div className="min-w-0">
                          <div className="truncate font-medium">
                            {activePlugin.name}
                          </div>
                          <div className="mt-1 flex items-center gap-1">
                            <Badge
                              variant="secondary"
                              className="h-4 rounded-sm px-1 text-[10px]"
                            >
                              {getPluginTypeLabel(activePlugin.type)}
                            </Badge>
                            <span className="truncate text-[11px] text-muted-foreground">
                              {activePlugin.id}
                            </span>
                          </div>
                        </div>

                        <p className="text-[11px] text-muted-foreground">
                          {activePlugin.description}
                        </p>

                        <label className="flex items-center gap-2 rounded-sm border px-2 py-1.5">
                          <Checkbox
                            checked={selectedPluginIds.includes(
                              activePlugin.id,
                            )}
                            onCheckedChange={(checked) => {
                              togglePlugin(activePlugin, checked === true);
                            }}
                          />
                          <span>Run plugin</span>
                        </label>

                        <Separator />

                        <div className="grid grid-cols-[5rem_1fr] gap-x-2 gap-y-1">
                          <div className="text-muted-foreground">Case</div>
                          <div className="truncate">{name || "Untitled"}</div>
                          <div className="text-muted-foreground">Sources</div>
                          <div>{paths.length}</div>
                        </div>

                        <div className="mt-auto rounded-sm border border-dashed px-2 py-2 text-[11px] text-muted-foreground">
                          This plugin does not expose configurable options yet.
                        </div>
                      </div>
                    ) : (
                      <div className="grid flex-1 place-items-center px-3 text-center text-xs text-muted-foreground">
                        Select a plugin to view options.
                      </div>
                    )}
                  </div>
                </div>
              </StepperContent>
            </StepperPanel>
          </section>
        </Stepper>

        <DialogFooter className="border-t p-2">
          <Button
            type="button"
            variant="outline"
            size="xs"
            className="h-7 rounded-sm px-2 text-xs"
            disabled={step === 1 || isSaving}
            onClick={() =>
              setStep((currentStep) => (currentStep - 1) as WizardStep)
            }
          >
            Back
          </Button>
          {step < 4 ? (
            <Button
              type="button"
              size="xs"
              className="h-7 rounded-sm px-2 text-xs"
              disabled={!activeCase || !canContinue}
              onClick={() =>
                setStep((currentStep) => (currentStep + 1) as WizardStep)
              }
            >
              Next
            </Button>
          ) : (
            <Button
              type="button"
              size="xs"
              className="h-7 rounded-sm px-2 text-xs"
              disabled={!activeCase || isSaving}
              onClick={() => {
                void finishWizard();
              }}
            >
              <CheckCircle2 className="size-3.5" aria-hidden="true" />
              {isSaving ? "Adding..." : "Finish"}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
