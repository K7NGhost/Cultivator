import { FileCode2 } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import type { FileMatch, SearchMatch } from "../model/types";

type SearchFileMatchListProps = {
  fileMatches: FileMatch[];
  isSearching: boolean;
  onSelectMatch: (match: SearchMatch) => void;
  selectedPath: string | null;
};

const FILE_MATCH_HEADER_HEIGHT = 28;
const FILE_MATCH_ROW_HEIGHT = 32;
const FILE_MATCH_GRID_COLUMNS = "minmax(0,1fr) 4.75rem 4.25rem 4.5rem";

export function SearchFileMatchList({
  fileMatches,
  isSearching,
  onSelectMatch,
  selectedPath,
}: SearchFileMatchListProps) {
  return (
    <div
      className="grid h-full min-h-0 w-full overflow-hidden text-xs"
      style={{
        gridTemplateRows: `${FILE_MATCH_HEADER_HEIGHT}px minmax(0, 1fr)`,
      }}
    >
      <div
        className="grid w-full border-b bg-muted text-[11px] font-medium text-muted-foreground"
        style={{ gridTemplateColumns: FILE_MATCH_GRID_COLUMNS }}
      >
        <div className="flex h-7 min-w-0 items-center px-2">File</div>
        <div className="flex h-7 min-w-0 items-center px-2">Lines</div>
        <div className="flex h-7 min-w-0 items-center px-2">Type</div>
        <div className="flex h-7 min-w-0 items-center px-2">Action</div>
      </div>

      <div className="min-h-0 w-full overflow-auto">
        {fileMatches.length > 0 ? (
          <div className="min-w-full">
            {fileMatches.map(({ match, matchedLines }) => {
              const isSelected = selectedPath === match.path;

              return (
                <div
                  key={match.path}
                  className={cn(
                    "grid cursor-default border-b text-xs",
                    isSelected && "bg-muted",
                  )}
                  style={{
                    gridTemplateColumns: FILE_MATCH_GRID_COLUMNS,
                    minHeight: FILE_MATCH_ROW_HEIGHT,
                  }}
                  onClick={() => onSelectMatch(match)}
                  onDoubleClick={() => onSelectMatch(match)}
                >
                  <div className="min-w-0 px-2 py-1">
                    <div className="flex min-w-0 items-start gap-1.5">
                      <FileCode2
                        className="mt-0.5 size-3.5 shrink-0 text-muted-foreground"
                        aria-hidden="true"
                      />
                      <div className="min-w-0">
                        <div className="truncate">{match.file}</div>
                        <div className="truncate font-mono text-[10px] text-muted-foreground">
                          {match.path}
                        </div>
                      </div>
                    </div>
                  </div>
                  <div className="flex min-w-0 items-center px-2 py-1">
                    {matchedLines.size.toLocaleString()}{" "}
                    {matchedLines.size === 1 ? "line" : "lines"}
                  </div>
                  <div className="flex min-w-0 items-center px-2 py-1">
                    <Badge
                      variant="outline"
                      className="h-5 max-w-full rounded-none px-1 text-[10px]"
                    >
                      <span className="truncate">{match.kind}</span>
                    </Badge>
                  </div>
                  <div className="flex min-w-0 items-center px-2 py-1">
                    <Button
                      type="button"
                      variant="ghost"
                      size="xs"
                      className="h-6 rounded-none px-1.5 text-[11px]"
                      onClick={(event) => {
                        event.stopPropagation();
                        onSelectMatch(match);
                      }}
                    >
                      Open
                    </Button>
                  </div>
                </div>
              );
            })}
          </div>
        ) : (
          <div className="flex h-24 items-center justify-center px-2 text-center text-xs text-muted-foreground">
            {isSearching ? "Searching..." : "Run search to show matches."}
          </div>
        )}
      </div>
    </div>
  );
}

