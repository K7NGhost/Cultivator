import { useState } from "react";
import { ExternalLink } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Button } from "@/components/ui/button";
import { locationBrowserUrl } from "@/lib/maps/locationUrl";

export function OpenLocationInBrowserButton({ latitude, longitude }: {
  latitude: number | null | undefined;
  longitude: number | null | undefined;
}) {
  const url = locationBrowserUrl(latitude, longitude);
  const [opening, setOpening] = useState(false);
  const [failedUrl, setFailedUrl] = useState<string | null>(null);

  async function openLocation() {
    if (!url || opening) return;
    setOpening(true);
    setFailedUrl(null);
    try {
      // The native opener uses the user's default browser, outside the webview.
      await openUrl(url);
    } catch {
      setFailedUrl(url);
    } finally {
      setOpening(false);
    }
  }

  return <div className="flex flex-wrap items-center gap-2">
    <Button type="button" variant="outline" size="sm" className="h-7 px-2 text-xs"
      disabled={!url || opening} onClick={() => void openLocation()}
      title={url ? `Open ${latitude}, ${longitude} in your default browser` : "Select a valid location first"}>
      <ExternalLink className="size-3.5" aria-hidden="true" />
      {opening ? "Opening…" : "Open in browser"}
    </Button>
    {url && failedUrl === url && <span role="alert" className="max-w-56 text-xs text-destructive">
      Could not open the default browser. Try again.
    </span>}
  </div>;
}
