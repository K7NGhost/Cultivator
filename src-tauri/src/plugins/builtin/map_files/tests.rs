use super::*;
use serde_json::json;

#[test]
fn kml_preserves_namespaces_holes_altitude_and_properties() {
    let input = br#"<kml xmlns="http://www.opengis.net/kml/2.2"><Document>
      <Placemark id="p1"><name>A &amp; B</name><description><![CDATA[<b>Original</b>]]></description>
        <ExtendedData><Data name="owner"><value>Examiner</value></Data></ExtendedData>
        <MultiGeometry><Point><coordinates>-73,40,12</coordinates></Point>
        <Polygon><outerBoundaryIs><LinearRing><coordinates>0,0 4,0 4,4 0,0</coordinates></LinearRing></outerBoundaryIs>
        <innerBoundaryIs><LinearRing><coordinates>1,1 2,1 2,2 1,1</coordinates></LinearRing></innerBoundaryIs></Polygon>
        </MultiGeometry></Placemark></Document></kml>"#;
    let layer = parse(input, "kml", &|| false).unwrap();
    assert_eq!(layer.features.len(), 1);
    assert_eq!(layer.features[0]["properties"]["name"], "A & B");
    assert_eq!(
        layer.features[0]["properties"]["description"],
        "<b>Original</b>"
    );
    assert_eq!(
        layer.features[0]["properties"]["extendedData"]["owner"],
        "Examiner"
    );
    assert_eq!(
        layer.features[0]["geometry"]["geometries"][0]["coordinates"],
        json!([-73.0, 40.0, 12.0])
    );
    assert_eq!(
        layer.features[0]["geometry"]["geometries"][1]["coordinates"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn gpx_keeps_track_segments_separate_and_preserves_point_times() {
    let input = br#"<gpx version="1.1"><wpt lat="40" lon="-73"><name>Home</name><ele>12</ele></wpt>
      <trk><name>Walk</name><trkseg><trkpt lat="40" lon="-73"><time>2026-01-01T00:00:00Z</time></trkpt>
      <trkpt lat="41" lon="-73"/></trkseg><trkseg><trkpt lat="42" lon="-73"/>
      <trkpt lat="43" lon="-73"/></trkseg></trk></gpx>"#;
    let layer = parse(input, "gpx", &|| false).unwrap();
    assert_eq!(layer.features.len(), 2);
    assert_eq!(
        layer.features[0]["geometry"]["coordinates"],
        json!([-73.0, 40.0, 12.0])
    );
    assert_eq!(layer.features[1]["geometry"]["type"], "MultiLineString");
    assert_eq!(
        layer.features[1]["geometry"]["coordinates"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        layer.features[1]["properties"]["coordinateTimes"][0][0],
        "2026-01-01T00:00:00Z"
    );
    assert_eq!(layer.positions, 5);
}

#[test]
fn retains_single_position_gpx_segments_and_kml_track_times() {
    let gpx = parse(br#"<gpx><rte><rtept lat="1" lon="2"/></rte><trk><trkseg/><trkseg><trkpt lat="3" lon="4"/></trkseg></trk></gpx>"#,
        "gpx", &|| false).unwrap();
    assert_eq!(gpx.features.len(), 2);
    assert_eq!(gpx.features[0]["geometry"]["type"], "Point");
    assert_eq!(
        gpx.features[1]["geometry"]["geometries"][0]["type"],
        "Point"
    );
    let kml = parse(
        br#"<kml xmlns:gx="http://www.google.com/kml/ext/2.2"><Placemark><gx:Track>
      <when>2026-01-01T00:00:00Z</when><gx:coord>1 2 3</gx:coord>
      <when>2026-01-01T00:01:00Z</when><gx:coord>2 3 4</gx:coord>
    </gx:Track></Placemark></kml>"#,
        "kml",
        &|| false,
    )
    .unwrap();
    assert_eq!(
        kml.features[0]["properties"]["coordinateTimes"][1],
        "2026-01-01T00:01:00Z"
    );
}

#[test]
fn geojson_supports_every_geometry_and_preserves_source_properties() {
    let input = json!({"type":"FeatureCollection", "features":[
        {"type":"Feature","id":"source-id","properties":{"name":"Point", "nested":{"n":1}},"geometry":{"type":"Point","coordinates":[1,2]}},
        {"type":"Feature","properties":null,"geometry":{"type":"GeometryCollection","geometries":[
            {"type":"MultiPoint","coordinates":[[1,2],[3,4]]},
            {"type":"MultiPolygon","coordinates":[[[[0,0],[1,0],[1,1],[0,0]]]]}
        ]}}
    ]});
    let layer = parse(&serde_json::to_vec(&input).unwrap(), "geojson", &|| false).unwrap();
    assert_eq!(layer.features[0], input["features"][0]);
    assert_eq!(layer.features[1]["properties"], json!({}));
    assert_eq!(layer.bounds, Some([0.0, 0.0, 3.0, 4.0]));
}

#[test]
fn malformed_geometry_is_reported_without_losing_valid_features() {
    let input = br#"{"type":"FeatureCollection","features":[
      {"type":"Feature","properties":{},"geometry":{"type":"Point","coordinates":[10,95]}},
      {"type":"Feature","properties":{"name":"good"},"geometry":{"type":"Point","coordinates":[10,45]}}
    ]}"#;
    let layer = parse(input, "geojson", &|| false).unwrap();
    assert_eq!(layer.features.len(), 1);
    assert_eq!(layer.warnings.len(), 1);
    assert!(parse(
        br#"{"type":"Point","coordinates":[500,0]}"#,
        "geojson",
        &|| false
    )
    .is_err());
    assert!(parse(
        br#"{"type":"FeatureCollection","crs":{"properties":{"name":"EPSG:3857"}},"features":[]}"#,
        "geojson",
        &|| false
    )
    .is_err());
}

#[test]
fn rejects_malformed_xml_external_entities_and_honors_cancellation() {
    assert!(parse(b"<gpx><wpt></gpx>", "gpx", &|| false).is_err());
    assert!(parse(
        br#"<!DOCTYPE kml SYSTEM "file:///secret"><kml/>"#,
        "kml",
        &|| false
    )
    .is_err());
    assert!(parse(b"<kml/>", "gpx", &|| false).is_err());
    assert_eq!(
        parse(b"<kml/>", "kml", &|| true).err().unwrap(),
        "Plugin run cancelled."
    );
}

#[test]
fn plugin_keeps_good_files_when_another_file_fails() {
    let source = tempfile::tempdir().unwrap();
    fs::write(
        source.path().join("good.geojson"),
        br#"{"type":"Point","coordinates":[1,2]}"#,
    )
    .unwrap();
    fs::write(source.path().join("bad.kml"), "invalid").unwrap();
    fs::write(source.path().join("unrelated.txt"), "ignore").unwrap();
    let mut progress = |_| {};
    let mut context = BuiltinPluginContext::new(
        vec![source.path().to_string_lossy().into_owned()],
        None,
        json!({}),
        vec![],
        &mut progress,
        &|| false,
    );
    MapFiles.run(&mut context).unwrap();
    let effects = context.into_effects();
    assert_eq!(effects.artifacts.unwrap().len(), 1);
    assert!(effects
        .logs
        .iter()
        .any(|(level, message)| level == "warn" && message.contains("bad.kml")));
    assert!(super::super::plugin("map-files").is_some());
}

#[test]
fn reads_deferred_map_sources_without_extracting_other_evidence() {
    let source = tempfile::tempdir().unwrap();
    let case = tempfile::tempdir().unwrap();
    fs::write(
        source.path().join("places.geojson"),
        br#"{"type":"Point","coordinates":[-73,40]}"#,
    )
    .unwrap();
    fs::write(source.path().join("unrelated.txt"), "leave in evidence").unwrap();
    let image = case.path().join("map.inodex");
    inodex::acquisition::acquire(source.path(), &image, inodex::filesystem::Limits::default())
        .unwrap();
    let imported = crate::evidence::import_for_test(crate::evidence::ImportRequest {
        case_folder_path: case.path().to_string_lossy().into_owned(),
        datasource_id: "map-fixture".into(),
        source_type: "diskImage".into(),
        paths: vec![image.to_string_lossy().into_owned()],
        partition: None,
    })
    .unwrap();
    let root = Path::new(&imported.paths[0]);
    assert!(!root.join("places.geojson").exists());
    let mut progress = |_| {};
    let mut context = BuiltinPluginContext::new(
        imported.paths.clone(),
        None,
        json!({}),
        vec![],
        &mut progress,
        &|| false,
    );
    MapFiles.run(&mut context).unwrap();
    assert_eq!(context.into_effects().artifacts.unwrap().len(), 1);
    assert!(root.join("places.geojson").exists());
    assert!(!root.join("unrelated.txt").exists());
}
