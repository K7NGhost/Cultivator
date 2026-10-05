import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { getPythonRuntimeStatus, preparePythonRuntime } from "../services/pluginRepository";
import type { PythonRuntimeStatus } from "../model/types";

export function PythonRuntimePanel() {
  const [status, setStatus] = useState<PythonRuntimeStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [isPreparing, setIsPreparing] = useState(false);

  useEffect(() => {
    let active = true;
    void getPythonRuntimeStatus().then(
      (value) => { if (active) setStatus(value); },
      (caught: unknown) => { if (active) setError(String(caught)); },
    );
    return () => { active = false; };
  }, []);

  async function prepare() {
    setIsPreparing(true);
    setError(null);
    try {
      setStatus(await preparePythonRuntime());
    } catch (caught) {
      setError(String(caught));
    } finally {
      setIsPreparing(false);
    }
  }

  return (
    <section className="shrink-0 border-b px-3 py-2 text-xs" aria-label="Python setup">
      <div className="flex items-center gap-3">
        <span className="font-medium">Python {status?.python ?? "3.12"}</span>
        <span className="text-muted-foreground" role="status">
          {isPreparing ? "Setting up Python…" : status?.ready ? "Ready" : "Set up automatically when a plugin runs"}
        </span>
        <Button size="xs" variant="outline" disabled={isPreparing} onClick={() => void prepare()}>
          {status?.ready ? "Check setup" : "Set up Python"}
        </Button>
      </div>
      {status && !status.uvAvailable && !status.ready && (
        <p className="mt-1 text-muted-foreground">
          Install <a className="underline" href="https://docs.astral.sh/uv/getting-started/installation/" target="_blank" rel="noreferrer">uv</a> and restart Cultivator to enable Python setup.
        </p>
      )}
      {error && <p className="mt-1 text-destructive" role="alert">{error}</p>}
    </section>
  );
}
