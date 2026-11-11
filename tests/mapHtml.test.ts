import { describe, expect, test } from "bun:test";
import { createMapHtml, type MapHtmlAssets, type MapHtmlOptions } from "../src/features/mapping/export/createMapHtml";
import { filterMapDates } from "../src/features/mapping/lib/dateFilter";
import type { VisibleMapLayer } from "../src/features/mapping/types";

const assets: MapHtmlAssets = { library: "/* bundled map */", worker: "/* bundled worker */", mapCss: ".map{color:red}",
  license: "MapLibre BSD license", runtime: "console.log('map')", css: "body{margin:0}" };
function layer(): VisibleMapLayer {
  return { id:"kml", label:"KML layer", filePath:"/source.kml", datasourceId:"source", format:"kml", color:"#f97316",
    featureCount:2,positionCount:2,warnings:[],collection:{type:"FeatureCollection",features:[
      {type:"Feature",properties:{name:"First",time:"2026-11-10T00:00:00Z"},geometry:{type:"Point",coordinates:[179,10]}},
      {type:"Feature",properties:{name:"Second",time:"2026-11-11T00:00:00Z"},geometry:{type:"Point",coordinates:[-179,20]}},
    ]} };
}
function options(): MapHtmlOptions {
  return {title:"Case map",layers:[layer()],dates:{from:"",to:"",includeUndated:false},basemap:"streets",exportedAt:"2026-11-11T12:00:00Z"};
}
function embedded(html: string, id: string) {
  const text = html.match(new RegExp(`<script type="application/json" id="${id}">([\\s\\S]*?)<\\/script>`))?.[1];
  if (!text) throw new Error("Missing embedded data");
  return JSON.parse(text);
}

describe("portable HTML maps", () => {
  test("retains the selected timezone and time filter, with display conversions separate from raw timestamps", () => {
    const original=layer();
    const dates={from:"2026-11-10",to:"2026-11-10",fromTime:"19:00",toTime:"19:30",includeUndated:false};
    const view=filterMapDates([original],dates,"America/New_York");
    const data=embedded(createMapHtml({...options(),layers:view.layers,dates,timeZone:"America/New_York"},assets),"map-data");
    expect(data.timeZone).toBe("America/New_York");
    expect(data.dates).toEqual(dates);
    expect(data.collection.features).toHaveLength(1);
    expect(data.collection.features[0].properties.time).toBe("2026-11-11T00:00:00Z");
    expect(data.localTimes["2026-11-11T00:00:00Z"]).toBe("2026-11-10 19:00:00 America/New_York (UTC-05:00)");
    expect(original.collection.features).toHaveLength(2);
  });
  test("embeds geometry, properties, library, worker, license and date-line bounds", () => {
    const html=createMapHtml(options(),assets);
    const data=embedded(html,"map-data");
    expect(data.collection.features).toHaveLength(2);
    expect(data.collection.features[0].properties.name).toBe("First");
    expect(data.bounds).toEqual([[179,10],[181,20]]);
    expect(data.exportedAt).toBe("2026-11-11T12:00:00Z");
    expect(embedded(html,"map-assets")).toEqual({library:assets.library,worker:assets.worker,license:assets.license});
    expect(html).not.toMatch(/<script[^>]+src=/);
    expect(html).not.toMatch(/<link[^>]+href=/);
  });
  test("exports only the date-filtered snapshot and preserves original identities", () => {
    const original=layer(), dates={from:"2026-11-11",to:"2026-11-11",includeUndated:false};
    const view=filterMapDates([original],dates);
    const data=embedded(createMapHtml({...options(),layers:view.layers,dates},assets),"map-data");
    expect(data.collection.features).toHaveLength(1);
    expect(data.collection.features[0].properties.name).toBe("Second");
    expect(data.collection.features[0].properties._cvIndex).toBe(1);
    expect(data.dates).toEqual(dates);
    expect(original.collection.features).toHaveLength(2);
  });
  test("evidence text cannot escape embedded JSON or HTML title", () => {
    const value=options();
    value.title='</title><script>alert(1)</script>';
    value.layers[0].collection.features[0].properties!.description='</script><img src=x onerror=alert(1)>\u2028';
    value.layers[0].label='</script><script>alert(2)</script>';
    const html=createMapHtml(value,assets);
    expect(html).not.toContain('<script>alert');
    expect(html).not.toContain('<img src=x');
    expect(html).toContain('&lt;/title&gt;');
    const data=embedded(html,"map-data");
    expect(data.layers[0].label).toBe(value.layers[0].label);
    expect(data.collection.features[0].properties.description).toBe(value.layers[0].collection.features[0].properties!.description);
  });
  test("does not create an empty report", () => {
    expect(()=>createMapHtml({...options(),layers:[]},assets)).toThrow("no visible features");
  });
});
