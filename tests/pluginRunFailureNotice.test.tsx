import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { PluginRunFailureNotice } from "../src/features/plugins/components/PluginRunFailureNotice";

test("failed extraction notices show the datasource, plugin, member and underlying reason", () => {
  const html = renderToStaticMarkup(
    <PluginRunFailureNotice
      datasourceName="samsung-10gb"
      failures={[{
        jobId: "job",
        pluginName: "Archive Extractor",
        error: "Failed to extract Android 11.zip: ZIP decoder panicked: attempt to add with overflow",
      }]}
    />,
  );
  expect(html).toContain("samsung-10gb");
  expect(html).toContain("Archive Extractor");
  expect(html).toContain("Android 11.zip");
  expect(html).toContain("attempt to add with overflow");
});

test("missing backend errors have an explicit fallback", () => {
  const html = renderToStaticMarkup(
    <PluginRunFailureNotice
      datasourceName="phone"
      failures={[{ jobId: "job", pluginName: "Archive Extractor", error: null }]}
    />,
  );
  expect(html).toContain("The plugin stopped without recording an error message.");
});
