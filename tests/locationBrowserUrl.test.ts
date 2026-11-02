import { expect, test } from "bun:test";
import { locationBrowserUrl } from "../src/lib/maps/locationUrl";

test("browser map URLs retain selected coordinate precision and correct latitude/longitude order", () => {
  const url = new URL(locationBrowserUrl(37.27929687, -77.40930938)!);
  expect(url.origin).toBe("https://www.google.com");
  expect(url.pathname).toBe("/maps/search/");
  expect(url.searchParams.get("api")).toBe("1");
  expect(url.searchParams.get("query")).toBe("37.27929687,-77.40930938");
  expect(new URL(locationBrowserUrl(-33.86, 151.2)!).searchParams.get("query")).toBe("-33.86,151.2");
});

test("missing and invalid positions cannot open a bogus location", () => {
  for (const [latitude, longitude] of [[null, 1], [1, undefined], [NaN, 1], [1, Infinity],
    [91, 1], [1, -181], [0, 0]] as const) {
    expect(locationBrowserUrl(latitude, longitude)).toBeNull();
  }
  expect(locationBrowserUrl(0, 10)).not.toBeNull();
  expect(locationBrowserUrl(10, 0)).not.toBeNull();
});
