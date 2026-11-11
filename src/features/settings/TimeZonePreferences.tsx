import { useState } from "react";
import { availableTimeZones, resolveTimeZone, saveTimeZone, useTimeZonePreference } from "./timeZoneSettings";

export function TimeZonePreferences() {
  const preference = useTimeZonePreference();
  const [zones] = useState(availableTimeZones);
  const [message, setMessage] = useState("");
  return <section className="grid gap-3 text-xs" aria-label="Timezone settings">
    <h2 className="border-b pb-1 font-medium">Date and time</h2>
    <label className="grid grid-cols-[13rem_minmax(0,1fr)] items-center gap-2">Display timezone
      <select aria-label="Display timezone" className="h-8 min-w-0 rounded-sm border bg-background px-2" value={preference}
        onChange={(event) => { try { saveTimeZone(event.target.value); setMessage("Timezone saved."); } catch (error) { setMessage(String(error)); } }}>
        <option value="system">System timezone ({resolveTimeZone("system")})</option>
        {zones.map((zone) => <option key={zone} value={zone}>{zone.replace(/_/g, " ")}</option>)}
      </select>
    </label>
    <p className="text-muted-foreground">Saves automatically. UTC and offset timestamps are converted to this timezone, including daylight saving time. Map date/time filters use the same timezone. Original evidence values are preserved; timestamps without a timezone keep their recorded clock.</p>
    {message && <p role="status" className="text-muted-foreground">{message}</p>}
  </section>;
}
