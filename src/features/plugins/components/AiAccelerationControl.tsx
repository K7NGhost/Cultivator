import { useEffect, useState } from "react";
import { Cpu, LoaderCircle, Zap } from "lucide-react";
import { toast } from "react-toastify";

import { Badge } from "@/components/ui/badge";
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
  enableGpuAcceleration,
  getAiAccelerationStatus,
  type AiAccelerationStatus,
} from "@/features/plugins/pluginRepository";

function getErrorMessage(caughtError: unknown) {
  return caughtError instanceof Error ? caughtError.message : String(caughtError);
}

/** Why the AI plugins run on the CPU, for the badge tooltip. */
function cpuReason(status: AiAccelerationStatus) {
  if (!status.gpuName) {
    return "No NVIDIA GPU was found, so AI plugins run on the CPU.";
  }
  if (!status.gpuSupported) {
    return `${status.gpuName} needs NVIDIA driver 528 or newer for GPU acceleration (installed: ${status.driverVersion ?? "unknown"}).`;
  }
  return `AI plugins run on the CPU. Switch to ${status.gpuName} for much faster tagging and indexing.`;
}

/**
 * Shows where the local AI models run (GPU or CPU) and offers a one-time
 * switch to the GPU build of PyTorch when this machine has a usable NVIDIA GPU.
 */
export function AiAccelerationControl() {
  const [status, setStatus] = useState<AiAccelerationStatus | null>(null);
  const [isConfirming, setIsConfirming] = useState(false);
  const [isInstalling, setIsInstalling] = useState(false);

  useEffect(() => {
    let isCurrent = true;
    getAiAccelerationStatus()
      .then((nextStatus) => {
        if (isCurrent) {
          setStatus(nextStatus);
        }
      })
      .catch(() => {
        // Without a Python runtime there is nothing to accelerate; show nothing.
      });
    return () => {
      isCurrent = false;
    };
  }, []);

  async function switchToGpu() {
    setIsConfirming(false);
    setIsInstalling(true);
    try {
      const nextStatus = await enableGpuAcceleration();
      setStatus(nextStatus);
      toast.success(`AI plugins now run on ${nextStatus.gpuName ?? "the GPU"}.`);
    } catch (caughtError) {
      toast.error(`GPU setup failed: ${getErrorMessage(caughtError)}`);
    } finally {
      setIsInstalling(false);
    }
  }

  if (!status) {
    return null;
  }

  if (status.torchUsesGpu || (status.gpuSupported && !status.torchInstalled)) {
    return (
      <Badge
        variant="secondary"
        className="h-5 gap-1 rounded-sm text-[11px]"
        title={
          status.torchUsesGpu
            ? `AI plugins run on ${status.gpuName ?? "the GPU"}.`
            : `The GPU build installs automatically the first time an AI plugin runs on ${status.gpuName}.`
        }
      >
        <Zap className="size-3" aria-hidden="true" />
        AI: GPU
      </Badge>
    );
  }

  const canSwitch = status.gpuSupported && status.torchInstalled;

  return (
    <>
      <Badge
        variant="outline"
        className="h-5 gap-1 rounded-sm text-[11px]"
        title={status.lastGpuError ?? cpuReason(status)}
      >
        <Cpu className="size-3" aria-hidden="true" />
        AI: CPU
      </Badge>
      {canSwitch && (
        <Button
          type="button"
          variant="outline"
          size="xs"
          className="h-7 rounded-sm px-2 text-xs"
          disabled={isInstalling}
          onClick={() => setIsConfirming(true)}
        >
          {isInstalling ? (
            <LoaderCircle className="size-3.5 animate-spin" aria-hidden="true" />
          ) : (
            <Zap className="size-3.5" aria-hidden="true" />
          )}
          {isInstalling ? "Installing GPU support" : "Use GPU"}
        </Button>
      )}

      <Dialog open={isConfirming} onOpenChange={setIsConfirming}>
        <DialogContent className="w-[min(30rem,calc(100vw-2rem))] rounded-sm p-0">
          <DialogHeader className="border-b px-3 py-2">
            <DialogTitle className="text-sm">Run AI plugins on the GPU</DialogTitle>
            <DialogDescription className="text-xs">
              Downloads the GPU build of PyTorch for {status.gpuName} once (about 3 GB).
              Media Tagging, Semantic Index and the Assistant then run on the GPU. Models
              and evidence stay on this machine.
            </DialogDescription>
          </DialogHeader>
          <DialogFooter className="border-t p-2">
            <Button
              type="button"
              variant="outline"
              size="xs"
              className="h-7 rounded-sm px-2 text-xs"
              onClick={() => setIsConfirming(false)}
            >
              Cancel
            </Button>
            <Button
              type="button"
              size="xs"
              className="h-7 rounded-sm px-2 text-xs"
              onClick={() => {
                void switchToGpu();
              }}
            >
              <Zap className="size-3.5" aria-hidden="true" />
              Download and switch
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
