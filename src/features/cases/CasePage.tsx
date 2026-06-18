import { useMemo, useState, type FormEvent } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  AlertCircle,
  CheckCircle2,
  Database,
  FilePlus2,
  FolderOpen,
  RefreshCw,
} from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Separator } from "@/components/ui/separator";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { useCases } from "@/features/cases/case-provider";
import type { CreateCaseInput } from "@/features/cases/types";

const emptyCaseForm: CreateCaseInput = {
  name: "",
  examiner: "",
  reference: "",
  description: "",
  parentDirectory: "",
};

function formatDateTime(value: string) {
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "short",
    timeStyle: "short",
  }).format(new Date(value));
}

export function CasePage() {
  const {
    activeCase,
    cases,
    createCase,
    error,
    isLoading,
    refreshCases,
    selectCase,
  } = useCases();
  const [form, setForm] = useState<CreateCaseInput>(emptyCaseForm);
  const [formError, setFormError] = useState<string | null>(null);
  const [isCreating, setIsCreating] = useState(false);
  const canCreate =
    form.name.trim().length > 0 && form.parentDirectory.length > 0 && !isCreating;
  const statusText = useMemo(() => {
    if (isLoading) {
      return "Loading cases";
    }

    if (activeCase) {
      return `Active: ${activeCase.name}`;
    }

    return "No active case";
  }, [activeCase, isLoading]);

  function updateFormField(field: keyof CreateCaseInput, value: string) {
    setForm((currentForm) => ({
      ...currentForm,
      [field]: value,
    }));
  }

  async function handleCreateCase(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setFormError(null);

    if (!form.name.trim()) {
      setFormError("Case name is required.");
      return;
    }

    if (!form.parentDirectory) {
      setFormError("Choose where to create the case.");
      return;
    }

    setIsCreating(true);

    try {
      await createCase(form);
      setForm(emptyCaseForm);
    } catch (caughtError) {
      setFormError(
        caughtError instanceof Error ? caughtError.message : String(caughtError),
      );
    } finally {
      setIsCreating(false);
    }
  }

  async function chooseCaseLocation() {
    const selectedPath = await open({
      directory: true,
      multiple: false,
      title: "Choose Case Location",
    });

    if (typeof selectedPath !== "string") {
      return;
    }

    updateFormField("parentDirectory", selectedPath);
  }

  return (
    <div className="flex h-full min-h-0 flex-col bg-background">
      <section className="flex h-9 shrink-0 items-center gap-2 border-b px-2">
        <div className="flex min-w-0 items-center gap-2">
          <Database className="size-3.5 text-muted-foreground" aria-hidden="true" />
          <h1 className="text-sm font-semibold">Case</h1>
          <Badge variant="outline" className="h-5 rounded-sm text-[11px]">
            SQLite persistence
          </Badge>
        </div>
        <Separator orientation="vertical" className="h-5" />
        <div className="min-w-0 truncate text-[11px] text-muted-foreground">
          {statusText}
        </div>
        <Button
          type="button"
          variant="outline"
          size="xs"
          className="ml-auto h-7 rounded-sm px-2 text-xs"
          disabled={isLoading}
          onClick={() => {
            void refreshCases();
          }}
        >
          <RefreshCw className="size-3.5" aria-hidden="true" />
          Refresh
        </Button>
      </section>

      {(error || formError) && (
        <section className="flex h-8 shrink-0 items-center gap-2 border-b px-2 text-xs text-destructive">
          <AlertCircle className="size-3.5" aria-hidden="true" />
          <span className="truncate">{formError ?? error}</span>
        </section>
      )}

      <section className="grid min-h-0 flex-1 grid-cols-[20rem_minmax(0,1fr)] overflow-hidden">
        <form
          className="flex min-h-0 flex-col border-r"
          onSubmit={(event) => {
            void handleCreateCase(event);
          }}
        >
          <div className="flex h-8 shrink-0 items-center justify-between border-b px-2">
            <div className="text-xs font-medium uppercase text-muted-foreground">
              New Case
            </div>
            <FilePlus2 className="size-3.5 text-muted-foreground" aria-hidden="true" />
          </div>
          <div className="space-y-2 p-2">
            <label className="grid gap-1 text-xs">
              <span className="text-muted-foreground">Case name</span>
              <Input
                className="h-8 text-xs"
                value={form.name}
                placeholder="Required"
                onChange={(event) => updateFormField("name", event.target.value)}
              />
            </label>
            <label className="grid gap-1 text-xs">
              <span className="text-muted-foreground">Examiner</span>
              <Input
                className="h-8 text-xs"
                value={form.examiner}
                onChange={(event) =>
                  updateFormField("examiner", event.target.value)
                }
              />
            </label>
            <label className="grid gap-1 text-xs">
              <span className="text-muted-foreground">Reference</span>
              <Input
                className="h-8 text-xs"
                value={form.reference}
                placeholder="Ticket, warrant, or lab ID"
                onChange={(event) =>
                  updateFormField("reference", event.target.value)
                }
              />
            </label>
            <label className="grid gap-1 text-xs">
              <span className="text-muted-foreground">Description</span>
              <Input
                className="h-8 text-xs"
                value={form.description}
                onChange={(event) =>
                  updateFormField("description", event.target.value)
                }
              />
            </label>
            <label className="grid gap-1 text-xs">
              <span className="text-muted-foreground">Create in</span>
              <div className="flex min-w-0 gap-1">
                <Input
                  className="h-8 min-w-0 flex-1 text-xs"
                  value={form.parentDirectory}
                  placeholder="Choose parent folder"
                  readOnly
                />
                <Button
                  type="button"
                  variant="outline"
                  size="icon"
                  className="size-8 rounded-sm"
                  aria-label="Choose case location"
                  onClick={() => {
                    void chooseCaseLocation();
                  }}
                >
                  <FolderOpen className="size-3.5" aria-hidden="true" />
                </Button>
              </div>
            </label>
          </div>
          <div className="mt-auto flex h-10 shrink-0 items-center justify-end border-t px-2">
            <Button
              type="submit"
              size="xs"
              className="h-7 rounded-sm px-2 text-xs"
              disabled={!canCreate}
            >
              <FilePlus2 className="size-3.5" aria-hidden="true" />
              {isCreating ? "Creating..." : "Create case"}
            </Button>
          </div>
        </form>

        <section className="flex min-h-0 min-w-0 flex-col">
          <div className="flex h-8 shrink-0 items-center justify-between border-b px-2">
            <div className="text-xs font-medium uppercase text-muted-foreground">
              Cases
            </div>
            <Badge variant="secondary" className="h-5 rounded-sm text-[11px]">
              {cases.length} stored
            </Badge>
          </div>
          <Table
            containerClassName="min-h-0 flex-1 overflow-auto"
            className="min-w-[1020px] table-fixed text-xs"
          >
            <TableHeader className="sticky top-0 z-10 bg-muted">
              <TableRow className="hover:bg-muted">
                <TableHead className="h-7 w-[260px] px-2 text-xs">Name</TableHead>
                <TableHead className="h-7 w-[160px] px-2 text-xs">
                  Reference
                </TableHead>
                <TableHead className="h-7 w-[140px] px-2 text-xs">
                  Examiner
                </TableHead>
                <TableHead className="h-7 w-[160px] px-2 text-xs">
                  Updated
                </TableHead>
                <TableHead className="h-7 w-[260px] px-2 text-xs">
                  Location
                </TableHead>
                <TableHead className="h-7 w-[80px] px-2 text-xs">
                  State
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {cases.map((caseRecord) => {
                const isActive = activeCase?.id === caseRecord.id;

                return (
                  <TableRow
                    key={caseRecord.id}
                    data-state={isActive ? "selected" : undefined}
                    className="h-8 cursor-default"
                    onClick={() => selectCase(caseRecord.id)}
                  >
                    <TableCell className="h-8 truncate px-2 py-0 font-medium">
                      {caseRecord.name}
                    </TableCell>
                    <TableCell className="h-8 truncate px-2 py-0 text-muted-foreground">
                      {caseRecord.reference || "-"}
                    </TableCell>
                    <TableCell className="h-8 truncate px-2 py-0">
                      {caseRecord.examiner || "-"}
                    </TableCell>
                    <TableCell className="h-8 px-2 py-0 text-muted-foreground">
                      {formatDateTime(caseRecord.updatedAt)}
                    </TableCell>
                    <TableCell className="h-8 truncate px-2 py-0 text-muted-foreground">
                      {caseRecord.folderPath}
                    </TableCell>
                    <TableCell className="h-8 px-2 py-0">
                      {isActive && (
                        <span className="inline-flex items-center gap-1 text-[11px]">
                          <CheckCircle2 className="size-3 text-emerald-600" />
                          Active
                        </span>
                      )}
                    </TableCell>
                  </TableRow>
                );
              })}
              {cases.length === 0 && (
                <TableRow>
                  <TableCell
                    colSpan={6}
                    className="h-24 text-center text-xs text-muted-foreground"
                  >
                    {isLoading ? "Loading cases..." : "No cases created yet."}
                  </TableCell>
                </TableRow>
              )}
            </TableBody>
          </Table>
        </section>
      </section>
    </div>
  );
}
