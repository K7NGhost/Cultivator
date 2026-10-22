import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { VicClassificationControls, VicFilterSidebar } from "../src/features/media/components/VicClassification";
import { VIC_CATEGORIES, classificationTextColor, matchesVicFilter, toggleVicFilter, vicClassification } from "../src/features/media/model/vicClassification";

describe("Project VIC review", () => {
  test("CLIP suggestions filter independently without changing manual assignment choices", () => {
    const labels = [{ category: 1 }, { category: 7 }];
    expect(matchesVicFilter(0, [257], null, labels)).toBe(true);
    expect(matchesVicFilter(null, [263], null, labels)).toBe(true);
    expect(matchesVicFilter(0, [270], null, labels)).toBe(false);
    expect(matchesVicFilter(0, [0, 270], null, labels)).toBe(true);
    const counts = Array(273).fill(0); counts[0] = 3; counts[258] = 1; counts[264] = 2;
    const sidebar = renderToStaticMarkup(<VicFilterSidebar filter={[257]} counts={counts} onChange={() => {}} />);
    expect(sidebar).toContain("CLIP: Weapons &amp; firearms");
    expect(sidebar).toContain("CLIP: Media characteristics");
    expect(sidebar).toContain(">3<");
    const controls = renderToStaticMarkup(<VicClassificationControls category={0} onClassify={() => {}} />);
    expect(controls).not.toContain("CLIP:");
  });
  test("custom classifications resolve labels and combine with VIC filters", () => {
    const custom = [{ value: 5, label: "Needs review", color: "#123456" }];
    expect(vicClassification(5, custom)?.label).toBe("Needs review");
    expect(vicClassification(0, custom)?.label).toBe("Non-pertinent");
    expect(matchesVicFilter(5, [0, 5])).toBe(true);
    expect(matchesVicFilter(null, [5])).toBe(false);
    expect(classificationTextColor("#000000")).toBe("#ffffff");
    expect(classificationTextColor("#ffffff")).toBe("#000000");
    expect(classificationTextColor("#ef4444")).toBe("#000000");
  });

  test("custom classifications appear in assignment controls and counted filters", () => {
    const categories = [{ value: 5, label: "Needs review", color: "#123456" }];
    const controls = renderToStaticMarkup(<VicClassificationControls category={5} customCategories={categories} onClassify={() => {}} />);
    expect(controls).toContain("Needs review");
    expect(controls).toContain("#123456");
    expect(controls).toContain('aria-pressed="true"');
    const sidebar = renderToStaticMarkup(<VicFilterSidebar filter={[5]} counts={[11, 23, 4, 5, 6, 7, 3]} onChange={() => {}}
      custom={{ categories, disabled: false, onSave: async () => {}, onDelete: async () => {} }} />);
    expect(sidebar).toContain("Needs review");
    expect(sidebar).toContain(">59<");
    expect(sidebar).toContain("Manage custom classifications");
    expect(sidebar).toContain("Add classification");
    expect(sidebar).toContain('aria-label="Edit Needs review"');
    expect(sidebar).toContain('aria-label="Delete Needs review"');
    expect(sidebar).toContain('type="color"');
  });
  test("category zero stays distinct from an unclassified image", () => {
    expect(vicClassification(0)?.label).toBe("Non-pertinent");
    expect(vicClassification(null)).toBeUndefined();
    expect(matchesVicFilter(0, [-1])).toBe(false);
    expect(matchesVicFilter(null, [-1])).toBe(true);
    expect(matchesVicFilter(undefined, [0])).toBe(false);
    expect(matchesVicFilter(0, [0])).toBe(true);
  });

  test("filters combine categories and clear back to the full gallery", () => {
    let filter = toggleVicFilter([], 0);
    filter = toggleVicFilter(filter, 4);
    expect(matchesVicFilter(0, filter)).toBe(true);
    expect(matchesVicFilter(4, filter)).toBe(true);
    expect(matchesVicFilter(1, filter)).toBe(false);
    filter = toggleVicFilter(toggleVicFilter(filter, 0), 4);
    expect(filter).toEqual([]);
    expect(matchesVicFilter(null, filter)).toBe(true);
    expect(matchesVicFilter(2, filter)).toBe(true);
  });

  test("review controls expose category names, color, and a selected state", () => {
    const markup = renderToStaticMarkup(<VicClassificationControls category={0} onClassify={() => {}} />);
    for (const category of VIC_CATEGORIES) {
      expect(markup).toContain(category.label);
      expect(markup).toContain(category.color);
    }
    expect(markup).toContain('aria-pressed="true"');
    expect(markup).toContain("Clear classification");
  });

  test("sidebar counts distinguish unclassified from category zero", () => {
    const markup = renderToStaticMarkup(<VicFilterSidebar filter={[0]} counts={[11, 23, 4, 5, 6, 7]} onChange={() => {}} />);
    expect(markup).toContain("All images");
    expect(markup).toContain("Unclassified");
    expect(markup).toContain(">56<");
    expect(markup).toContain(">23<");
    expect(markup).toContain("checked");
  });
});
