use super::{
    checkpoint,
    geometry::{position, Document},
    MAX_FEATURES,
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
        kml(&root, document, cancelled)
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

fn numbers(text: &str, separator: char) -> Result<Vec<f64>, String> {
    let parts: Vec<_> = if separator == ' ' {
        text.split_whitespace().collect()
    } else {
        text.split(separator).collect()
    };
    parts
        .into_iter()
        .map(|part| {
            part.trim()
                .parse::<f64>()
                .map_err(|_| "Invalid coordinate number".into())
        })
        .collect()
}

fn kml_coordinates(node: &Node) -> Result<Vec<Value>, String> {
    node.value("coordinates")
        .ok_or("KML geometry has no coordinates")?
        .split_whitespace()
        .map(|tuple| position(&numbers(tuple, ',')?))
        .collect()
}

fn kml_geometry(node: &Node) -> Result<Value, String> {
    match node.name.as_str() {
        "Point" => {
            let coordinates = kml_coordinates(node)?;
            if coordinates.len() != 1 {
                return Err("KML Point requires one coordinate".into());
            }
            Ok(json!({"type":"Point", "coordinates":coordinates[0]}))
        }
        "LineString" => Ok(json!({"type":"LineString", "coordinates":kml_coordinates(node)?})),
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
                )?;
                if ring.first() != ring.last() {
                    if let Some(first) = ring.first().cloned() {
                        ring.push(first);
                    }
                }
                rings.push(ring);
            }
            Ok(json!({"type":"Polygon", "coordinates":rings}))
        }
        "Track" => {
            let coordinates = node
                .named("coord")
                .map(|coord| position(&numbers(coord.text.trim(), ' ')?))
                .collect::<Result<Vec<_>, String>>()?;
            Ok(json!({"type":"LineString", "coordinates":coordinates}))
        }
        "MultiGeometry" | "MultiTrack" => {
            let geometries = node
                .children
                .iter()
                .filter(|child| is_geometry(&child.name))
                .map(kml_geometry)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(json!({"type":"GeometryCollection", "geometries":geometries}))
        }
        _ => Err("Unsupported KML geometry".into()),
    }
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

fn kml(node: &Node, document: &mut Document, cancelled: &dyn Fn() -> bool) -> Result<(), String> {
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
        let geometry = node.children.iter().find(|child| is_geometry(&child.name));
        if let Some(geometry) = geometry {
            if geometry.name == "Track" {
                properties.insert(
                    "coordinateTimes".into(),
                    json!(geometry
                        .named("when")
                        .map(|n| n.text.trim())
                        .collect::<Vec<_>>()),
                );
            }
            match kml_geometry(geometry)
                .and_then(|value| document.add(feature(value, properties), cancelled))
            {
                Ok(()) => (),
                Err(error) => {
                    checkpoint(cancelled)?;
                    document.warning(error);
                }
            }
        }
    } else if is_geometry(&node.name) {
        match kml_geometry(node)
            .and_then(|value| document.add(feature(value, Map::new()), cancelled))
        {
            Ok(()) => (),
            Err(error) => {
                checkpoint(cancelled)?;
                document.warning(error);
            }
        }
    } else {
        for child in &node.children {
            kml(child, document, cancelled)?;
        }
    }
    Ok(())
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
