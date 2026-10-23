// IDs 0–4 are reserved for VIC; case-specific categories use IDs 5 and above.
export type VicCategory = number;
export type VicFilterValue = VicCategory | -1;
export type ClassificationDefinition = { value: number; label: string; color: string };

// Category numbers and names follow the Project VIC 2.0 review scheme.
export const VIC_CATEGORIES: ReadonlyArray<ClassificationDefinition> = [
  { value: 0, label: "Non-pertinent", color: "#84cc16" },
  { value: 1, label: "CAM (Child Abuse Material)", color: "#ef4444" },
  { value: 2, label: "Child Exploitative / Age Difficult", color: "#eab308" },
  { value: 3, label: "CGI / Animation (Child Exploitative)", color: "#a855f7" },
  { value: 4, label: "Comparison Images (Non-pertinent)", color: "#3b82f6" },
];
export function vicClassification(category?: VicCategory | null, custom: ReadonlyArray<ClassificationDefinition> = []) {
  return VIC_CATEGORIES.find((entry) => entry.value === category) ?? custom.find((entry) => entry.value === category);
}
export function classificationTextColor(color: string) {
  const [red, green, blue] = [1, 3, 5].map((offset) => {
    const channel = parseInt(color.slice(offset, offset + 2), 16) / 255;
    return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
  });
  return red * 0.2126 + green * 0.7152 + blue * 0.0722 > 0.179 ? "#000000" : "#ffffff";
}
export const NSFW_FILTERS = [
  { value: 272, label: "Flagged" },
] as const;
export function nsfwFilterValue(score?: number | null) {
  return typeof score === "number" && Number.isFinite(score) && score >= 0 && score <= 1
    ? score >= 0.5 ? 272 : 273 : 274;
}
export function matchesVicFilter(category: VicCategory | null | undefined, filter: VicFilterValue[], faceCount?: number | null, clipClassifications?: Array<{ category: number }>, nsfwScore?: number | null) {
  return !filter.length || filter.includes(category ?? -1) || (filter.includes(256) && (faceCount ?? 0) > 0)
    || !!clipClassifications?.some(match => filter.includes(256 + match.category))
    || filter.includes(nsfwFilterValue(nsfwScore));
}
export function toggleVicFilter(filter: VicFilterValue[], category: VicFilterValue) {
  return filter.includes(category) ? filter.filter((value) => value !== category) : [...filter, category];
}
