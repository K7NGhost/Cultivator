import { useMemo, useState, type ReactNode } from "react";
import { Button } from "@/components/ui/button";
import type { StoredArtifactRecord } from "../model/types";
import type { CustomTableView } from "../utils/customTableGrouping";
import { detectCoordinatePairs } from "../utils/artifactLocations";
import { detectMessageColumns } from "../utils/artifactMessages";
import { ArtifactLocationPreview } from "./ArtifactLocationPreview";
import { ArtifactMessagePreview } from "./ArtifactMessagePreview";

export function ArtifactDataPreview({ table, sourceArtifacts, fallback, pageOnly = false, selectedRowIndex, onSelectRow, selectedArtifactId, onSelectArtifactId }: {
  table: CustomTableView; sourceArtifacts: StoredArtifactRecord[]; fallback: ReactNode; pageOnly?: boolean;
  selectedRowIndex?: number; onSelectRow?: (index: number) => void; selectedArtifactId?: string; onSelectArtifactId?: (id: string) => void;
}) {
  const messages = useMemo(() => detectMessageColumns(table.columns), [table.columns]);
  const locations = useMemo(() => detectCoordinatePairs(table.columns).length > 0, [table.columns]);
  const [preferred, setPreferred] = useState<"messages" | "locations">("messages");
  if (!messages && !locations) return <>{fallback}</>;
  const kind = messages && (!locations || preferred === "messages") ? "messages" : "locations";
  return <div className="flex h-full min-h-0 flex-1 flex-col">
    {messages && locations && <div className="flex shrink-0 gap-1 border-b px-2 py-1 text-xs" aria-label="Artifact preview type">
      <Button size="xs" variant={kind === "messages" ? "secondary" : "ghost"} aria-pressed={kind === "messages"} onClick={() => setPreferred("messages")}>Messages</Button>
      <Button size="xs" variant={kind === "locations" ? "secondary" : "ghost"} aria-pressed={kind === "locations"} onClick={() => setPreferred("locations")}>Locations</Button>
    </div>}
    {kind === "messages" && messages ? <ArtifactMessagePreview table={table} columns={messages} sourceArtifacts={sourceArtifacts} selectedRowIndex={selectedRowIndex} onSelectRow={onSelectRow} onSelectArtifactId={onSelectArtifactId} pageOnly={pageOnly} />
      : <><div className="shrink-0 px-2 py-1 text-[11px] text-muted-foreground">{pageOnly ? "Locations on this page" : "Locations"}</div><ArtifactLocationPreview table={table} selectedArtifactId={selectedArtifactId} onSelectArtifactId={onSelectArtifactId} /></>}
  </div>;
}
