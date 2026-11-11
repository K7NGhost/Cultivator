use super::{
    checkpoint,
    geometry::{position, Document},
    MAX_FEATURES, MAX_POSITIONS,
};
use quick_xml::{events::Event, Reader};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

#[derive(Default)]
struct Node {
    name: String,
    attributes: BTreeMap<String, String>,
    text: String,
    children: Vec<Node>,
}

impl Node {
    fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|child| child.name == name)
    }
    fn value(&self, name: &str) -> Option<&str> {
        self.child(name)
            .map(|child| child.text.trim())
            .filter(|text| !text.is_empty())
    }
    fn named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> {
        self.children.iter().filter(move |child| child.name == name)
    }
}

fn tree(bytes: &[u8], cancelled: &dyn Fn() -> bool) -> Result<Node, String> {
    let mut reader = Reader::from_reader(bytes);
    let mut stack = vec![Node::default()];
    let mut nodes = 0usize;
    loop {
        checkpoint(cancelled)?;
        let event = reader
            .read_event()
            .map_err(|e| format!("Invalid XML: {e}"))?;
        let empty = matches!(event, Event::Empty(_));
        match event {
            Event::Start(event) | Event::Empty(event) => {
                nodes += 1;
                if nodes > 1_000_000 || stack.len() > 128 {
                    return Err("XML exceeds the node or depth limit".into());
                }
                let mut node = Node {
                    name: String::from_utf8_lossy(event.local_name().as_ref()).into_owned(),
                    ..Node::default()
                };
                for attribute in event.attributes() {
                    let attribute = attribute.map_err(|e| e.to_string())?;
                    node.attributes.insert(
                        String::from_utf8_lossy(attribute.key.local_name().as_ref()).into_owned(),
                        attribute
                            .unescape_value()
                            .map_err(|e| e.to_string())?
                            .into_owned(),
                    );
                }
                if empty {
                    stack
                        .last_mut()
                        .ok_or("Invalid XML structure")?
                        .children
                        .push(node);
                } else {
                    stack.push(node);
                }
            }
            Event::End(_) => {
                if stack.len() < 2 {
                    return Err("Unbalanced XML elements".into());
                }
                let node = stack.pop().ok_or("Missing XML element")?;
                stack
                    .last_mut()
                    .ok_or("Missing XML parent")?
                    .children
                    .push(node);
            }
            Event::Text(text) => stack
                .last_mut()
                .ok_or("Missing XML text parent")?
                .text
                .push_str(&text.xml_content().map_err(|e| e.to_string())?),
            Event::CData(text) => stack
                .last_mut()
                .ok_or("Missing XML text parent")?
                .text
                .push_str(&text.xml_content().map_err(|e| e.to_string())?),
            Event::GeneralRef(reference) => {
                let text = if let Some(character) =
                    reference.resolve_char_ref().map_err(|e| e.to_string())?
                {
                    character.to_string()
                } else {
                    match reference.decode().map_err(|e| e.to_string())?.as_ref() {
                        "amp" => "&",
                        "lt" => "<",
                        "gt" => ">",
                        "quot" => "\"",
                        "apos" => "'",
                        _ => return Err("Unsupported XML entity".into()),
                    }
                    .to_owned()
                };
                stack
                    .last_mut()
                    .ok_or("Missing XML entity parent")?
                    .text
                    .push_str(&text);
            }
            Event::DocType(_) => {
                return Err("XML document type declarations are not supported".into())
            }
            Event::Eof => break,
            _ => (),
        }
    }
    if stack.len() != 1 {
        return Err("Unclosed XML elements".into());
    }
    let mut document = stack.pop().ok_or("Empty XML document")?;
    if document.children.len() != 1 || !document.text.trim().is_empty() {
        return Err("Expected one XML root element".into());
    }
    document
        .children
        .pop()
        .ok_or_else(|| "Empty XML document".into())
}

