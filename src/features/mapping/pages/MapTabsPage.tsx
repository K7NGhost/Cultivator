import { useEffect, useRef, useState } from "react";
import { useLocation, useSearchParams } from "react-router-dom";
import { Drone, Map } from "lucide-react";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { DronePage } from "@/features/drone/pages/DronePage";
import { MapPage } from "./MapPage";

type MapView = "general" | "drone";

export function MapTabsPage() {
  const location = useLocation();
  const [params, setParams] = useSearchParams();
  const view: MapView = location.pathname === "/drone" || params.get("view") === "drone" ? "drone" : "general";
  const currentView = useRef(view);
  currentView.current = view;
  const active = location.pathname === "/map" || location.pathname === "/drone";
  const [visited, setVisited] = useState(() => new Set<MapView>([view]));

  useEffect(() => {
    if (active) setVisited((previous) => previous.has(view) ? previous : new Set([...previous, view]));
  }, [view, active]);

  function changeView(value: string) {
    if ((value !== "general" && value !== "drone") || value === currentView.current) return;
    currentView.current = value;
    setParams((previous) => {
      const next = new URLSearchParams(previous);
      if (value === "drone") next.set("view", "drone");
      else next.delete("view");
      return next;
    });
  }

  return <Tabs value={view} onValueChange={changeView} className="h-full min-h-0 min-w-0 gap-0 overflow-hidden">
    <div className="shrink-0 border-b bg-background px-2">
      <TabsList variant="line" className="h-8" aria-label="Map views">
        <TabsTrigger value="general" className="rounded-none text-xs"><Map className="size-3.5" />General map</TabsTrigger>
        <TabsTrigger value="drone" className="rounded-none text-xs"><Drone className="size-3.5" />Drone map</TabsTrigger>
      </TabsList>
    </div>
    <TabsContent value="general" forceMount hidden={view !== "general"} className="m-0 min-h-0 overflow-hidden">
      {((active && view === "general") || visited.has("general")) && <MapPage active={active && view === "general"} />}
    </TabsContent>
    <TabsContent value="drone" forceMount hidden={view !== "drone"} className="m-0 min-h-0 overflow-hidden">
      {((active && view === "drone") || visited.has("drone")) && <DronePage active={active && view === "drone"} />}
    </TabsContent>
  </Tabs>;
}
