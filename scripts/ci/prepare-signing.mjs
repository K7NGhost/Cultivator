import { mkdirSync, writeFileSync } from "node:fs";

mkdirSync(".cache", { recursive: true });
writeFileSync(".cache/release-signing.json", "{}\n");

if (process.platform === "darwin") {
  const certificate = process.env.APPLE_CERTIFICATE;
  const password = process.env.APPLE_CERTIFICATE_PASSWORD;
  const identity = process.env.APPLE_SIGNING_IDENTITY;
  if ([certificate, password, identity].some(Boolean)) {
    if (![certificate, password, identity].every(Boolean)) {
      throw new Error("Configure APPLE_CERTIFICATE, APPLE_CERTIFICATE_PASSWORD, and APPLE_SIGNING_IDENTITY together.");
    }
  } else {
    console.log("No Apple certificate configured; building unsigned bundles.");
  }
  const notarization = [
    process.env.APPLE_ID,
    process.env.APPLE_PASSWORD,
    process.env.APPLE_TEAM_ID,
  ];
  if (notarization.some(Boolean) && (!notarization.every(Boolean) || !identity)) {
    throw new Error("Notarization requires signing credentials plus APPLE_ID, APPLE_PASSWORD, and APPLE_TEAM_ID.");
  }
}