pub(super) fn parse(
    bytes: &[u8],
    format: &str,
    document: &mut Document,
    cancelled: &dyn Fn() -> bool,
) -> Result<(), String> {
    let root = tree(bytes, cancelled)?;
    if root.name != format {
        return Err(format!("Expected a {format} root element"));
    }
    if format == "kml" {
        let mut diagnostics = KmlDiagnostics::default();
        kml(
            &root,
            document,
            cancelled,
            &mut Vec::new(),
            &mut diagnostics,
        )?;
        if diagnostics.omitted_altitudes > 0 {
            document.warning(format!("{} KML coordinates had an unknown elevation; their valid longitude/latitude were retained as 2D positions.", diagnostics.omitted_altitudes));
        }
        if diagnostics.track_gaps > 0 {
            document.warning(format!("{} invalid or empty KML track positions were skipped; separate track segments were retained without connecting across gaps.", diagnostics.track_gaps));
        }
        if diagnostics.unmatched_times > 0 {
            document.warning(format!("{} KML tracks had different position/timestamp counts. Known timestamps were paired by source index, missing times remain null, and original times were retained separately.", diagnostics.unmatched_times));
        }
        if diagnostics.single_positions > 0 {
            document.warning(format!(
                "{} single-position KML track segments were retained as points.",
                diagnostics.single_positions
            ));
        }
        if diagnostics.dropped_tracks > 0 {
            document.warning(format!("{} invalid KML MultiTrack segments were skipped; valid sibling tracks were retained.", diagnostics.dropped_tracks));
        }
        Ok(())
    } else {
        gpx(&root, document, cancelled)
    }
}

fn properties(node: &Node, fields: &[&str]) -> Map<String, Value> {
    fields
        .iter()
        .filter_map(|name| {
            node.value(name)
                .map(|value| ((*name).to_owned(), json!(value)))
        })
        .collect()
}

fn feature(geometry: Value, properties: Map<String, Value>) -> Value {
    json!({"type":"Feature", "geometry":geometry, "properties":properties})
}

#[derive(Default)]
struct KmlDiagnostics {
    omitted_altitudes: usize,
    track_gaps: usize,
    unmatched_times: usize,
    single_positions: usize,
    dropped_tracks: usize,
}

impl KmlDiagnostics {
    fn add(&mut self, other: &Self) {
        self.omitted_altitudes += other.omitted_altitudes;
        self.track_gaps += other.track_gaps;
        self.unmatched_times += other.unmatched_times;
        self.single_positions += other.single_positions;
        self.dropped_tracks += other.dropped_tracks;
    }
}

struct KmlGeometry {
    geometry: Value,
    times: Option<Value>,
    source_times: Option<Value>,
}

impl KmlGeometry {
    fn plain(geometry: Value) -> Self {
        Self {
            geometry,
            times: None,
            source_times: None,
        }
    }
}

