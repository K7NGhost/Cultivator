import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { ChatAnswer } from "../src/features/chat/ChatAnswer";

test("chat references only supplied evidence and renders model HTML as inert text", () => {
  const source = { id: "E1", group: "1:row:0", stage: "1", source: "/messages.db", body: "Phone paired", chunk: 0, methods: ["Tantivy", "SQLite"] };
  const html = renderToStaticMarkup(<ChatAnswer text={'Paired phone [E1]. Unknown [E9]. <img src=x onerror=alert(1)>'} sources={[source]} select={() => {}} />);
  expect(html).toContain("<button");
  expect(html).toContain("/messages.db");
  expect(html).toContain("[E9]");
  expect(html).toContain("&lt;img");
  expect(html).not.toContain("<img");
});
