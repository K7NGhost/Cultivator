import {
  CaseSensitive,
  FileCode2,
  FolderOpen,
  Hexagon,
  Play,
  Regex,
  Search,
} from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Separator } from "@/components/ui/separator";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { cn } from "@/lib/utils";

const searchMatches = [
  {
    file: "History",
    path: "Users/Inter/AppData/Local/Browser/Profile/History",
    line: 3812,
    column: 44,
    type: "SQLite",
    match: "https://accounts.example.test/login",
    context: "last_visit_time, url, title, typed_count",
  },
  {
    file: "notes.txt",
    path: "Users/Inter/Documents/notes.txt",
    line: 18,
    column: 7,
    type: "Text",
    match: "recovery phrase stored in old browser profile",
    context: "Follow up on recovery phrase stored in old browser profile",
  },
  {
    file: "Preferences",
    path: "Users/Inter/AppData/Roaming/App/Preferences",
    line: 94,
    column: 19,
    type: "JSON",
    match: "\"sync_account\": \"inter@example.test\"",
    context: "profile settings and sync metadata",
  },
  {
    file: "Login Data",
    path: "Users/Inter/AppData/Local/Browser/Profile/Login Data",
    line: 0,
    column: 0,
    type: "SQLite",
    match: "binary match in database page",
    context: "rg reported binary content match",
  },
];

const textPreview = [
  "12  Open browser profile export",
  "13  Validate user script output against source file",
  "14  Queue credential parser for Login Data",
  "15",
  "16  Browser artifacts:",
  "17  - History database",
  "18  - recovery phrase stored in old browser profile",
  "19  - Extension settings",
  "20",
  "21  Add findings to review report",
];

const hexPreview = [
  "00000000  53 51 4c 69 74 65 20 66  6f 72 6d 61 74 20 33 00  SQLite format 3.",
  "00000010  10 00 01 01 00 40 20 20  00 00 02 6b 00 00 08 f4  .....@  ...k....",
  "00000020  00 00 00 00 00 00 00 00  00 00 00 18 00 00 00 04  ................",
  "00000030  00 00 00 00 00 00 00 00  00 00 00 01 00 00 00 00  ................",
  "00000040  72 65 63 6f 76 65 72 79  20 70 68 72 61 73 65 20  recovery phrase ",
  "00000050  73 74 6f 72 65 64 20 69  6e 20 6f 6c 64 20 62 72  stored in old br",
];