fn kml_position(
    text: &str,
    separator: char,
    diagnostics: &mut KmlDiagnostics,
) -> Result<Value, String> {
    let parts: Vec<_> = if separator == ' ' {
        text.split_whitespace().collect()
    } else {
        text.split(separator).map(str::trim).collect()
    };
    if !(2..=3).contains(&parts.len()) {
        return Err("KML position requires longitude, latitude and optional elevation".into());
    }
    let mut values = parts[..2]
        .iter()
        .map(|part| {
            part.parse::<f64>()
                .map_err(|_| "Invalid coordinate number".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    // An unknown optional altitude must not discard a known location. Horizontal
    // coordinates remain strict: NaN, infinity and out-of-range values fail.
    let horizontal = position(&values)?;
    if let Some(altitude) = parts.get(2) {
        match altitude
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
        {
            Some(altitude) => {
                values.push(altitude);
                return position(&values);
            }
            None => diagnostics.omitted_altitudes += 1,
        }
    }
    Ok(horizontal)
}

fn kml_coordinates(node: &Node, diagnostics: &mut KmlDiagnostics) -> Result<Vec<Value>, String> {
    node.value("coordinates")
        .ok_or("KML geometry has no coordinates")?
        .split_whitespace()
        .map(|tuple| kml_position(tuple, ',', diagnostics))
        .collect()
}

fn kml_geometry(
    node: &Node,
    diagnostics: &mut KmlDiagnostics,
    cancelled: &dyn Fn() -> bool,
) -> Result<KmlGeometry, String> {
    checkpoint(cancelled)?;
    match node.name.as_str() {
        "Point" => {
            let coordinates = kml_coordinates(node, diagnostics)?;
            if coordinates.len() != 1 {
                return Err("KML Point requires one coordinate".into());
            }
            Ok(KmlGeometry::plain(
                json!({"type":"Point", "coordinates":coordinates[0]}),
            ))
        }
        "LineString" => {
            let coordinates = kml_coordinates(node, diagnostics)?;
            if coordinates.len() == 1 {
                diagnostics.single_positions += 1;
            }
            Ok(KmlGeometry::plain(gpx_line(coordinates)))
        }
        "Polygon" => {
            let mut rings = Vec::new();
            for boundary in node
                .named("outerBoundaryIs")
                .chain(node.named("innerBoundaryIs"))
            {
                let mut ring = kml_coordinates(
                    boundary
                        .child("LinearRing")
                        .ok_or("KML polygon boundary has no ring")?,
                    diagnostics,
                )?;
                if ring.first() != ring.last() {
                    if let Some(first) = ring.first().cloned() {
                        ring.push(first);
                    }
                }
                rings.push(ring);
            }
            Ok(KmlGeometry::plain(
                json!({"type":"Polygon", "coordinates":rings}),
            ))
        }
        "Track" => kml_track(node, diagnostics, cancelled),
        "MultiGeometry" | "MultiTrack" => {
            let mut geometries = Vec::new();
            let mut times = Vec::new();
            let mut source_times = Vec::new();
            for child in node
                .children
                .iter()
                .filter(|child| is_geometry(&child.name))
            {
                let mut child_diagnostics = KmlDiagnostics::default();
                let geometry = match kml_geometry(child, &mut child_diagnostics, cancelled) {
                    Ok(geometry) => geometry,
                    Err(_) if node.name == "MultiTrack" => {
                        checkpoint(cancelled)?;
                        diagnostics.dropped_tracks += 1;
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                diagnostics.add(&child_diagnostics);
                geometries.push(geometry.geometry);
                times.push(geometry.times.unwrap_or(Value::Null));
                source_times.push(geometry.source_times.unwrap_or(Value::Null));
            }
            if geometries.is_empty() {
                return Err("KML geometry has no valid segments".into());
            }
            Ok(KmlGeometry {
                geometry: json!({"type":"GeometryCollection", "geometries":geometries}),
                times: times
                    .iter()
                    .any(|time| !time.is_null())
                    .then(|| json!(times)),
                source_times: source_times
                    .iter()
                    .any(|time| !time.is_null())
                    .then(|| json!(source_times)),
            })
        }
        _ => Err("Unsupported KML geometry".into()),
    }
}

fn kml_track(
    node: &Node,
    diagnostics: &mut KmlDiagnostics,
    cancelled: &dyn Fn() -> bool,
) -> Result<KmlGeometry, String> {
    let times: Vec<Value> = node
        .named("when")
        .map(|node| {
            let text = node.text.trim();
            if text.is_empty() {
                Value::Null
            } else {
                json!(text)
            }
        })
        .collect();
    let positions: Vec<&Node> = node.named("coord").collect();
    if positions.len() > MAX_POSITIONS {
        return Err("KML track exceeds the position limit".into());
    }
    let mismatch = times.len() != positions.len();
    if mismatch {
        diagnostics.unmatched_times += 1;
    }
    let mut segments = Vec::new();
    let mut segment_times = Vec::new();
    let mut coordinates = Vec::new();
    let mut coordinate_times = Vec::new();
    let mut gaps = 0;
    for (index, coord) in positions.into_iter().enumerate() {
        checkpoint(cancelled)?;
        match kml_position(coord.text.trim(), ' ', diagnostics) {
            Ok(position) => {
                coordinates.push(position);
                coordinate_times.push(times.get(index).cloned().unwrap_or(Value::Null));
            }
            Err(_) => {
                gaps += 1;
                if !coordinates.is_empty() {
                    segments.push(std::mem::take(&mut coordinates));
                    segment_times.push(std::mem::take(&mut coordinate_times));
                }
            }
        }
    }
    if !coordinates.is_empty() {
        segments.push(coordinates);
        segment_times.push(coordinate_times);
    }
    if segments.is_empty() {
        return Err("KML track has no valid positions".into());
    }
    diagnostics.track_gaps += gaps;
    diagnostics.single_positions += segments.iter().filter(|segment| segment.len() == 1).count();
    let (geometry, coordinate_times) = if segments.len() == 1 {
        (
            gpx_line(segments.pop().unwrap()),
            json!(segment_times.pop().unwrap()),
        )
    } else {
        (gpx_track(segments), json!(segment_times))
    };
    Ok(KmlGeometry {
        geometry,
        times: Some(coordinate_times),
        source_times: (mismatch || gaps > 0).then(|| json!(times)),
    })
}

fn is_geometry(name: &str) -> bool {
    matches!(
        name,
        "Point" | "LineString" | "Polygon" | "MultiGeometry" | "Track" | "MultiTrack"
    )
}

fn extended_data(node: &Node, output: &mut Map<String, Value>) {
    if let Some(extended) = node.child("ExtendedData") {
        let mut values = Map::new();
        for child in &extended.children {
            if child.name == "Data" {
                if let (Some(name), Some(value)) =
                    (child.attributes.get("name"), child.value("value"))
                {
                    values.insert(name.clone(), json!(value));
                }
            } else if child.name == "SchemaData" {
                for value in child.named("SimpleData") {
                    if let Some(name) = value.attributes.get("name") {
                        values.insert(name.clone(), json!(value.text.trim()));
                    }
                }
            }
        }
        output.insert("extendedData".into(), json!(values));
    }
}

fn kml(
    node: &Node,
    document: &mut Document,
    cancelled: &dyn Fn() -> bool,
    folders: &mut Vec<String>,
    diagnostics: &mut KmlDiagnostics,
) -> Result<(), String> {
    checkpoint(cancelled)?;
    if document.features.len() >= MAX_FEATURES {
        return Err("Map exceeds the feature limit".into());
    }
    if node.name == "Placemark" {
        let mut properties = properties(node, &["name", "description", "styleUrl", "address"]);
        extended_data(node, &mut properties);
        if let Some(id) = node.attributes.get("id") {
            properties.insert("sourceId".into(), json!(id));
        }
        if let Some(timestamp) = node.child("TimeStamp").and_then(|time| time.value("when")) {
            properties.insert("time".into(), json!(timestamp));
        }
        if let Some(span) = node.child("TimeSpan") {
            for field in ["begin", "end"] {
                if let Some(value) = span.value(field) {
                    properties.insert(field.into(), json!(value));
                }
            }
        }
        let geometry = node.children.iter().find(|child| is_geometry(&child.name));
        if let Some(geometry) = geometry {
            let mut local = KmlDiagnostics::default();
            match kml_geometry(geometry, &mut local, cancelled).and_then(|value| {
                kml_properties(&mut properties, geometry, &value, folders, &local);
                document.add(feature(value.geometry, properties), cancelled)
            }) {
                Ok(()) => diagnostics.add(&local),
                Err(error) => {
                    checkpoint(cancelled)?;
                    document.warning(error);
                }
            }
        }
    } else if is_geometry(&node.name) {
        let mut local = KmlDiagnostics::default();
        match kml_geometry(node, &mut local, cancelled).and_then(|value| {
            let mut properties = Map::new();
            kml_properties(&mut properties, node, &value, folders, &local);
            document.add(feature(value.geometry, properties), cancelled)
        }) {
            Ok(()) => diagnostics.add(&local),
            Err(error) => {
                checkpoint(cancelled)?;
                document.warning(error);
            }
        }
    } else {
        let folder = matches!(node.name.as_str(), "Document" | "Folder")
            .then(|| node.value("name"))
            .flatten();
        if let Some(folder) = folder {
            folders.push(folder.to_string());
        }
        for child in &node.children {
            kml(child, document, cancelled, folders, diagnostics)?;
        }
        if folder.is_some() {
            folders.pop();
        }
    }
    Ok(())
}

fn kml_properties(
    properties: &mut Map<String, Value>,
    node: &Node,
    geometry: &KmlGeometry,
    folders: &[String],
    diagnostics: &KmlDiagnostics,
) {
    properties.insert("kmlGeometryType".into(), json!(node.name));
    if !folders.is_empty() {
        properties.insert("folderPath".into(), json!(folders));
    }
    if let Some(times) = &geometry.times {
        properties.insert("coordinateTimes".into(), times.clone());
    }
    if let Some(times) = &geometry.source_times {
        properties.insert("sourceTrackTimes".into(), times.clone());
    }
    if diagnostics.omitted_altitudes > 0 {
        properties.insert(
            "omittedAltitudeCount".into(),
            json!(diagnostics.omitted_altitudes),
        );
        if node.name == "Point" {
            if let Some(altitude) = node
                .value("coordinates")
                .and_then(|value| value.split(',').nth(2))
            {
                properties.insert("sourceAltitude".into(), json!(altitude.trim()));
            }
        }
    }
}

fn gpx_position(node: &Node) -> Result<Value, String> {
    let mut values = vec![
        node.attributes
            .get("lon")
            .ok_or("GPX point has no longitude")?
            .parse()
            .map_err(|_| "Invalid GPX longitude")?,
        node.attributes
            .get("lat")
            .ok_or("GPX point has no latitude")?
            .parse()
            .map_err(|_| "Invalid GPX latitude")?,
    ];
    if let Some(elevation) = node.value("ele") {
        values.push(elevation.parse().map_err(|_| "Invalid GPX elevation")?);
    }
    position(&values)
}

fn gpx_line(coordinates: Vec<Value>) -> Value {
    match coordinates.len() {
        0 => Value::Null,
        1 => json!({"type":"Point", "coordinates":coordinates[0]}),
        _ => json!({"type":"LineString", "coordinates":coordinates}),
    }
}

fn gpx_track(mut segments: Vec<Vec<Value>>) -> Value {
    segments.retain(|segment| !segment.is_empty());
    if segments.is_empty() {
        return Value::Null;
    }
    if segments.iter().all(|segment| segment.len() > 1) {
        json!({"type":"MultiLineString", "coordinates":segments})
    } else {
        json!({"type":"GeometryCollection", "geometries":segments.into_iter().map(gpx_line).collect::<Vec<_>>()})
    }
}

fn gpx(node: &Node, document: &mut Document, cancelled: &dyn Fn() -> bool) -> Result<(), String> {
    for child in &node.children {
        checkpoint(cancelled)?;
        let mut properties = properties(
            child,
            &["name", "desc", "cmt", "time", "sym", "type", "number"],
        );
        properties.insert("gpxType".into(), json!(child.name));
        let result = match child.name.as_str() {
            "wpt" => gpx_position(child)
                .map(|coordinates| json!({"type":"Point", "coordinates":coordinates})),
            "rte" => {
                properties.insert(
                    "coordinateTimes".into(),
                    json!(child
                        .named("rtept")
                        .map(|p| p.value("time"))
                        .collect::<Vec<_>>()),
                );
                child
                    .named("rtept")
                    .map(gpx_position)
                    .collect::<Result<Vec<_>, _>>()
                    .map(gpx_line)
            }
            "trk" => {
                properties.insert(
                    "coordinateTimes".into(),
                    json!(child
                        .named("trkseg")
                        .map(|s| s
                            .named("trkpt")
                            .map(|p| p.value("time"))
                            .collect::<Vec<_>>())
                        .collect::<Vec<_>>()),
                );
                child
                    .named("trkseg")
                    .map(|segment| {
                        segment
                            .named("trkpt")
                            .map(gpx_position)
                            .collect::<Result<Vec<_>, _>>()
                    })
                    .collect::<Result<Vec<_>, _>>()
                    .map(gpx_track)
            }
            _ => continue,
        };
        match result.and_then(|geometry| document.add(feature(geometry, properties), cancelled)) {
            Ok(()) => (),
            Err(error) => {
                checkpoint(cancelled)?;
                document.warning(error);
            }
        }
    }
    Ok(())
}
