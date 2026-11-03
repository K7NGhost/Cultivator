import { useEffect, useRef, useState } from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { FlipHorizontal2, LoaderCircle, RotateCcw, RotateCw, ZoomIn, ZoomOut } from "lucide-react";
import { TransformComponent, TransformWrapper, type ReactZoomPanPinchRef } from "react-zoom-pan-pinch";

import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import type { MediaItem } from "../model/types";

type Props = { item: MediaItem; onClose: () => void };

export default function MediaImageViewer({ item, onClose }: Props) {
  const [source, setSource] = useState<string>();
  const [failed, setFailed] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const [loaded, setLoaded] = useState(false);
  const [naturalSize, setNaturalSize] = useState({ width: item.width || 1, height: item.height || 1 });
  const [viewport, setViewport] = useState({ width: 1, height: 1 });
  const [rotation, setRotation] = useState(0);
  const [flipped, setFlipped] = useState(false);
  const [brightness, setBrightness] = useState(100);
  const [contrast, setContrast] = useState(100);
  const [grayscale, setGrayscale] = useState(false);
  const [zoom, setZoom] = useState(1);
  const transform = useRef<ReactZoomPanPinchRef>(null);
  const [canvas, setCanvas] = useState<HTMLDivElement | null>(null);
  const [previousFocus] = useState(() => document.activeElement instanceof HTMLElement ? document.activeElement : null);

  useEffect(() => {
    let current = true;
    setSource(undefined);
    setFailed(false);
    setLoaded(false);
    invoke<string>("ensure_evidence_file", { path: item.mediaPath || item.path }).then(path => {
      if (current) setSource(convertFileSrc(path));
    }).catch(() => { if (current) setFailed(true); });
    return () => { current = false; };
  }, [item.mediaPath, item.path, attempt]);

  useEffect(() => {
    if (!canvas) return;
    const observer = new ResizeObserver(([entry]) => {
      setViewport({ width: Math.max(1, entry.contentRect.width - 32), height: Math.max(1, entry.contentRect.height - 32) });
    });
    observer.observe(canvas);
    return () => observer.disconnect();
  }, [canvas]);

  const sideways = rotation % 180 !== 0;
  const rotatedWidth = sideways ? naturalSize.height : naturalSize.width;
  const rotatedHeight = sideways ? naturalSize.width : naturalSize.height;
  const fitScale = Math.min(1, viewport.width / rotatedWidth, viewport.height / rotatedHeight);
  const width = naturalSize.width * fitScale;
  const height = naturalSize.height * fitScale;
  const contentWidth = rotatedWidth * fitScale;
  const contentHeight = rotatedHeight * fitScale;

  useEffect(() => {
    if (loaded) void transform.current?.centerView(1, 0);
  }, [contentWidth, contentHeight, loaded]);

  const reset = () => {
    setRotation(0);
    setFlipped(false);
    setBrightness(100);
    setContrast(100);
    setGrayscale(false);
    void transform.current?.centerView(1);
  };

  return (
    <Dialog open onOpenChange={open => { if (!open) onClose(); }}>
      <DialogContent className="flex h-[min(92vh,60vw,60rem)] w-[96vw] max-w-none flex-col gap-0 overflow-hidden p-0 sm:max-w-[96rem]"
        onCloseAutoFocus={event => {
          if (previousFocus?.isConnected) { event.preventDefault(); previousFocus.focus({ preventScroll: true }); }
        }}>
        <header className="shrink-0 border-b px-3 py-2 pr-12">
          <DialogTitle className="truncate text-sm" title={item.name}>{item.name}</DialogTitle>
          <DialogDescription className="mt-1 text-xs">Scroll or pinch to zoom. Drag to pan. Display adjustments apply only to this preview.</DialogDescription>
        </header>
        <div role="toolbar" aria-label="Image view controls" className="flex shrink-0 flex-wrap items-center gap-1 border-b p-2">
          <Button size="icon-sm" variant="outline" aria-label="Zoom out" title="Zoom out" disabled={!loaded || failed} onClick={() => { void transform.current?.zoomOut(); }}><ZoomOut /></Button>
          <output className="w-14 text-center text-xs tabular-nums" aria-label="Image zoom">{loaded ? `${Math.round(zoom * fitScale * 100)}%` : "—"}</output>
          <Button size="icon-sm" variant="outline" aria-label="Zoom in" title="Zoom in" disabled={!loaded || failed} onClick={() => { void transform.current?.zoomIn(); }}><ZoomIn /></Button>
          <Button size="sm" variant="outline" disabled={!loaded || failed} onClick={() => { void transform.current?.centerView(1); }}>Fit</Button>
          <Button size="sm" variant="outline" disabled={!loaded || failed} onClick={() => { void transform.current?.centerView(1 / fitScale); }}>Actual size</Button>
          <span className="mx-1 h-5 border-l" aria-hidden="true" />
          <Button size="icon-sm" variant="outline" aria-label="Rotate left" title="Rotate left" disabled={!loaded || failed} onClick={() => setRotation(value => (value + 270) % 360)}><RotateCcw /></Button>
          <Button size="icon-sm" variant="outline" aria-label="Rotate right" title="Rotate right" disabled={!loaded || failed} onClick={() => setRotation(value => (value + 90) % 360)}><RotateCw /></Button>
          <Button size="icon-sm" variant="outline" aria-label="Flip horizontally" title="Flip horizontally" aria-pressed={flipped} disabled={!loaded || failed} onClick={() => setFlipped(value => !value)}><FlipHorizontal2 /></Button>
          <Button size="sm" variant="outline" className="ml-auto" onClick={reset}>Reset view</Button>
        </div>
        <div ref={setCanvas} className="relative min-h-0 flex-1 overflow-hidden bg-black">
          {!failed && source && <TransformWrapper ref={transform} minScale={1} maxScale={Math.max(8, 1 / fitScale)} centerOnInit
            doubleClick={{ mode: "toggle" }} onTransform={(_, state) => setZoom(state.scale)}>
            <TransformComponent wrapperStyle={{ width: "100%", height: "100%" }} contentStyle={{ width: contentWidth, height: contentHeight }}>
              <div style={{ width: contentWidth, height: contentHeight, position: "relative" }}>
                <img src={source} alt={item.name} draggable={false} decoding="async"
                  style={{ position: "absolute", left: "50%", top: "50%", width, height, maxWidth: "none",
                    transform: `translate(-50%, -50%) rotate(${rotation}deg) scaleX(${flipped ? -1 : 1})`,
                    filter: `brightness(${brightness}%) contrast(${contrast}%) grayscale(${grayscale ? 100 : 0}%)` }}
                  onLoad={event => {
                    setNaturalSize({ width: event.currentTarget.naturalWidth, height: event.currentTarget.naturalHeight });
                    setLoaded(true);
                  }} onError={() => { setFailed(true); setLoaded(false); }} />
              </div>
            </TransformComponent>
          </TransformWrapper>}
          {!loaded && !failed && <div role="status" className="pointer-events-none absolute inset-0 flex items-center justify-center gap-2 text-sm text-white"><LoaderCircle className="size-5 animate-spin" />Loading image…</div>}
          {failed && <div role="alert" className="absolute inset-0 flex flex-col items-center justify-center gap-3 p-4 text-center text-sm text-white">
            <p>This image could not be displayed. The file may be unavailable or its format unsupported.</p>
            <Button variant="secondary" size="sm" onClick={() => setAttempt(value => value + 1)}>Retry</Button>
          </div>}
        </div>
        <fieldset disabled={!loaded || failed} className="flex shrink-0 flex-wrap items-center gap-x-5 gap-y-2 border-t p-3 text-xs">
          <legend className="sr-only">Image display adjustments</legend>
          <Adjustment label="Brightness" value={brightness} onChange={setBrightness} />
          <Adjustment label="Contrast" value={contrast} onChange={setContrast} />
          <label className="flex items-center gap-2"><input type="checkbox" checked={grayscale} onChange={event => setGrayscale(event.target.checked)} />Grayscale</label>
        </fieldset>
      </DialogContent>
    </Dialog>
  );
}

function Adjustment({ label, value, onChange }: { label: string; value: number; onChange: (value: number) => void }) {
  return <label className="flex items-center gap-2">{label}
    <input type="range" min={0} max={200} step={5} value={value} aria-label={label}
      className="w-28 accent-primary" onChange={event => onChange(Number(event.target.value))} />
    <output className="w-9 text-right tabular-nums">{value}%</output>
  </label>;
}
