use super::*;

pub(super) fn rule_sql(rule: &Rule, values: &mut Vec<SqlValue>) -> Result<String> {
    node_sql(rule, values, 0, &mut 256)
}

fn node_sql(
    rule: &Rule,
    values: &mut Vec<SqlValue>,
    depth: usize,
    remaining: &mut usize,
) -> Result<String> {
    if depth > 8 || *remaining == 0 {
        return Err("Use at most eight nested groups and 256 conditions".into());
    }
    *remaining -= 1;
    if let Some(rules) = &rule.rules {
        let mode = rule.mode.as_deref().unwrap_or("all");
        if !["all", "any"].contains(&mode) {
            return Err("Choose All or Any for a condition group".into());
        }
        if rules.is_empty() {
            return Err("Add a condition to each nested group".into());
        }
        let clauses = rules
            .iter()
            .map(|child| node_sql(child, values, depth + 1, remaining))
            .collect::<Result<Vec<_>>>()?;
        return Ok(format!(
            "({})",
            clauses.join(if mode == "any" { " OR " } else { " AND " })
        ));
    }
    if rule.value.len() > 4096 {
        return Err("Condition values must be shorter than 4096 bytes".into());
    }
    let column = match rule.field.as_str() {
        "raw" => "a.raw",
        "rating" => "a.rating",
        "flag" => "a.flag",
        "label" => "a.label",
        "tag" => "a.tags",
        "date" => "substr(a.capture,1,10)",
        "camera" => "a.camera",
        "lens" => "a.lens",
        "edited" => "a.edited",
        "available" => "a.available",
        "review" => "a.review",
        "path" => "a.path",
        "extension" => "rr_extension(a.path)",
        "iso" => "rr_exif_number(a.exif,'iso')",
        "aperture" => "rr_exif_number(a.exif,'aperture')",
        "shutter_speed" => "rr_exif_number(a.exif,'shutter_speed')",
        "focal_length" => "rr_exif_number(a.exif,'focal_length')",
        "metadata" => {
            let key = rule.key.as_deref().unwrap_or("").trim();
            if key.is_empty() || key.len() > 128 {
                return Err("Choose a metadata field name".into());
            }
            values.push(SqlValue::Text(key.into()));
            "(SELECT CAST(value AS TEXT) FROM json_each(a.exif) WHERE key=? COLLATE NOCASE LIMIT 1)"
        }
        _ => return Err("Unsupported collection field".into()),
    };
    if matches!(rule.operator.as_str(), "exists" | "missing") {
        let presence = if rule.field == "tag" {
            "EXISTS(SELECT 1 FROM json_each(a.tags) WHERE value NOT LIKE 'color:%' AND length(trim(value))>0)".to_string()
        } else {
            format!("lower(trim(coalesce({column},''))) NOT IN ('','null','unknown','n/a')")
        };
        return Ok(if rule.operator == "missing" {
            format!("NOT ({presence})")
        } else {
            format!("({presence})")
        });
    }
    let operator = match rule.operator.as_str() {
        "eq" => "=",
        "ne" => "<>",
        "gte" => ">=",
        "lte" => "<=",
        "gt" | "after" => ">",
        "lt" | "before" => "<",
        "contains" | "starts" | "ends" => "LIKE",
        "not_contains" => "NOT LIKE",
        _ => return Err("Unsupported collection operator".into()),
    };
    if rule.field == "tag" && matches!(rule.operator.as_str(), "eq" | "ne") {
        values.push(SqlValue::Text(rule.value.clone()));
        values.push(SqlValue::Text(format!("user:{}", rule.value)));
        return Ok(format!(
            "{}EXISTS(SELECT 1 FROM json_each(a.tags) WHERE value IN(?,?))",
            if rule.operator == "ne" { "NOT " } else { "" }
        ));
    }
    let numeric = matches!(
        rule.field.as_str(),
        "rating"
            | "edited"
            | "available"
            | "raw"
            | "iso"
            | "aperture"
            | "shutter_speed"
            | "focal_length"
    );
    if numeric {
        let value: f64 = rule
            .value
            .parse()
            .map_err(|_| "A numeric condition value is required")?;
        if !value.is_finite() || matches!(operator, "LIKE" | "NOT LIKE") {
            return Err("Choose a finite numeric comparison".into());
        }
        values.push(SqlValue::Real(value));
    } else {
        let escaped = rule
            .value
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        let value = match rule.operator.as_str() {
            "starts" => format!("{escaped}%"),
            "ends" => format!("%{escaped}"),
            "contains" | "not_contains" => format!("%{escaped}%"),
            _ => {
                if rule.field == "extension" {
                    rule.value.trim_start_matches('.').to_lowercase()
                } else {
                    rule.value.clone()
                }
            }
        };
        values.push(SqlValue::Text(value));
    }
    if rule.field == "tag" && matches!(operator, "LIKE" | "NOT LIKE") {
        return Ok(format!(
            "{}EXISTS(SELECT 1 FROM json_each(a.tags) WHERE value NOT LIKE 'color:%' AND value LIKE ? ESCAPE '\\')",
            if operator == "NOT LIKE" { "NOT " } else { "" }
        ));
    }
    Ok(if matches!(operator, "LIKE" | "NOT LIKE") {
        format!("coalesce({column},'') {operator} ? ESCAPE '\\'")
    } else {
        format!("{column} {operator} ?")
    })
}