export function SearchPage() {
  const selectedMatch = searchMatches[1];

  return (
    <div className="flex h-full min-h-0 flex-col bg-background">
      <section className="flex h-9 shrink-0 items-center gap-2 border-b px-2">
        <div className="relative w-80">
          <Search
            className="absolute left-2 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground"
            aria-hidden="true"
          />
          <Input
            className="h-7 rounded-none pl-7 text-xs"
            defaultValue="recovery phrase"
            aria-label="Search query"
          />
        </div>
        <Button size="xs" className="h-7 rounded-none px-2 text-xs">
          <Play className="size-3.5" aria-hidden="true" />
          Run rg
        </Button>
        <Separator orientation="vertical" className="h-5" />
        <Button variant="ghost" size="xs" className="h-7 rounded-none px-2 text-xs">
          <Regex className="size-3.5" aria-hidden="true" />
          Regex
        </Button>
        <Button variant="ghost" size="xs" className="h-7 rounded-none px-2 text-xs">
          <CaseSensitive className="size-3.5" aria-hidden="true" />
          Case
        </Button>
        <div className="ml-auto flex items-center gap-2 text-[11px] text-muted-foreground">
          <span>ripgrep preview UI</span>
          <Badge variant="outline" className="h-5 rounded-none text-[11px]">
            {searchMatches.length} matches
          </Badge>
        </div>
      </section>

      <section className="flex h-8 shrink-0 items-center gap-2 border-b px-2">
        <FolderOpen className="size-3.5 text-muted-foreground" aria-hidden="true" />
        <span className="truncate text-xs">
          rg --line-number --column --context 1 "recovery phrase" Users/Inter
        </span>
      </section>

      <div className="grid min-h-0 flex-1 grid-rows-[minmax(260px,1fr)_260px]">
        <section className="min-h-0 border-b" aria-label="Search matches">
          <ScrollArea className="h-full">
            <Table>
              <TableHeader>
                <TableRow className="h-7">
                  <TableHead className="h-7 w-[90px] px-2 text-[11px]">
                    File
                  </TableHead>
                  <TableHead className="h-7 px-2 text-[11px]">Path</TableHead>
                  <TableHead className="h-7 w-[70px] px-2 text-[11px]">
                    Line
                  </TableHead>
                  <TableHead className="h-7 w-[80px] px-2 text-[11px]">
                    Type
                  </TableHead>
                  <TableHead className="h-7 px-2 text-[11px]">Match</TableHead>
                  <TableHead className="h-7 w-[92px] px-2 text-[11px]">
                    Action
                  </TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {searchMatches.map((match, index) => (
                  <TableRow
                    key={`${match.path}-${match.line}-${match.column}`}
                    data-state={index === 1 ? "selected" : undefined}
                    className="h-8"
                  >
                    <TableCell className="max-w-[120px] px-2 py-1 text-xs">
                      <div className="flex min-w-0 items-center gap-1.5">
                        <FileCode2
                          className="size-3.5 shrink-0 text-muted-foreground"
                          aria-hidden="true"
                        />
                        <span className="truncate">{match.file}</span>
                      </div>
                    </TableCell>
                    <TableCell className="max-w-[360px] px-2 py-1 text-xs">
                      <span className="block truncate">{match.path}</span>
                    </TableCell>
                    <TableCell className="px-2 py-1 text-xs">
                      {match.line}:{match.column}
                    </TableCell>
                    <TableCell className="px-2 py-1 text-xs">
                      <Badge variant="outline" className="h-5 rounded-none px-1 text-[10px]">
                        {match.type}
                      </Badge>
                    </TableCell>
                    <TableCell className="max-w-[360px] px-2 py-1 text-xs">
                      <span className="block truncate">{match.match}</span>
                      <span className="block truncate text-[11px] text-muted-foreground">
                        {match.context}
                      </span>
                    </TableCell>
                    <TableCell className="px-2 py-1">
                      <Button
                        variant="ghost"
                        size="xs"
                        className="h-6 rounded-none px-1.5 text-[11px]"
                      >
                        Open
                      </Button>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </ScrollArea>
        </section>

        <section className="min-h-0" aria-label="File preview">
          <Tabs defaultValue="text" className="h-full gap-0">
            <div className="flex h-8 items-center justify-between border-b px-2">
              <div className="min-w-0 text-xs">
                <span className="font-medium">Preview: </span>
                <span className="text-muted-foreground">{selectedMatch.path}</span>
              </div>
              <TabsList
                variant="line"
                className="h-7 rounded-none p-0"
                aria-label="Preview mode"
              >
                <TabsTrigger
                  value="text"
                  className="h-7 rounded-none px-2 text-xs"
                >
                  <FileCode2 className="size-3.5" aria-hidden="true" />
                  Text
                </TabsTrigger>
                <TabsTrigger
                  value="hex"
                  className="h-7 rounded-none px-2 text-xs"
                >
                  <Hexagon className="size-3.5" aria-hidden="true" />
                  Hex
                </TabsTrigger>
              </TabsList>
            </div>

            <TabsContent value="text" className="m-0 h-[calc(100%-2rem)]">
              <ScrollArea className="h-full">
                <pre className="p-2 font-mono text-xs leading-5">
                  {textPreview.map((line) => (
                    <div
                      key={line}
                      className={cn(
                        "whitespace-pre-wrap",
                        line.includes("recovery phrase") && "bg-amber-500/20",
                      )}
                    >
                      {line}
                    </div>
                  ))}
                </pre>
              </ScrollArea>
            </TabsContent>

            <TabsContent value="hex" className="m-0 h-[calc(100%-2rem)]">
              <ScrollArea className="h-full">
                <pre className="p-2 font-mono text-xs leading-5">
                  {hexPreview.map((line) => (
                    <div
                      key={line}
                      className={cn(
                        "whitespace-pre-wrap",
                        line.includes("72 65 63 6f") && "bg-amber-500/20",
                      )}
                    >
                      {line}
                    </div>
                  ))}
                </pre>
              </ScrollArea>
            </TabsContent>
          </Tabs>
        </section>
      </div>

      <footer className="flex h-6 shrink-0 items-center gap-3 border-t px-2 text-[11px] text-muted-foreground">
        <span>Search idle</span>
        <span>Engine: ripgrep</span>
        <span>Scope: logical files</span>
        <span>Preview: text/hex</span>
      </footer>
    </div>
  );
}
