process.argv = [process.argv[0], process.argv[1], "build"];
await import("./tauri-runtime.mjs");
