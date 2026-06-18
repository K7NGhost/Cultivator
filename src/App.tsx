import { Route, Routes } from "react-router-dom";

import { AppShell } from "@/app/AppShell";
import "./App.css";

function App() {
  return (
    <Routes>
      <Route path="*" element={<AppShell />} />
    </Routes>
  );
}

export default App;
