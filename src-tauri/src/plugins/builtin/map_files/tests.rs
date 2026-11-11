use super::*;
use serde_json::json;

#[test]
fn kml_description_exposes_recorded_event_speed_and_retains_source() {
    let description = r#"<![CDATA[<table><tr><th align="left">Identifier</th><td>Traction Event: Velocity Indicator speed = 10.7 MPH (17.2 KPH), Acceleration = 2.5 g (implied)</td></tr><tr><th>Bearing</th><td>90&#176;</td></tr></table>"#;
    let input = format!(
        r#"<kml><Placemark><description><![CDATA[{description}]]></description><Point><coordinates>1,2,NaN</coordinates></Point></Placemark></kml>"#
    );
    let layer = parse(input.as_bytes(), "kml", &|| false).unwrap();
    let properties = &layer.features[0]["properties"];
    assert_eq!(properties["speed"], "10.7 MPH (17.2 KPH)");
    assert_eq!(properties["description"], description);
    assert_eq!(properties["descriptionFields"]["Bearing"], "90°");
    assert!(properties.get("speedLimit").is_none());
}

#[test]
fn kml_speed_limits_and_undocumented_track_speeds_stay_distinct() {
    let layer = parse(br#"<kml>
      <Placemark><description><![CDATA[<table><tr><th>Identifier</th><td>Speed Limit : 25</td></tr></table>]]></description><Point><coordinates>1,2</coordinates></Point></Placemark>
      <Placemark><description><![CDATA[<table><tr><th>Time</th><td>9/22/2020 4:42:00 AM</td></tr><tr><th>Bearing</th><td>0</td></tr><tr><th>Distance</th><td>12 m</td></tr></table>]]></description><Point><coordinates>1,2</coordinates></Point></Placemark>
    </kml>"#, "kml", &|| false).unwrap();
    assert_eq!(layer.features[0]["properties"]["speedLimit"], "25");
    assert!(layer.features[0]["properties"].get("speed").is_none());
    assert!(layer.features[1]["properties"].get("speed").is_none());
    assert_eq!(
        layer.features[1]["properties"]["descriptionFields"]["Distance"],
        "12 m"
    );
}

#[test]
fn kml_description_table_fields_are_text_with_units_and_duplicate_rows_preserved() {
    let input = br#"<kml><Placemark><description><![CDATA[<TABLE><TR><TD><b>Speed</b></TD><TD>0&nbsp;km/h</TD></TR><tr><th>Speed Limit</th><td>30 MPH</td></tr><tr><th>Note</th><td>A &amp; B &lt;img src=x onerror=alert(1)&gt;</td></tr><tr><th>Note</th><td>Second</td></tr></TABLE>]]></description><Point><coordinates>1,2</coordinates></Point></Placemark></kml>"#;
    let layer = parse(input, "kml", &|| false).unwrap();
    let properties = &layer.features[0]["properties"];
    assert_eq!(properties["speed"], "0 km/h");
    assert_eq!(properties["speedLimit"], "30 MPH");
    assert_eq!(
        properties["descriptionFields"]["Note"],
        json!(["A & B <img src=x onerror=alert(1)>", "Second"])
    );
}

#[test]
fn track_log_retains_locations_with_unknown_elevation_and_their_folder_and_times() {
    let layer=parse(br#"<kml><Document><name>Recording</name><Folder><name>Track Log</name>
      <Placemark><name>Sample</name><TimeStamp><when>2020-09-22T04:41:58-04:00</when></TimeStamp><Point><coordinates>-73,40,NaN</coordinates></Point></Placemark>
      <Placemark><Point><coordinates>-74,41,</coordinates></Point></Placemark>
      <Placemark><Point><coordinates>-75,42,Infinity</coordinates></Point></Placemark>
      <Placemark><Point><coordinates>-76,43,0</coordinates></Point></Placemark>
      <Placemark><Point><coordinates>-77,44</coordinates></Point></Placemark>
    </Folder></Document></kml>"#,"kml",&||false).unwrap();
    assert_eq!(layer.features.len(), 5);
    assert_eq!(
        layer.features[0]["geometry"]["coordinates"],
        json!([-73.0, 40.0])
    );
    assert_eq!(layer.features[0]["properties"]["sourceAltitude"], "NaN");
    assert_eq!(layer.features[0]["properties"]["omittedAltitudeCount"], 1);
    assert_eq!(
        layer.features[0]["properties"]["time"],
        "2020-09-22T04:41:58-04:00"
    );
    assert_eq!(
        layer.features[0]["properties"]["folderPath"],
        json!(["Recording", "Track Log"])
    );
    assert_eq!(
        layer.features[3]["geometry"]["coordinates"],
        json!([-76.0, 43.0, 0.0])
    );
    assert!(layer.features[4]["properties"]
        .get("omittedAltitudeCount")
        .is_none());
    assert_eq!(layer.warnings.len(), 1);
    assert!(layer.warnings[0].contains("3 KML coordinates"));
}

#[test]
fn unknown_altitude_does_not_allow_invalid_horizontal_positions() {
    let layer = parse(
        br#"<kml><Folder><name>Track Log</name>
      <Placemark><Point><coordinates>NaN,40,NaN</coordinates></Point></Placemark>
      <Placemark><Point><coordinates>-73,95,NaN</coordinates></Point></Placemark>
      <Placemark><Point><coordinates>-73,40,NaN</coordinates></Point></Placemark>
    </Folder></kml>"#,
        "kml",
        &|| false,
    )
    .unwrap();
    assert_eq!(layer.features.len(), 1);
    assert_eq!(layer.positions, 1);
    assert_eq!(layer.bounds, Some([-73.0, 40.0, -73.0, 40.0]));
    assert!(layer
        .warnings
        .iter()
        .any(|warning| warning.contains("Invalid WGS84")));
}

#[test]
fn kml_multitrack_and_nested_geometry_preserve_separate_tracks_and_aligned_times() {
    let layer=parse(br#"<kml xmlns:gx="http://www.google.com/kml/ext/2.2"><Folder><name>Track Log</name><Placemark><name>Trip</name>
      <MultiGeometry><Point><coordinates>0,0</coordinates></Point><gx:MultiTrack>
        <gx:Track><when>2026-11-11T10:00:00Z</when><when>2026-11-11T10:01:00Z</when><gx:coord>1 2 NaN</gx:coord><gx:coord>2 3 4</gx:coord></gx:Track>
        <gx:Track><when>2026-11-12T12:00:00Z</when><when>2026-11-12T12:01:00Z</when><gx:coord>10 20 5</gx:coord><gx:coord>11 21 6</gx:coord></gx:Track>
      </gx:MultiTrack></MultiGeometry></Placemark></Folder></kml>"#,"kml",&||false).unwrap();
    assert_eq!(layer.features.len(), 1);
    assert_eq!(
        layer.features[0]["properties"]["coordinateTimes"],
        json!([
            null,
            [
                ["2026-11-11T10:00:00Z", "2026-11-11T10:01:00Z"],
                ["2026-11-12T12:00:00Z", "2026-11-12T12:01:00Z"]
            ]
        ])
    );
    assert_eq!(
        layer.features[0]["geometry"]["geometries"][1]["geometries"][0]["coordinates"],
        json!([[1.0, 2.0], [2.0, 3.0, 4.0]])
    );
    assert_eq!(
        layer.features[0]["geometry"]["geometries"][1]["geometries"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn kml_tracks_do_not_connect_across_missing_or_invalid_locations() {
    let layer=parse(br#"<kml xmlns:gx="http://www.google.com/kml/ext/2.2"><Placemark><gx:Track>
      <when>2026-11-11T10:00:00Z</when><when>2026-11-11T10:01:00Z</when><when>2026-11-11T10:02:00Z</when><when>2026-11-11T10:03:00Z</when><when>2026-11-11T10:04:00Z</when>
      <gx:coord>1 2 3</gx:coord><gx:coord>2 3 4</gx:coord><gx:coord/><gx:coord>10 20 5</gx:coord><gx:coord>11 21 6</gx:coord>
    </gx:Track></Placemark></kml>"#,"kml",&||false).unwrap();
    let feature = &layer.features[0];
    assert_eq!(feature["geometry"]["type"], "MultiLineString");
    assert_eq!(
        feature["geometry"]["coordinates"],
        json!([
            [[1.0, 2.0, 3.0], [2.0, 3.0, 4.0]],
            [[10.0, 20.0, 5.0], [11.0, 21.0, 6.0]]
        ])
    );
    assert_eq!(
        feature["properties"]["coordinateTimes"],
        json!([
            ["2026-11-11T10:00:00Z", "2026-11-11T10:01:00Z"],
            ["2026-11-11T10:03:00Z", "2026-11-11T10:04:00Z"]
        ])
    );
    assert_eq!(
        feature["properties"]["sourceTrackTimes"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
    assert!(layer
        .warnings
        .iter()
        .any(|warning| warning.contains("without connecting across gaps")));
}

#[test]
fn kml_multitrack_keeps_valid_siblings_and_single_samples() {
    let layer=parse(br#"<kml xmlns:gx="http://www.google.com/kml/ext/2.2"><Placemark><gx:MultiTrack>
      <gx:Track><gx:coord>NaN 2 3</gx:coord></gx:Track>
      <gx:Track><when>2026-11-11T10:00:00Z</when><gx:coord>1 2 3</gx:coord></gx:Track>
    </gx:MultiTrack></Placemark><Placemark><TimeSpan><begin>2026-11-10T00:00:00Z</begin><end>2026-11-12T00:00:00Z</end></TimeSpan><LineString><coordinates>4,5,NaN</coordinates></LineString></Placemark></kml>"#,"kml",&||false).unwrap();
    assert_eq!(layer.features.len(), 2);
    assert_eq!(
        layer.features[0]["geometry"]["geometries"][0]["type"],
        "Point"
    );
    assert_eq!(
        layer.features[0]["properties"]["coordinateTimes"],
        json!([["2026-11-11T10:00:00Z"]])
    );
    assert_eq!(layer.features[1]["geometry"]["type"], "Point");
    assert_eq!(
        layer.features[1]["properties"]["kmlGeometryType"],
        "LineString"
    );
    assert_eq!(
        layer.features[1]["properties"]["begin"],
        "2026-11-10T00:00:00Z"
    );
    assert_eq!(
        layer.features[1]["properties"]["end"],
        "2026-11-12T00:00:00Z"
    );
    assert!(layer
        .warnings
        .iter()
        .any(|warning| warning.contains("valid sibling tracks")));
}

#[test]
fn kml_track_timestamp_mismatches_keep_nulls_and_original_times() {
    let layer=parse(br#"<kml xmlns:gx="http://www.google.com/kml/ext/2.2"><Placemark><gx:Track>
      <when>2026-11-11T10:00:00Z</when><gx:coord>1 2 3</gx:coord><gx:coord>2 3 4</gx:coord>
    </gx:Track></Placemark><Placemark><gx:Track><when>2026-11-12T00:00:00Z</when><when>2026-11-13T00:00:00Z</when><gx:coord>3 4 5</gx:coord></gx:Track></Placemark></kml>"#,"kml",&||false).unwrap();
    assert_eq!(
        layer.features[0]["properties"]["coordinateTimes"],
        json!(["2026-11-11T10:00:00Z", null])
    );
    assert_eq!(
        layer.features[1]["properties"]["sourceTrackTimes"],
        json!(["2026-11-12T00:00:00Z", "2026-11-13T00:00:00Z"])
    );
    assert_eq!(
        layer.features[1]["properties"]["coordinateTimes"],
        json!(["2026-11-12T00:00:00Z"])
    );
    assert!(layer
        .warnings
        .iter()
        .any(|warning| warning.contains("2 KML tracks")));
}

#[test]
#[ignore = "set CULTIVATOR_KML_TRACK_LOG_SOURCE to verify a local datasource without modifying it"]
fn verifies_track_log_datasource() {
    let path = std::env::var("CULTIVATOR_KML_TRACK_LOG_SOURCE").expect("KML source path");
    let input = fs::read(path).unwrap();
    let layer = parse(&input, "kml", &|| false).unwrap();
    let source_points = layer
        .features
        .iter()
        .filter(|feature| feature["properties"]["kmlGeometryType"] == "Point")
        .count();
    let source_lines = layer
        .features
        .iter()
        .filter(|feature| feature["properties"]["kmlGeometryType"] == "LineString")
        .count();
    let points = layer
        .features
        .iter()
        .filter(|feature| feature["geometry"]["type"] == "Point")
        .count();
    let lines = layer
        .features
        .iter()
        .filter(|feature| feature["geometry"]["type"] == "LineString")
        .count();
    let speeds = layer
        .features
        .iter()
        .filter(|feature| feature["properties"].get("speed").is_some())
        .count();
    let speed_limits = layer
        .features
        .iter()
        .filter(|feature| feature["properties"].get("speedLimit").is_some())
        .count();
    let description_tables = layer
        .features
        .iter()
        .filter(|feature| feature["properties"].get("descriptionFields").is_some())
        .count();
    let counts = json!({"features":layer.features.len(),"sourcePoints":source_points,"sourceLines":source_lines,"renderedPoints":points,"renderedLines":lines,"positions":layer.positions,"speeds":speeds,"speedLimits":speed_limits,"descriptionTables":description_tables});
    println!("KML_TRACK_LOG {counts} warnings={:?}", layer.warnings);
    assert!(!layer.features.is_empty());
    if let Ok(expected) = std::env::var("CULTIVATOR_KML_TRACK_LOG_EXPECTED") {
        let expected: serde_json::Value = serde_json::from_str(&expected).unwrap();
        for (key, value) in expected.as_object().expect("expected count object") {
            assert_eq!(counts.get(key), Some(value), "{key}");
        }
    }
}

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
