import { describe, expect, test } from "bun:test";

import type { ReportData } from "../src/features/reports/reportData";
import { buildReportHtml, buildTaggedItemsCsv, csvField, escapeHtml, formatDate } from "../src/features/reports/reportHtml";

const hostile = `<script>alert("x")</script>`;

function data(): ReportData {
  return {
    generatedAt: new Date("2026-09-28T12:00:00Z"),
    caseRecord: {
      id: "case-1",
      name: `Case ${hostile}`,
      examiner: "A. Examiner",
      reference: "REF-1",
      description: "",
      folderPath: "C:/Cases/one",
      databasePath: "C:/Cases/one/case.sqlite",
      createdAt: "2026-09-01T10:00:00Z",
      updatedAt: "2026-09-01T10:00:00Z",
    },
    dataSources: [],
    counts: { hashedFiles: 2, messages: 1, calls: 0, contacts: 0, photos: 1, videos: 0 },
    tagged: [
      {
        tag: {
          id: "t1",
          filePath: "C:/Evidence/DCIM/IMG_0412.jpg",
          fileName: "IMG_0412.jpg",
          fileKind: "file",
          fileSize: 2048,
          tagName: "Bookmark",
          tagGroup: "bookmark",
          comment: hostile,
          createdAt: "",
          updatedAt: "",
        },
        hash: { filePath: "C:/Evidence/DCIM/IMG_0412.jpg", md5: "a".repeat(32), sha1: "b".repeat(40), sha256: "c".repeat(64), hashedAt: "" },
      },
    ],
    hashSetHits: [],
    hashSetCount: 0,
    mismatches: [{ name: "beach_2025.jpg", path: "C:/Evidence/Pictures/beach_2025.jpg" }],
    jobs: [],
    logs: [],
  };
}

describe("buildReportHtml", () => {
  const sections = { tagged: true, hashSetHits: true, mismatches: true, processingLog: true, photos: true };

  test("escapes evidence text so nothing in it can run", () => {
    const { html } = buildReportHtml(data(), sections, hostile, "Report_files");

    expect(html).not.toContain("<script>");
    expect(html).toContain(escapeHtml(hostile));
  });

  test("lists tagged items with their hashes and copies their photos", () => {
    const { html, attachments } = buildReportHtml(data(), sections, "", "Report_files");

    expect(html).toContain("c".repeat(64));
    expect(html).toContain("beach_2025.jpg");
    expect(attachments).toEqual([{ source: "C:/Evidence/DCIM/IMG_0412.jpg", name: "001-IMG_0412.jpg" }]);
    expect(html).toContain('src="Report_files/001-IMG_0412.jpg"');
  });
});

describe("CSV", () => {
  test("quotes separators and defuses spreadsheet formulas", () => {
    expect(csvField('a,"b"')).toBe('"a,""b"""');
    expect(csvField("=HYPERLINK(1)")).toBe("'=HYPERLINK(1)");
    expect(buildTaggedItemsCsv(data()).split("\r\n")[1]).toContain("Bookmark,bookmark");
  });
});

test("formats plugin times stored as epoch milliseconds", () => {
  expect(formatDate("1790564947009")).toContain("2026");
  expect(formatDate("not a date")).toBe("not a date");
});
