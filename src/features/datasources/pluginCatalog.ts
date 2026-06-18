import type { DataSourcePlugin } from "@/features/datasources/types";

export const dataSourcePlugins: DataSourcePlugin[] = [
  {
    id: "file-metadata",
    name: "File Metadata",
    description: "Collect size, timestamps, extension, and filesystem attributes.",
    type: "other",
  },
  {
    id: "keyword-scanner",
    name: "Keyword Scanner",
    description: "Index plain text and binary strings for case search.",
    type: "other",
  },
  {
    id: "string-extractor",
    name: "String Extractor",
    description: "Extract printable strings from unknown and binary files.",
    type: "other",
  },
  {
    id: "image-metadata",
    name: "Image Metadata",
    description: "Read EXIF and basic image properties.",
    type: "other",
  },
  {
    id: "sqlite-parser",
    name: "SQLite Parser",
    description: "Inspect SQLite databases and known application stores.",
    type: "other",
  },
  {
    id: "browser-history",
    name: "Browser History",
    description: "Parse common Chromium and Firefox browsing artifacts.",
    type: "windows",
  },
];
