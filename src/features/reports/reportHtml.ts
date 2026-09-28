import type { ReportData, ReportSections } from "@/features/reports/reportData";

export type ReportAttachment = { source: string; name: string };

const IMAGE_EXTENSIONS = new Set(["jpg", "jpeg", "png", "gif", "webp", "bmp"]);
const APP_VERSION = "0.1.0";

/** Evidence text can contain anything, including markup: always escape it. */
export function escapeHtml(value: unknown) {
  return String(value ?? "")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

/**
 * One CSV field. Values starting with = + - @ are prefixed so a spreadsheet
 * never runs them as formulas (a file name in evidence can be hostile).
 */
export function csvField(value: unknown) {
  let text = String(value ?? "");
  if (/^[=+\-@\t\r]/.test(text)) {
    text = `'${text}`;
  }
  return /[",\r\n]/.test(text) ? `"${text.replace(/"/g, '""')}"` : text;
}

/** The report page, and the tagged photos to copy into `<report>_files`. */
export function buildReportHtml(
  data: ReportData,
  sections: ReportSections,
  notes: string,
  attachmentsFolder: string,
): { html: string; attachments: ReportAttachment[] } {
  const attachments: ReportAttachment[] = [];
  const { caseRecord } = data;
  const rows = (cells: unknown[][]) =>
    cells.map((row) => `<tr>${row.map((cell) => `<td>${cell}</td>`).join("")}</tr>`).join("");
  const table = (headers: string[], body: string) =>
    `<table><thead><tr>${headers.map((header) => `<th>${header}</th>`).join("")}</tr></thead><tbody>${body}</tbody></table>`;
  const section = (title: string, content: string) => `<section><h2>${title}</h2>${content}</section>`;
  const none = (text: string) => `<p class="muted">${text}</p>`;

  const parts = [
    `<header><h1>Case report: ${escapeHtml(caseRecord.name)}</h1>
      <p class="muted">Generated ${escapeHtml(data.generatedAt.toLocaleString())} (${escapeHtml(data.generatedAt.toISOString())}) by Cultivator ${APP_VERSION}</p></header>`,
    section(
      "Case",
      table(
        ["Field", "Value"],
        rows([
          ["Case name", escapeHtml(caseRecord.name)],
          ["Reference", escapeHtml(caseRecord.reference || "-")],
          ["Examiner", escapeHtml(caseRecord.examiner || "-")],
          ["Description", escapeHtml(caseRecord.description || "-")],
          ["Created", escapeHtml(formatDate(caseRecord.createdAt))],
          ["Case folder", `<code>${escapeHtml(caseRecord.folderPath)}</code>`],
        ]),
      ),
    ),
  ];

  if (notes.trim()) {
    parts.push(section("Examiner notes", `<p class="notes">${escapeHtml(notes.trim())}</p>`));
  }

  parts.push(
    section(
      "Data sources",
      data.dataSources.length === 0
        ? none("No data sources.")
        : table(
            ["Name", "Type", "Location", "Added"],
            rows(
              data.dataSources.map((source) => [
                escapeHtml(source.name),
                escapeHtml(source.type),
                (source.paths.length > 0 ? source.paths : [source.path])
                  .map((path) => `<code>${escapeHtml(path)}</code>`)
                  .join("<br>"),
                escapeHtml(formatDate(source.createdAt)),
              ]),
            ),
          ),
    ),
    section(
      "Summary",
      table(
        ["Item", "Count"],
        rows([
          ["Files hashed (MD5, SHA-1, SHA-256)", data.counts.hashedFiles],
          ["Messages", data.counts.messages],
          ["Calls", data.counts.calls],
          ["Contacts", data.counts.contacts],
          ["Photos", data.counts.photos],
          ["Videos", data.counts.videos],
          ["Tagged items", data.tagged.length],
          ["Hash set hits", `${data.hashSetHits.length} (${data.hashSetCount} hash sets checked)`],
          ["Extension mismatches", data.mismatches.length],
        ].map(([label, value]) => [escapeHtml(label), escapeHtml(typeof value === "number" ? value.toLocaleString() : value)])),
      ),
    ),
  );

  if (sections.tagged) {
    const byTag = new Map<string, typeof data.tagged>();
    for (const item of data.tagged) {
      byTag.set(item.tag.tagName, [...(byTag.get(item.tag.tagName) ?? []), item]);
    }
    const groups = [...byTag.entries()].map(([tagName, items]) => {
      const body = rows(
        items.map(({ tag, hash }) => {
          const extension = tag.fileName.split(".").pop()?.toLowerCase() ?? "";
          let picture = "";
          if (sections.photos && IMAGE_EXTENSIONS.has(extension)) {
            const name = `${String(attachments.length + 1).padStart(3, "0")}-${safeName(tag.fileName)}`;
            attachments.push({ source: tag.filePath, name });
            picture = `<img src="${escapeHtml(`${attachmentsFolder}/${name}`)}" alt="${escapeHtml(tag.fileName)}">`;
          }
          return [
            `${picture}<strong>${escapeHtml(tag.fileName)}</strong><br><code>${escapeHtml(tag.filePath)}</code>`,
            escapeHtml(formatBytes(tag.fileSize)),
            escapeHtml(tag.fileModifiedMs ? new Date(tag.fileModifiedMs).toLocaleString() : "-"),
            escapeHtml(tag.comment || "-"),
            hash
              ? `<code>MD5 ${escapeHtml(hash.md5)}<br>SHA-1 ${escapeHtml(hash.sha1)}<br>SHA-256 ${escapeHtml(hash.sha256)}</code>`
              : `<span class="muted">Not hashed</span>`,
          ];
        }),
      );
      return `<h3>${escapeHtml(tagName)} (${items.length})</h3>${table(["File", "Size", "Modified", "Note", "Hashes"], body)}`;
    });
    parts.push(section(`Tagged items (${data.tagged.length})`, groups.join("") || none("Nothing is tagged yet.")));
  }

  if (sections.hashSetHits) {
    parts.push(
      section(
        `Hash set hits (${data.hashSetHits.length})`,
        data.hashSetHits.length === 0
          ? none(data.hashSetCount === 0 ? "No hash sets are imported." : "No files match the imported hash sets.")
          : table(
              ["Hash set", "Kind", "File", "Matching hash"],
              rows(
                data.hashSetHits.map((hit) => [
                  escapeHtml(hit.setName),
                  escapeHtml(hit.kind === "known-bad" ? "Known bad" : "Known good"),
                  `<code>${escapeHtml(hit.filePath)}</code>`,
                  `<code>${escapeHtml(hit.hash)}</code>`,
                ]),
              ),
            ),
      ),
    );
  }

  if (sections.mismatches) {
    parts.push(
      section(
        `Extension mismatches (${data.mismatches.length})`,
        data.mismatches.length === 0
          ? none("No file's content contradicts its extension.")
          : table(
              ["File", "Location"],
              rows(data.mismatches.map((file) => [escapeHtml(file.name), `<code>${escapeHtml(file.path)}</code>`])),
            ),
      ),
    );
  }

  if (sections.processingLog) {
    parts.push(
      section(
        "Processing log",
        (data.jobs.length === 0
          ? none("No plugins have run.")
          : table(
              ["Plugin", "Status", "Started", "Finished", "Error"],
              rows(
                data.jobs.map((job) => [
                  escapeHtml(job.pluginId),
                  escapeHtml(job.status),
                  escapeHtml(formatDate(job.startedAt)),
                  escapeHtml(job.finishedAt ? formatDate(job.finishedAt) : "-"),
                  escapeHtml(job.error ?? ""),
                ]),
              ),
            )) +
          (data.logs.length === 0
            ? ""
            : `<h3>Plugin messages</h3>${table(
                ["Time", "Plugin", "Level", "Message"],
                rows(
                  data.logs.map((log) => [
                    escapeHtml(formatDate(log.createdAt)),
                    escapeHtml(log.pluginId),
                    escapeHtml(log.level),
                    escapeHtml(log.message),
                  ]),
                ),
              )}`),
      ),
    );
  }

  parts.push(`<footer>
    <p>Hashes were computed by Cultivator's Hash Calculator plugin. Evidence was read without modification;
    SQLite databases were opened from private working copies. AI results (captions, meaning search) were
    produced on this computer with pinned model versions recorded in the processing log.</p>
    <p>Cultivator shows this report's SHA-256 when it is saved; record it to show the report hasn't changed.</p>
  </footer>`);

  const html = `<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Case report: ${escapeHtml(caseRecord.name)}</title>
<style>
  body { font: 13px/1.45 system-ui, -apple-system, "Segoe UI", sans-serif; color: #1f2328; margin: 32px auto; max-width: 1100px; padding: 0 16px; }
  h1 { font-size: 22px; margin: 0 0 4px; } h2 { font-size: 16px; border-bottom: 1px solid #d0d7de; padding-bottom: 4px; margin-top: 28px; }
  h3 { font-size: 13px; margin: 16px 0 6px; } .muted { color: #656d76; } .notes { white-space: pre-wrap; }
  table { border-collapse: collapse; width: 100%; } th, td { border: 1px solid #d0d7de; padding: 4px 6px; text-align: left; vertical-align: top; }
  th { background: #f6f8fa; } code { font: 11px ui-monospace, Consolas, monospace; word-break: break-all; }
  img { display: block; max-width: 180px; max-height: 140px; margin-bottom: 4px; border: 1px solid #d0d7de; }
  footer { margin-top: 32px; font-size: 11px; color: #656d76; border-top: 1px solid #d0d7de; }
  @media print { body { margin: 0; } section { break-inside: auto; } tr { break-inside: avoid; } }
</style></head><body>
${parts.join("\n")}
</body></html>`;

  return { html, attachments };
}

/** The tagged items as a spreadsheet. */
export function buildTaggedItemsCsv(data: ReportData) {
  const header = ["Tag", "Tag group", "Note", "File name", "Path", "Size (bytes)", "Modified", "MD5", "SHA-1", "SHA-256"];
  const lines = data.tagged.map(({ tag, hash }) =>
    [
      tag.tagName,
      tag.tagGroup,
      tag.comment,
      tag.fileName,
      tag.filePath,
      tag.fileSize ?? "",
      tag.fileModifiedMs ? new Date(tag.fileModifiedMs).toISOString() : "",
      hash?.md5 ?? "",
      hash?.sha1 ?? "",
      hash?.sha256 ?? "",
    ]
      .map(csvField)
      .join(","),
  );
  return [header.map(csvField).join(","), ...lines].join("\r\n") + "\r\n";
}

function safeName(name: string) {
  return name.replace(/[^A-Za-z0-9._-]/g, "_").slice(0, 80) || "file";
}

/** ISO text, or epoch milliseconds as text (how plugin jobs and logs store times). */
export function formatDate(value: string) {
  const date = new Date(/^\d+$/.test(value) ? Number(value) : value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString(undefined, { timeZoneName: "short" });
}

function formatBytes(size?: number) {
  if (size === undefined) return "-";
  const units = ["B", "KB", "MB", "GB"];
  let value = size;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(unit === 0 ? 0 : 1)} ${units[unit]} (${size.toLocaleString()} bytes)`;
}
