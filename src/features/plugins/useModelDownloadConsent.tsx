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
  getAiAccelerationStatus,
  getMediaTaggingStatus,
  getSemanticModelStatus,
  mediaTaggingPluginId,
  semanticIndexPluginId,
  type ModelStatus,
} from "@/features/plugins/pluginRepository";

type ModelPlugin = {
  pluginId: string;
  name: string;
  getStatus: () => Promise<ModelStatus>;
  /** What is left to download, or null when everything is on disk. */
  downloadSize: (status: ModelStatus | null) => string | null;
};

type PendingDownload = ModelPlugin & { size: string };

const MODEL_PLUGINS: ModelPlugin[] = [
  {
    pluginId: mediaTaggingPluginId,
    name: "Media Tagging (AI)",
    getStatus: getMediaTaggingStatus,
    downloadSize: (status) => (status?.packagesInstalled && status.modelCached ? null : "~1.5 GB"),
  },
  {
    pluginId: semanticIndexPluginId,
    name: "Semantic Index (AI) + precision check",
    getStatus: getSemanticModelStatus,
    downloadSize: (status) => {
      if (status?.packagesInstalled && status.modelCached && status.precisionModelCached) {
        return null;
      }
      // The search model is ~2.3 GB, the precision check's ~1.2 GB.
      return status?.modelCached ? "~1.2 GB" : "~3.5 GB";
    },
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
  const [pendingPlugins, setPendingPlugins] = useState<PendingDownload[] | null>(null);
  const [gpuName, setGpuName] = useState<string | null>(null);

  async function confirmModelDownloads(pluginIds: string[]): Promise<string[]> {
    const needed: PendingDownload[] = [];

    for (const plugin of MODEL_PLUGINS) {
      if (!pluginIds.includes(plugin.pluginId)) {
        continue;
      }
      const status = await plugin.getStatus().catch(() => null);
      const size = plugin.downloadSize(status);
      if (size) {
        needed.push({ ...plugin, size });
      }
    }

    if (needed.length === 0) {
      return pluginIds;
    }

    // A first install on an NVIDIA machine also fetches the GPU build of PyTorch.
    const acceleration = await getAiAccelerationStatus().catch(() => null);
    setGpuName(
      acceleration?.gpuSupported && !acceleration.torchInstalled ? acceleration.gpuName : null,
    );

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
              <span className="text-muted-foreground">{plugin.size}</span>
            </li>
          ))}
        </ul>
        {gpuName && (
          <p className="border-t px-3 py-2 text-[11px] text-muted-foreground">
            Includes the GPU build of PyTorch for {gpuName} (about 3 GB more), so the
            models run on the GPU.
          </p>
        )}
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
