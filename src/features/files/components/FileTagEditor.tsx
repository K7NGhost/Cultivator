import { useEffect, useState } from "react";
import { Tag, X } from "lucide-react";
import { toast } from "react-toastify";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import type { EvidenceDirectoryEntry } from "@/features/evidence/evidence-provider";
import {
  listFileTagsForPaths,
  removeFileTag,
  upsertFileTag,
  type FileTagGroup,
  type FileTagRecord,
} from "@/features/files/fileTagRepository";
import { normalizePath } from "@/features/cases/caseDatabase";

/** Same names as the Files page's tag menu, so both show the same tags. */
const QUICK_TAGS: Array<{ name: string; group: FileTagGroup }> = [
  { name: "Bookmark", group: "bookmark" },
  { name: "Follow Up", group: "follow-up" },
  { name: "Notable Item (Notable)", group: "notable" },
];

/**
 * The examiner's tags and notes on one file, stored with the case (the same
 * tags the Files page shows and lists under Tags).
 */
export function FileTagEditor({
  caseDatabasePath,
  entry,
}: {
  caseDatabasePath: string | null;
  entry: EvidenceDirectoryEntry;
}) {
  const [tags, setTags] = useState<FileTagRecord[]>([]);
  const [tagName, setTagName] = useState("");
  const [note, setNote] = useState("");
  const [refreshKey, setRefreshKey] = useState(0);

  useEffect(() => {
    if (!caseDatabasePath) {
      setTags([]);
      return;
    }
    let isCurrent = true;
    listFileTagsForPaths(caseDatabasePath, [entry.path])
      .then((byPath) => {
        if (isCurrent) {
          setTags(byPath[normalizePath(entry.path)] ?? []);
        }
      })
      .catch((error) => toast.error(errorText(error)));
    return () => {
      isCurrent = false;
    };
  }, [caseDatabasePath, entry.path, refreshKey]);

  async function add(name: string, group: FileTagGroup, comment = "") {
    if (!caseDatabasePath) {
      return;
    }
    try {
      await upsertFileTag({ caseDatabasePath, entry, tagName: name, tagGroup: group, comment });
      setRefreshKey((key) => key + 1);
    } catch (error) {
      toast.error(errorText(error));
    }
  }

  async function remove(name: string) {
    if (!caseDatabasePath) {
      return;
    }
    try {
      await removeFileTag({ caseDatabasePath, filePath: entry.path, tagName: name });
      setRefreshKey((key) => key + 1);
    } catch (error) {
      toast.error(errorText(error));
    }
  }

  if (!caseDatabasePath) {
    return null;
  }

  return (
    <div className="grid gap-1.5 border-b pb-2">
      <div className="flex items-center gap-1 text-[11px] text-muted-foreground">
        <Tag className="size-3" aria-hidden="true" />
        Examiner tags
      </div>
      {tags.length > 0 && (
        <div className="flex flex-wrap gap-1">
          {tags.map((tag) => (
            <Badge
              key={tag.tagName}
              variant="secondary"
              className="h-5 gap-1 rounded-sm px-1.5 text-[10px]"
              title={tag.comment || tag.tagName}
            >
              {tag.tagName}
              {tag.comment && <span className="text-muted-foreground">: {tag.comment}</span>}
              <button
                type="button"
                className="opacity-60 hover:opacity-100"
                aria-label={`Remove tag ${tag.tagName}`}
                onClick={() => void remove(tag.tagName)}
              >
                <X className="size-3" />
              </button>
            </Badge>
          ))}
        </div>
      )}
      <div className="flex flex-wrap gap-1">
        {QUICK_TAGS.filter((quick) => !tags.some((tag) => tag.tagName === quick.name)).map(
          (quick) => (
            <Button
              key={quick.name}
              type="button"
              variant="outline"
              size="xs"
              className="h-6 rounded-sm px-1.5 text-[11px]"
              onClick={() => void add(quick.name, quick.group)}
            >
              + {quick.name.replace(" (Notable)", "")}
            </Button>
          ),
        )}
      </div>
      <form
        className="grid gap-1"
        onSubmit={(event) => {
          event.preventDefault();
          if (tagName.trim()) {
            void add(tagName, "custom", note).then(() => {
              setTagName("");
              setNote("");
            });
          }
        }}
      >
        <Input
          value={tagName}
          onChange={(event) => setTagName(event.target.value)}
          placeholder="Custom tag"
          className="h-6 rounded-sm text-[11px]"
          aria-label="Custom tag"
        />
        <Input
          value={note}
          onChange={(event) => setNote(event.target.value)}
          placeholder="Note (optional)"
          className="h-6 rounded-sm text-[11px]"
          aria-label="Tag note"
        />
        <Button
          type="submit"
          size="xs"
          className="h-6 rounded-sm px-1.5 text-[11px]"
          disabled={!tagName.trim()}
        >
          Add tag
        </Button>
      </form>
    </div>
  );
}

function errorText(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}
