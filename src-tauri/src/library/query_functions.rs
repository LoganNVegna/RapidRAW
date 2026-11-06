//! Keep the existing search syntax and EXIF sorting consistent across catalog pages.
use super::*;
use rusqlite::functions::FunctionFlags;
fn number(value: &str) -> f64 {
    let start = value
        .find(|c: char| c.is_ascii_digit() || c == '-' || c == '.')
        .unwrap_or(value.len());
    value[start..]
        .split(|c: char| !c.is_ascii_digit() && c != '-' && c != '.')
        .next()
        .unwrap_or("")
        .parse()
        .unwrap_or(0.)
}
fn shutter(value: &str) -> f64 {
    if let Some((a, b)) = value.split_once('/') {
        let denominator = number(b);
        if denominator != 0. {
            number(a) / denominator
        } else {
            0.
        }
    } else {
        number(value)
    }
}
fn exif_number(exif: &HashMap<String, String>, field: &str) -> f64 {
    let get = |key: &str| exif.get(key).map(String::as_str).unwrap_or("");
    match field {
        "iso" => number(
            exif.get("PhotographicSensitivity")
                .or_else(|| exif.get("ISOSpeedRatings"))
                .map(String::as_str)
                .unwrap_or(""),
        ),
        "aperture" | "f" => number(get("FNumber")),
        "focal_length" | "focal" | "mm" => number(get("FocalLength")),
        "shutter_speed" | "shutter" | "s" => shutter(get("ExposureTime")),
        _ => 0.,
    }
}
fn matches_tag(tags: &[String], exif: &HashMap<String, String>, rating: i64, query: &str) -> bool {
    let query = query.to_lowercase();
    let field = query
        .split(|c: char| !c.is_ascii_alphabetic())
        .next()
        .unwrap_or("");
    let rest = query[field.len()..].trim_start();
    if !matches!(
        field,
        "iso"
            | "aperture"
            | "f"
            | "shutter"
            | "s"
            | "focal"
            | "mm"
            | "rating"
            | "color"
            | "camera"
            | "make"
            | "model"
            | "lens"
    ) || !rest.starts_with([':', '<', '>', '='])
    {
        return tags
            .iter()
            .any(|tag| tag.to_lowercase().replacen("user:", "", 1).contains(&query));
    }
    let rest = rest.strip_prefix(':').unwrap_or(rest).trim_start();
    let operator = [">=", "<=", ">", "<", "="]
        .into_iter()
        .find(|op| rest.starts_with(op))
        .unwrap_or("=");
    let value = rest.strip_prefix(operator).unwrap_or(rest).trim();
    if matches!(
        field,
        "iso" | "aperture" | "f" | "shutter" | "s" | "focal" | "mm" | "rating"
    ) {
        let actual = if field == "rating" {
            rating as f64
        } else {
            exif_number(exif, field)
        };
        let expected = if field == "shutter" || field == "s" {
            shutter(value)
        } else {
            number(value)
        };
        return match operator {
            ">=" => actual >= expected,
            "<=" => actual <= expected,
            ">" => actual > expected,
            "<" => actual < expected,
            _ => actual == expected,
        };
    }
    let text = match field {
        "color" => tags
            .iter()
            .find_map(|t| t.strip_prefix("color:"))
            .unwrap_or("")
            .to_string(),
        "camera" | "make" | "model" => ["Make", "Model"]
            .into_iter()
            .filter_map(|k| exif.get(k))
            .cloned()
            .collect::<Vec<_>>()
            .join(" "),
        "lens" => ["LensModel", "Lens", "LensMake"]
            .into_iter()
            .filter_map(|k| exif.get(k))
            .cloned()
            .collect::<Vec<_>>()
            .join(" "),
        _ => String::new(),
    };
    operator == "=" && text.to_lowercase().contains(value)
}
pub(super) fn register(db: &Connection) -> rusqlite::Result<()> {
    let flags = FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC;
    db.create_scalar_function("rr_basename", 1, flags, |ctx| {
        let path = ctx.get::<String>(0)?;
        Ok(path.rsplit(['/', '\\']).next().unwrap_or("").to_string())
    })?;
    db.create_scalar_function("rr_exif_number", 2, flags, |ctx| {
        let exif: HashMap<String, String> =
            serde_json::from_str(&ctx.get::<String>(0)?).unwrap_or_default();
        Ok(exif_number(&exif, &ctx.get::<String>(1)?))
    })?;
    db.create_scalar_function("rr_tag_match", 4, flags, |ctx| {
        let tags: Vec<String> = serde_json::from_str(&ctx.get::<String>(0)?).unwrap_or_default();
        let exif: HashMap<String, String> =
            serde_json::from_str(&ctx.get::<String>(1)?).unwrap_or_default();
        Ok(matches_tag(
            &tags,
            &exif,
            ctx.get(2)?,
            &ctx.get::<String>(3)?,
        ))
    })
}
