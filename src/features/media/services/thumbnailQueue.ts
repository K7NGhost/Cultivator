import { renderMediaThumbnail, updateMediaThumbnail, type ThumbnailPriority } from "./mediaRepository";

type Request = {
  id: string;
  users: Map<symbol, ThumbnailPriority>;
  priority: ThumbnailPriority;
  promise: Promise<string | null>;
  cancel: () => void;
  settled: boolean;
  started: boolean;
};
const pending = new Map<string, Request>();
const rank: Record<ThumbnailPriority, number> = { visible: 0, nearby: 1, background: 2 };
const dispatchQueue: Array<{ priority: () => ThumbnailPriority; run: () => void }> = [];
let dispatchTimer: ReturnType<typeof setTimeout> | undefined;
function dispatch(priority: () => ThumbnailPriority, run: () => void) {
  dispatchQueue.push({ priority, run });
  if (dispatchTimer !== undefined) return;
  // Collect the observers' notifications for this frame before sending IPC, so
  // leading overscan rows cannot occupy every native worker before visible rows.
  dispatchTimer = setTimeout(() => {
    dispatchTimer = undefined;
    dispatchQueue.splice(0).sort((a, b) => rank[a.priority()] - rank[b.priority()]).forEach((job) => job.run());
  }, 0);
}

// Rust owns scheduling and concurrency. Share consumers without caching paths
// here: every request must pass native source-fingerprint validation.
export function requestThumbnail(
  key: string, casePath: string, sourcePath: string, mediaId = sourcePath,
  priority: ThumbnailPriority = "visible",
) {
  let request = pending.get(key);
  if (!request) {
    const id = crypto.randomUUID();
    let cancel!: () => void;
    const cancelled = new Promise<null>((resolve) => { cancel = () => resolve(null); });
    const users = new Map<symbol, ThumbnailPriority>();
    request = { id, users, priority, cancel, settled: false, started: false, promise: Promise.resolve(null) };
    const current = request;
    const work = new Promise<string | null>((resolve, reject) => {
      dispatch(() => current.priority, () => {
        if (!users.size) { resolve(null); return; }
        current.started = true;
        void renderMediaThumbnail(casePath, sourcePath, mediaId, id, current.priority).then((result) => resolve(result.path), reject);
      });
    });
    current.promise = Promise.race([work, cancelled]).finally(() => {
      current.settled = true;
      if (pending.get(key) === current) pending.delete(key);
    });
    pending.set(key, current);
  }
  const current = request;
  const user = Symbol();
  current.users.set(user, priority);
  const update = () => {
    if (current.settled) return;
    const next = [...current.users.values()].sort((a, b) => rank[a] - rank[b])[0];
    if (!next) {
      if (pending.get(key) === current) pending.delete(key);
      current.cancel();
      if (current.started) void updateMediaThumbnail(current.id, null).catch((error: unknown) => console.error("Thumbnail cancellation failed", error));
    } else if (next !== current.priority) {
      current.priority = next;
      if (current.started) void updateMediaThumbnail(current.id, next).catch((error: unknown) => console.error("Thumbnail priority update failed", error));
    }
  };
  update();
  return {
    promise: current.promise,
    setPriority(next: ThumbnailPriority) {
      if (current.users.has(user)) { current.users.set(user, next); update(); }
    },
    release() { if (current.users.delete(user)) update(); },
  };
}
