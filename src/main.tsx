import React from "react";
import ReactDOM from "react-dom/client";
import { HashRouter } from "react-router-dom";
import { ToastContainer } from "react-toastify";
import "react-toastify/dist/ReactToastify.css";
import App from "./App";
import { ThemeProvider } from "./components/theme-provider";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ThemeProvider defaultTheme="light">
      <HashRouter>
        <App />
        <ToastContainer
          position="bottom-right"
          autoClose={false}
          closeButton
          closeOnClick={false}
          draggable={false}
          newestOnTop
          pauseOnFocusLoss={false}
          theme="colored"
        />
      </HashRouter>
    </ThemeProvider>
  </React.StrictMode>,
);
