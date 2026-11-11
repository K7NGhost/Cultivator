use super::{checkpoint, MAX_FEATURES, MAX_POSITIONS};
use serde_json::{json, Value};

#[derive(Default)]
pub(super) struct Document {
    pub features: Vec<Value>,
    pub warnings: Vec<String>,
    pub positions: usize,
    pub bounds: Option<[f64; 4]>,
}

pub(super) fn position(values: &[f64]) -> Result<Value, String> {
    if values.len() < 2
        || values.len() > 3
        || values.iter().any(|value| !value.is_finite())
        || !(-180.0..=180.0).contains(&values[0])
        || !(-90.0..=90.0).contains(&values[1])
    {
        return Err(
            "Invalid WGS84 coordinate (expected longitude, latitude, optional elevation)".into(),
        );
    }
    Ok(json!(values))
}

impl Document {
    pub fn add(&mut self, mut feature: Value, cancelled: &dyn Fn() -> bool) -> Result<(), String> {
        checkpoint(cancelled)?;
        if self.features.len() >= MAX_FEATURES {
            return Err("Map exceeds the 100,000 feature limit".into());
        }
        if feature["type"] != "Feature" {
            return Err("Expected a GeoJSON Feature".into());
        }
        if feature["geometry"].is_null() {
            return Ok(());
        }
        let mut positions = Vec::new();
        validate_geometry(&feature["geometry"], 0, &mut positions, cancelled)?;
        if self.positions + positions.len() > MAX_POSITIONS {
            return Err("Map exceeds the 500,000 position limit".into());
        }
        if !feature["properties"].is_object() && !feature["properties"].is_null() {
            return Err("Feature properties must be an object or null".into());
        }
        if feature["properties"].is_null() {
            feature["properties"] = json!({});
        }
        for [lon, lat] in positions.iter().copied() {
            match &mut self.bounds {
                Some(bounds) => {
                    bounds[0] = bounds[0].min(lon);
                    bounds[1] = bounds[1].min(lat);
                    bounds[2] = bounds[2].max(lon);
                    bounds[3] = bounds[3].max(lat);
                }
                None => self.bounds = Some([lon, lat, lon, lat]),
            }
        }
        self.positions += positions.len();
        self.features.push(feature);
        Ok(())
    }

    pub fn geojson(&mut self, value: Value, cancelled: &dyn Fn() -> bool) -> Result<(), String> {
        if let Some(crs) = value.get("crs").filter(|v| !v.is_null()) {
            let name = crs["properties"]["name"]
                .as_str()
                .unwrap_or("")
                .to_ascii_uppercase();
            if !matches!(
                name.as_str(),
                "EPSG:4326" | "URN:OGC:DEF:CRS:OGC:1.3:CRS84" | "URN:OGC:DEF:CRS:EPSG::4326"
            ) {
                return Err("GeoJSON must use WGS84 longitude/latitude coordinates".into());
            }
        }
        let features = match value["type"].as_str() {
            Some("FeatureCollection") => value["features"]
                .as_array()
                .cloned()
                .ok_or("Missing GeoJSON features array")?,
            Some("Feature") => vec![value],
            Some(
                "Point" | "MultiPoint" | "LineString" | "MultiLineString" | "Polygon"
                | "MultiPolygon" | "GeometryCollection",
            ) => vec![json!({"type":"Feature", "properties":{}, "geometry":value})],
            _ => return Err("Expected GeoJSON geometry, Feature or FeatureCollection".into()),
        };
        if features.len() > MAX_FEATURES {
            return Err("Map exceeds the 100,000 feature limit".into());
        }
        for (index, feature) in features.into_iter().enumerate() {
            if let Err(error) = self.add(feature, cancelled) {
                checkpoint(cancelled)?;
                self.warning(format!("Feature {}: {error}", index + 1));
            }
        }
        Ok(())
    }

    pub fn warning(&mut self, message: String) {
        if self.warnings.len() < 100 {
            self.warnings.push(message);
        }
    }
}

fn array(value: &Value) -> Result<&Vec<Value>, String> {
    value
        .as_array()
        .ok_or_else(|| "Invalid GeoJSON coordinates array".into())
}

fn coordinate(value: &Value, output: &mut Vec<[f64; 2]>) -> Result<(), String> {
    let values = array(value)?
        .iter()
        .map(|v| v.as_f64().ok_or("Coordinate is not numeric"))
        .collect::<Result<Vec<_>, _>>()?;
    position(&values)?;
    if output.len() >= MAX_POSITIONS {
        return Err("Geometry exceeds the 500,000 position limit".into());
    }
    output.push([values[0], values[1]]);
    Ok(())
}

fn line(
    value: &Value,
    ring: bool,
    output: &mut Vec<[f64; 2]>,
    cancelled: &dyn Fn() -> bool,
) -> Result<(), String> {
    let points = array(value)?;
    if points.len() < if ring { 4 } else { 2 } {
        return Err("Line or polygon ring has too few positions".into());
    }
    if ring && points.first() != points.last() {
        return Err("Polygon ring is not closed".into());
    }
    for point in points {
        checkpoint(cancelled)?;
        coordinate(point, output)?;
    }
    Ok(())
}

fn polygon(
    value: &Value,
    output: &mut Vec<[f64; 2]>,
    cancelled: &dyn Fn() -> bool,
) -> Result<(), String> {
    let rings = array(value)?;
    if rings.is_empty() {
        return Err("Polygon has no outer ring".into());
    }
    for ring in rings {
        line(ring, true, output, cancelled)?;
    }
    Ok(())
}

fn validate_geometry(
    value: &Value,
    depth: usize,
    output: &mut Vec<[f64; 2]>,
    cancelled: &dyn Fn() -> bool,
) -> Result<(), String> {
    checkpoint(cancelled)?;
    if depth > 32 {
        return Err("GeoJSON geometry nesting is too deep".into());
    }
    let coordinates = &value["coordinates"];
    match value["type"].as_str() {
        Some("Point") => coordinate(coordinates, output),
        Some("MultiPoint") => {
            for point in array(coordinates)? {
                coordinate(point, output)?;
            }
            Ok(())
        }
        Some("LineString") => line(coordinates, false, output, cancelled),
        Some("MultiLineString") => {
            for points in array(coordinates)? {
                line(points, false, output, cancelled)?;
            }
            Ok(())
        }
        Some("Polygon") => polygon(coordinates, output, cancelled),
        Some("MultiPolygon") => {
            for rings in array(coordinates)? {
                polygon(rings, output, cancelled)?;
            }
            Ok(())
        }
        Some("GeometryCollection") => {
            for geometry in array(&value["geometries"])? {
                validate_geometry(geometry, depth + 1, output, cancelled)?;
            }
            Ok(())
        }
        _ => Err("Unsupported GeoJSON geometry type".into()),
    }
}
