import { afterEach, describe, expect, mock, test } from "bun:test";

mock.module("@tauri-apps/api/core", () => ({ convertFileSrc: (path: string) => path }));
const { getVideoThumbnail } = await import("../src/features/media/videoThumbnails");

const originalDocument = globalThis.document;
afterEach(() => {
  globalThis.document = originalDocument;
});

function installDecoder({ fail = false, stall = false } = {}) {
  const videos: FakeVideo[] = [];
  const frames: number[] = [];
  class FakeVideo extends EventTarget {
    duration = 100;
    videoWidth = 1920;
    videoHeight = 1080;
    src = "";
    time = 0;
    released = false;
    get currentTime() { return this.time; }
    set currentTime(value: number) {
      this.time = value;
      queueMicrotask(() => this.dispatchEvent(new Event("seeked")));
    }
    load() {
      if (this.src && !stall) {
        queueMicrotask(() => this.dispatchEvent(new Event(fail ? "error" : "loadeddata")));
      }
    }
    pause() {}
    removeAttribute() { this.src = ""; this.released = true; }
  }
  globalThis.document = {
    createElement(tag: string) {
      if (tag === "video") {
        const video = new FakeVideo();
        videos.push(video);
        return video;
      }
      return {
        getContext: () => ({
          fillRect() {},
          drawImage(video: FakeVideo) { frames.push(video.currentTime); },
        }),
        toDataURL: () => "data:image/jpeg;base64,thumbnail",
      };
    },
  } as unknown as Document;
  return { videos, frames };
}

describe("video thumbnails", () => {
  test("samples across the clip, releases the decoder, and reuses the result", async () => {
    const { videos, frames } = installDecoder();
    const signal = new AbortController().signal;
    const source = await getVideoThumbnail("frames", "clip.mp4", signal);
    expect(source).toStartWith("data:image/jpeg");
    expect(frames).toEqual([10, 35, 60, 85]);
    expect(videos[0].released).toBe(true);
    expect(await getVideoThumbnail("frames", "clip.mp4", signal)).toBe(source);
    expect(videos).toHaveLength(1);
    await getVideoThumbnail("changed-file", "clip.mp4", signal);
    expect(videos).toHaveLength(2);
  });

  test("releases a failed decoder and allows subsequent clips to run", async () => {
    const failed = installDecoder({ fail: true });
    const signal = new AbortController().signal;
    await expect(getVideoThumbnail("broken", "broken.mp4", signal)).rejects.toThrow();
    expect(failed.videos[0].released).toBe(true);
    const healthy = installDecoder();
    await getVideoThumbnail("healthy", "healthy.mp4", signal);
    expect(healthy.frames).toHaveLength(4);
  });

  test("limits active decoders and cancels both active and queued work", async () => {
    const { videos } = installDecoder({ stall: true });
    const controllers = Array.from({ length: 6 }, () => new AbortController());
    const results = Promise.allSettled(controllers.map((controller, index) =>
      getVideoThumbnail(`cancel-${index}`, "clip.mp4", controller.signal),
    ));
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(videos).toHaveLength(2);
    controllers.forEach((controller) => controller.abort());
    expect((await results).every((result) => result.status === "rejected")).toBe(true);
    expect(videos).toHaveLength(2);
    expect(videos.every((video) => video.released)).toBe(true);
  });
});
