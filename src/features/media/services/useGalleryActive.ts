import { useSyncExternalStore } from "react";
import { useLocation } from "react-router-dom";

const subscribe = (listener: () => void) => {
  document.addEventListener("visibilitychange", listener);
  return () => document.removeEventListener("visibilitychange", listener);
};
export function useGalleryActive() {
  const location = useLocation();
  const visible = useSyncExternalStore(subscribe, () => document.visibilityState !== "hidden");
  return location.pathname === "/media" && visible;
}
