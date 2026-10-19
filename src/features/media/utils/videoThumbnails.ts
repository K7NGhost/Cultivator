import { convertFileSrc } from "@tauri-apps/api/core";
import { getPerformanceSettings, subscribePerformanceSettings, thumbnailCacheBytes } from "../../settings/performanceSettings";

const FRAME_FRACTIONS = [0.1, 0.35, 0.6, 0.85];
const FRAME_WIDTH = 240;
const FRAME_HEIGHT = 135;
const thumbnails = new Map<string, string>();
let generation = 0;
export function clearVideoThumbnails() { generation++; thumbnails.clear(); }
subscribePerformanceSettings(clearVideoThumbnails);
// Two decoder lanes keep a large gallery from opening a decoder for every tile.
const lanes: Promise<void>[] = [Promise.resolve(), Promise.resolve()];
let nextLane = 0;

export function getVideoThumbnail(
  key: string,
  path: string,
  signal: AbortSignal,
): Promise<string> {
  const cached = thumbnails.get(key);
  if (cached) {
    thumbnails.delete(key);
    thumbnails.set(key, cached);
    return Promise.resolve(cached);
  }

  const lane = nextLane++ % lanes.length;
  const epoch = generation;
  const request = lanes[lane].then(async () => {
    signal.throwIfAborted();
    const existing = thumbnails.get(key);
    if (existing) return existing;
    const thumbnail = await captureVideoFrames(path, signal);
    signal.throwIfAborted();
    if (epoch !== generation) return thumbnail;
    thumbnails.set(key, thumbnail);
    const capacity = Math.max(1, Math.min(128, Math.floor(thumbnailCacheBytes(getPerformanceSettings()) / 4 / (FRAME_WIDTH * FRAME_HEIGHT * 16 + 4096))));
    if (thumbnails.size > capacity) {
      const oldest = thumbnails.keys().next().value;
      if (oldest !== undefined) thumbnails.delete(oldest);
    }
    return thumbnail;
  });
  // A failed or cancelled clip must not block subsequent thumbnails.
  lanes[lane] = request.then(() => undefined, () => undefined);
  return request;
}

async function captureVideoFrames(path: string, signal: AbortSignal) {
  const video = document.createElement("video");
  video.muted = true;
  video.playsInline = true;
  video.preload = "auto";
  video.crossOrigin = "anonymous";
  const canvas = document.createElement("canvas");
  canvas.width = FRAME_WIDTH * 2;
  canvas.height = FRAME_HEIGHT * 2;
  const context = canvas.getContext("2d");
  if (!context) throw new Error("Video thumbnail rendering is unavailable.");

  try {
    await waitForVideo(video, "loadeddata", signal, () => {
      video.src = convertFileSrc(path);
      video.load();
    });
    if (
      !Number.isFinite(video.duration) || video.duration <= 0 ||
      !video.videoWidth || !video.videoHeight
    ) {
      throw new Error("Video has no decodable frames.");
    }

    context.fillStyle = "#000";
    context.fillRect(0, 0, canvas.width, canvas.height);
    for (const [index, fraction] of FRAME_FRACTIONS.entries()) {
      await waitForVideo(video, "seeked", signal, () => {
        video.currentTime = video.duration * fraction;
      });
      // Preserve the entire frame, including portrait footage, without cropping.
      const scale = Math.min(
        FRAME_WIDTH / video.videoWidth,
        FRAME_HEIGHT / video.videoHeight,
      );
      const width = video.videoWidth * scale;
      const height = video.videoHeight * scale;
      const x = (index % 2) * FRAME_WIDTH;
      const y = Math.floor(index / 2) * FRAME_HEIGHT;
      context.drawImage(
        video,
        x + (FRAME_WIDTH - width) / 2,
        y + (FRAME_HEIGHT - height) / 2,
        width,
        height,
      );
    }
    return canvas.toDataURL("image/jpeg", 0.8);
  } finally {
    video.pause();
    video.removeAttribute("src");
    video.load();
  }
}

function waitForVideo(
  video: HTMLVideoElement,
  event: "loadeddata" | "seeked",
  signal: AbortSignal,
  start: () => void,
): Promise<void> {
  return new Promise((resolve, reject) => {
    signal.throwIfAborted();
    const cleanup = () => {
      clearTimeout(timeout);
      video.removeEventListener(event, onReady);
      video.removeEventListener("error", onError);
      signal.removeEventListener("abort", onAbort);
    };
    const onReady = () => {
      cleanup();
      resolve();
    };
    const onError = () => {
      cleanup();
      reject(new Error("Video preview unavailable."));
    };
    const onAbort = () => {
      cleanup();
      reject(signal.reason);
    };
    const timeout = setTimeout(onError, 10_000);
    video.addEventListener(event, onReady, { once: true });
    video.addEventListener("error", onError, { once: true });
    signal.addEventListener("abort", onAbort, { once: true });
    try {
      start();
    } catch (error) {
      cleanup();
      reject(error);
    }
  });
}
