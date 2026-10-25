// Export the local WebView trace when the Chrome connector cannot save to this
// workspace. This connects only to the already enabled localhost debug endpoint.
import { readFile, writeFile, access, mkdir } from "node:fs/promises";
import { dirname } from "node:path";
import { gzipSync } from "node:zlib";
import { setTimeout as delay } from "node:timers/promises";

const flags = Object.fromEntries(Array.from({length: (process.argv.length - 2) / 2}, (_, i) => process.argv.slice(2 + i * 2, 4 + i * 2)));
if (!flags["--port-file"] || !flags["--output"]) throw new Error("--port-file and --output are required");
const output = flags["--output"];
await mkdir(dirname(output), {recursive: true});
const port = (await readFile(flags["--port-file"], "utf8")).split(/\r?\n/)[0];
const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
const target = targets.find(t => t.type === "page" && t.url.startsWith("http://localhost:1420/"));
if (!target) throw new Error("Cultivator's local page was not found");
const socket = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((resolve, reject) => { socket.addEventListener("open", resolve, {once:true}); socket.addEventListener("error", reject, {once:true}); });
let nextId = 0;
const pending = new Map();
let complete;
const completed = new Promise(resolve => { complete = resolve; });
socket.addEventListener("message", event => {
  const message = JSON.parse(event.data);
  if (message.id && pending.has(message.id)) {
    const {resolve, reject} = pending.get(message.id);
    pending.delete(message.id);
    if (message.error) reject(new Error(JSON.stringify(message.error))); else resolve(message.result);
  }
  if (message.method === "Tracing.tracingComplete") complete(message.params);
});
function call(method, params = {}) {
  return new Promise((resolve, reject) => {
    const id = ++nextId;
    pending.set(id, {resolve, reject});
    socket.send(JSON.stringify({id, method, params}));
  });
}
const started = Date.now();
await call("Tracing.start", {transferMode:"ReturnAsStream", categories:"devtools.timeline,disabled-by-default-devtools.timeline,v8.execute,blink.user_timing,loading,disabled-by-default-v8.cpu_profiler"});
await writeFile(output + ".ready", new Date(started).toISOString());
console.log("Recording local Cultivator trace", {output, started:new Date(started).toISOString()});
const seconds = Number(flags["--seconds"] ?? 180);
while (Date.now() - started < seconds * 1000) {
  if (await access(output + ".stop").then(() => true, () => false)) break;
  await delay(250);
}
await call("Tracing.end");
const {stream, dataLossOccurred} = await completed;
const chunks = [];
while (true) {
  const result = await call("IO.read", {handle:stream, size:1024 * 1024});
  chunks.push(Buffer.from(result.data, result.base64Encoded ? "base64" : "utf8"));
  if (result.eof) break;
}
await call("IO.close", {handle:stream});
socket.close();
const raw = Buffer.concat(chunks);
await writeFile(output, gzipSync(raw));
const metadata = {url:target.url, startedUtc:new Date(started).toISOString(), finishedUtc:new Date().toISOString(), rawBytes:raw.length, dataLossOccurred:!!dataLossOccurred};
await writeFile(output + ".metadata.json", JSON.stringify(metadata, null, 2) + "\n");
console.log("Trace saved", metadata);
