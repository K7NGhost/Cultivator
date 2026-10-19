import type { FileViewMediaPage } from "./mediaRepository";

// Query after saving: a classification filter may remove the reviewed image,
// shifting the next image into its former index, including across page borders.
export async function nextReviewImage(
  currentIndex: number,
  currentStillVisible: boolean,
  fetchPage: (offset: number) => Promise<FileViewMediaPage>,
) {
  const wanted = currentIndex + (currentStillVisible ? 1 : 0);
  let page = await fetchPage(Math.floor(wanted / 500) * 500);
  if (!page.totalCount) return null;
  const index = Math.min(wanted, page.totalCount - 1);
  if (index < page.offset) page = await fetchPage(Math.floor(index / 500) * 500);
  const item = page.items[index - page.offset];
  if (!item) throw new Error("The next image could not be loaded. Refresh the gallery and retry.");
  return { item, index };
}
