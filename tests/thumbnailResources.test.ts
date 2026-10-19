import { afterEach, expect, test } from "bun:test";
import { decodeThumbnail } from "../src/features/media/services/decodedThumbnailCache";

const originalImage = globalThis.Image;
const originalFetch = globalThis.fetch;
afterEach(() => {
  globalThis.Image = originalImage;
  globalThis.fetch = originalFetch;
});

function installImage(fail = false) {
  const images: Array<{ src: string }> = [];
  class FakeImage {
    src = "";
    naturalWidth = 256;
    naturalHeight = 192;
    constructor() { images.push(this); }
    async decode() { if (fail) throw new Error("Bad image"); }
    removeAttribute() { this.src = ""; }
  }
  globalThis.Image = FakeImage as unknown as typeof Image;
  const reads: Array<RequestInit | undefined> = [];
  globalThis.fetch = (async (_input: unknown, options?: RequestInit) => {
    reads.push(options);
    return new Response(new Blob(["thumbnail"], { type: "image/jpeg" }));
  }) as typeof fetch;
  return { images, reads };
}

test("decoded thumbnails release their image and revoke their blob on eviction", async () => {
  const { images, reads } = installImage();
  const value = await decodeThumbnail("asset://thumbnail.jpg");
  expect(value.source).toStartWith("blob:");
  expect(reads[0]?.cache).toBe("no-store");
  expect((await originalFetch(value.source)).ok).toBe(true);
  value.dispose();
  expect(images[0].src).toBe("");
  // Use the real Blob URL implementation to check revocation.
  await expect(originalFetch(value.source)).rejects.toThrow();
});

test("failed decodes release their browser image resource", async () => {
  const { images } = installImage(true);
  await expect(decodeThumbnail("asset://broken.jpg")).rejects.toThrow("Bad image");
  expect(images[0].src).toBe("");
});

test("cancelled reads never create an image", async () => {
  const { images } = installImage();
  const controller = new AbortController();
  controller.abort();
  await expect(decodeThumbnail("asset://cancelled.jpg", controller.signal)).rejects.toThrow();
  expect(images).toHaveLength(0);
});
