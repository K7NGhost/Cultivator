import { useEffect, useRef, useState } from "react";
import { useLocation, useSearchParams } from "react-router-dom";
import { Database, Search } from "lucide-react";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useCases } from "@/features/cases";
import { SearchPage } from "./SearchPage";
import { IndexedSearchPage } from "./IndexedSearchPage";

type SearchView = "indexed" | "grep";

export function SearchTabsPage() {
  const location = useLocation();
  const [params, setParams] = useSearchParams();
  const { activeCase } = useCases();
  const view: SearchView = params.get("view") === "grep" ? "grep" : "indexed";
  const currentView = useRef(view);
  currentView.current = view;
  const active = location.pathname === "/search";
  const [visited, setVisited] = useState(() => new Set<SearchView>([view]));

  useEffect(() => {
    if (active) setVisited(previous => previous.has(view) ? previous : new Set([...previous, view]));
  }, [view, active]);

  function changeView(value: string) {
    if ((value !== "indexed" && value !== "grep") || value === currentView.current) return;
    currentView.current = value;
    setParams(previous => {
      const next = new URLSearchParams(previous);
      if (value === "grep") next.set("view", "grep");
      else next.delete("view");
      return next;
    });
  }

  return <Tabs value={view} onValueChange={changeView} className="h-full min-h-0 min-w-0 gap-0 overflow-hidden">
    <div className="shrink-0 border-b bg-background px-2">
      <TabsList variant="line" className="h-8" aria-label="Search views">
        <TabsTrigger value="indexed" className="rounded-none text-xs"><Database className="size-3.5" />Indexed</TabsTrigger>
        <TabsTrigger value="grep" className="rounded-none text-xs"><Search className="size-3.5" />Grep</TabsTrigger>
      </TabsList>
    </div>
    <TabsContent value="indexed" forceMount hidden={view !== "indexed"} className="m-0 min-h-0 overflow-hidden">
      {((active && view === "indexed") || visited.has("indexed")) && <IndexedSearchPage key={activeCase?.databasePath ?? "no-case"} active={active && view === "indexed"} />}
    </TabsContent>
    <TabsContent value="grep" forceMount hidden={view !== "grep"} className="m-0 min-h-0 overflow-hidden">
      {((active && view === "grep") || visited.has("grep")) && <SearchPage />}
    </TabsContent>
  </Tabs>;
}
