import { Navigate, Route, Routes } from "react-router-dom";

import { AppShell } from "@/app/AppShell";
import { FilesPage } from "@/features/files/FilesPage";
import { SearchPage } from "@/features/search/SearchPage";
import { PlaceholderPage } from "@/features/workspace/PlaceholderPage";
import "./App.css";

function App() {
  return (
    <Routes>
      <Route element={<AppShell />}>
        <Route index element={<Navigate to="/files" replace />} />
        <Route
          path="/case"
          element={
            <PlaceholderPage
              title="Case"
              description="Case metadata, examiner notes, evidence intake, and audit state."
            />
          }
        />
        <Route path="/files" element={<FilesPage />} />
        <Route path="/search" element={<SearchPage />} />
        <Route
          path="/plugins"
          element={
            <PlaceholderPage
              title="Plugins"
              description="User scripts, extractors, parser permissions, and execution priority."
            />
          }
        />
        <Route
          path="/artifacts"
          element={
            <PlaceholderPage
              title="Artifacts"
              description="Normalized records extracted from logical files by user plugins."
            />
          }
        />
        <Route
          path="/timeline"
          element={
            <PlaceholderPage
              title="Timeline"
              description="Timestamps correlated from file metadata and plugin outputs."
            />
          }
        />
        <Route
          path="/reports"
          element={
            <PlaceholderPage
              title="Reports"
              description="Findings, exports, evidence summaries, and review packages."
            />
          }
        />
        <Route path="*" element={<Navigate to="/files" replace />} />
      </Route>
    </Routes>
  );
}

export default App;
