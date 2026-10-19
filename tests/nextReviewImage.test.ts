import { describe, expect, test } from "bun:test";
import type { MediaItem } from "../src/features/media/model/types";
import type { FileViewMediaPage } from "../src/features/media/services/mediaRepository";
import { nextReviewImage } from "../src/features/media/services/nextReviewImage";

function gallery(totalCount: number) {
  const offsets: number[] = [];
  const fetchPage = async (offset: number): Promise<FileViewMediaPage> => {
    offsets.push(offset);
    return {
      offset, limit: 500, totalCount, hasNextPage: offset + 500 < totalCount,
      items: Array.from({ length: Math.min(500, Math.max(0, totalCount - offset)) }, (_, index) => ({
        id: `image-${index + offset}`, path: `/${index + offset}.jpg`, mediaType: "image",
      } as MediaItem)),
    };
  };
  return { offsets, fetchPage };
}

describe("keyboard review advancement", () => {
  test("advances across a gallery page boundary", async () => {
    const { offsets, fetchPage } = gallery(1003);
    const next = await nextReviewImage(499, true, fetchPage);
    expect(next?.item.path).toBe("/500.jpg");
    expect(next?.index).toBe(500);
    expect(offsets).toEqual([500]);
  });

  test("does not skip the next image when a filter removes the classified image", async () => {
    const { fetchPage } = gallery(1002);
    const next = await nextReviewImage(499, false, fetchPage);
    expect(next?.index).toBe(499);
    expect(next?.item.path).toBe("/499.jpg");
  });

  test("keeps the last image selected when there is no next image", async () => {
    const { offsets, fetchPage } = gallery(500);
    const next = await nextReviewImage(499, true, fetchPage);
    expect(next?.index).toBe(499);
    expect(next?.item.path).toBe("/499.jpg");
    expect(offsets).toEqual([500, 0]);
  });

  test("selects the last remaining image when the final filtered image is removed", async () => {
    const { fetchPage } = gallery(499);
    const next = await nextReviewImage(499, false, fetchPage);
    expect(next?.index).toBe(498);
  });

  test("clears selection when no matching images remain", async () => {
    const { fetchPage } = gallery(0);
    expect(await nextReviewImage(0, false, fetchPage)).toBeNull();
  });

  test("reports a load failure instead of advancing to an unloaded image", async () => {
    await expect(nextReviewImage(499, true, async () => { throw new Error("Database unavailable"); }))
      .rejects.toThrow("Database unavailable");
  });
});
