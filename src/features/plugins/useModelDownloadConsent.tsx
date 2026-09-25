import { useRef, useState } from "react";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  getMediaTaggingStatus,
  getSemanticModelStatus,
  mediaTaggingPluginId,
  semanticIndexPluginId,
  type ModelStatus,
} from "@/features/plugins/pluginRepository";

type ModelPlugin = {
  pluginId: string;
  name: string;
  downloadSize: string;
  getStatus: () => Promise<ModelStatus>;
};

const MODEL_PLUGINS: ModelPlugin[] = [
  {
    pluginId: mediaTaggingPluginId,
    name: "Media Tagging (AI)",
    downloadSize: "~1.5 GB",
    getStatus: getMediaTaggingStatus,
  },
  {
    pluginId: semanticIndexPluginId,
    name: "Semantic Index (AI)",
    downloadSize: "~2.3 GB",
    getStatus: getSemanticModelStatus,
  },
];

/**
 * Asks once before any plugin run that would download a local AI model.
 * `confirmModelDownloads` resolves to the plugin IDs that may run: all of
 * them if confirmed (or nothing needs downloading), otherwise the list
 * without the model plugins that would download.
 */
export function useModelDownloadConsent() {
  const resolverRef = useRef<((confirmed: boolean) => void) | null>(null);
  const [pendingPlugins, setPendingPlugins] = useState<ModelPlugin[] | null>(null);

  async function confirmModelDownloads(pluginIds: string[]): Promise<string[]> {
    const needed: ModelPlugin[] = [];

    for (const plugin of MODEL_PLUGINS) {
      if (!pluginIds.includes(plugin.pluginId)) {
        continue;
      }
      try {
        const status = await plugin.getStatus();
        if (!(status.packagesInstalled && status.modelCached)) {
          needed.push(plugin);
        }
      } catch {
        needed.push(plugin);
      }
    }

    if (needed.length === 0) {
      return pluginIds;
    }

    const confirmed = await new Promise<boolean>((resolve) => {
      resolverRef.current = resolve;
      setPendingPlugins(needed);
    });

    return confirmed
      ? pluginIds
      : pluginIds.filter((pluginId) => !needed.some((plugin) => plugin.pluginId === pluginId));
  }

  function settle(confirmed: boolean) {
    const resolve = resolverRef.current;
    resolverRef.current = null;
    setPendingPlugins(null);
    resolve?.(confirmed);
  }

  const modelDownloadDialog = (
    <Dialog
      open={pendingPlugins !== null}
      onOpenChange={(isOpen) => {
        if (!isOpen) {
          settle(false);
        }
      }}
    >
      <DialogContent className="w-[min(30rem,calc(100vw-2rem))] rounded-sm p-0">
        <DialogHeader className="border-b px-3 py-2">
          <DialogTitle className="text-sm">Download AI models</DialogTitle>
          <DialogDescription className="text-xs">
            These plugins need a one-time model download. The models run entirely on this
            machine &mdash; evidence is never sent anywhere.
          </DialogDescription>
        </DialogHeader>
        <ul className="grid gap-1 px-3 py-2 text-xs">
          {pendingPlugins?.map((plugin) => (
            <li key={plugin.pluginId} className="flex items-center justify-between gap-2">
              <span className="font-medium">{plugin.name}</span>
              <span className="text-muted-foreground">{plugin.downloadSize}</span>
            </li>
          ))}
        </ul>
        <DialogFooter className="border-t p-2">
          <Button
            type="button"
            variant="outline"
            size="xs"
            className="h-7 rounded-sm px-2 text-xs"
            onClick={() => settle(false)}
          >
            Skip these plugins
          </Button>
          <Button
            type="button"
            size="xs"
            className="h-7 rounded-sm px-2 text-xs"
            onClick={() => settle(true)}
          >
            Download and run
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );

  return { confirmModelDownloads, modelDownloadDialog };
}
