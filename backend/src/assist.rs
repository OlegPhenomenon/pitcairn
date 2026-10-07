//! Mock AI assist (§5 `PITCAIRN_AI_MODE`): deterministic heuristic extraction —
//! no model, no network, same input always yields the same suggestions.
//! Suggestions are advisory only; the UI applies them only on explicit accept.

use serde_json::{Value, json};

use crate::dto::AssistSuggestionDto;

/// Parse a template `schema_json` into (field_key, label, type) triples.
fn schema_fields(schema: &Value) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    if let Some(sections) = schema["sections"].as_array() {
        for section in sections {
            if let Some(fields) = section["fields"].as_array() {
                for f in fields {
                    let key = f["key"].as_str().unwrap_or_default().to_string();
                    if key.is_empty() {
                        continue;
                    }
                    out.push((
                        key,
                        f["label"].as_str().unwrap_or_default().to_string(),
                        f["type"].as_str().unwrap_or_default().to_string(),
                    ));
                }
            }
        }
    }
    out
}

/// Find ISO `YYYY-MM-DD` dates in `text` (deterministic scan, no regex dep).
fn find_iso_dates(text: &str) -> Vec<(usize, String)> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    for i in 0..bytes.len().saturating_sub(9) {
        let w = &bytes[i..i + 10];
        let digits = |r: &[u8]| r.iter().all(|b| b.is_ascii_digit());
        if digits(&w[0..4]) && w[4] == b'-' && digits(&w[5..7]) && w[7] == b'-' && digits(&w[8..10])
        {
            let candidate = &text[i..i + 10];
            if chrono::NaiveDate::parse_from_str(candidate, "%Y-%m-%d").is_ok() {
                // Not preceded/followed by a digit (avoid matching inside
                // longer numeric runs).
                let prev_ok = i == 0 || !bytes[i - 1].is_ascii_digit();
                let next_ok = i + 10 >= bytes.len() || !bytes[i + 10].is_ascii_digit();
                if prev_ok && next_ok {
                    out.push((i, candidate.to_string()));
                }
            }
        }
    }
    out
}

const MONTHS: [(&str, u32); 12] = [
    ("january", 1),
    ("february", 2),
    ("march", 3),
    ("april", 4),
    ("may", 5),
    ("june", 6),
    ("july", 7),
    ("august", 8),
    ("september", 9),
    ("october", 10),
    ("november", 11),
    ("december", 12),
];

/// Find `D Month YYYY` / `Month D, YYYY` dates and normalise to ISO.
fn find_named_dates(text: &str) -> Vec<(usize, String)> {
    let lower = text.to_lowercase();
    let mut out = Vec::new();
    for (name, month) in MONTHS {
        let mut start = 0;
        while let Some(pos) = lower[start..].find(name) {
            let idx = start + pos;
            let before = lower[..idx].trim_end();
            let after = lower[idx + name.len()..].trim_start();
            // "15 April 2026" — day number immediately before the month name.
            let day_before: Option<u32> = before
                .split_whitespace()
                .last()
                .map(|s| s.trim_end_matches(|c: char| !c.is_ascii_digit()))
                .and_then(|s| s.parse().ok())
                .filter(|d| (1..=31).contains(d));
            let mut after_nums = after
                .split(|c: char| !c.is_ascii_digit())
                .filter(|s| !s.is_empty());
            let first_after: Option<u32> = after_nums.next().and_then(|s| s.parse().ok());
            // "April 15, 2026" — day then year after the month name.
            let (day_after, year_after_first): (Option<u32>, Option<i32>) =
                match first_after.filter(|d| (1..=31).contains(d)) {
                    Some(d) => (
                        Some(d),
                        after_nums
                            .find(|s| s.len() == 4)
                            .and_then(|s| s.parse().ok()),
                    ),
                    None => (None, None),
                };
            let year: Option<i32> = year_after_first.or_else(|| {
                after
                    .split(|c: char| !c.is_ascii_digit())
                    .find(|s| s.len() == 4)
                    .and_then(|s| s.parse().ok())
            });
            let day = day_before.or(day_after);
            if let (Some(day), Some(year)) = (day, year)
                && let Some(d) = chrono::NaiveDate::from_ymd_opt(year, month, day)
            {
                out.push((idx, d.format("%Y-%m-%d").to_string()));
            }
            start = idx + name.len();
        }
    }
    out.sort_by_key(|(i, _)| *i);
    out.dedup_by(|a, b| a.1 == b.1);
    out
}

/// All dates found in the text, in reading order, ISO format.
fn find_dates(text: &str) -> Vec<(usize, String)> {
    let mut v = find_iso_dates(text);
    v.extend(find_named_dates(text));
    v.sort_by_key(|(i, _)| *i);
    v.dedup_by(|a, b| a.1 == b.1);
    v
}

/// The first non-empty line (a classic document title position).
fn first_line(text: &str) -> Option<(String, String)> {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(|l| (l.to_string(), l.to_string()))
}

/// First line containing an organisation keyword (University, Institute, ...).
fn find_organisation(text: &str) -> Option<(String, String)> {
    const KEYS: [&str; 6] = [
        "university",
        "institute",
        "institution",
        "laboratory",
        "lab",
        "organisation",
    ];
    text.lines().find_map(|l| {
        let trimmed = l.trim();
        let lower = trimmed.to_lowercase();
        if trimmed.len() >= 4 && KEYS.iter().any(|k| lower.contains(k)) {
            Some((trimmed.to_string(), trimmed.to_string()))
        } else {
            None
        }
    })
}

/// Extract the paragraph following a heading line like "Objectives:" or
/// "3. Methods". Returns the paragraph text and the matched heading line.
fn section_by_heading(text: &str, keys: &[&str]) -> Option<(String, String)> {
    let lines: Vec<&str> = text.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let normalized = line
            .trim()
            .trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ')')
            .trim()
            .trim_end_matches(':')
            .to_lowercase();
        if normalized.len() > 40 || normalized.is_empty() {
            continue;
        }
        let heading_hit = keys
            .iter()
            .any(|k| normalized == *k || normalized.starts_with(&format!("{k} ")));
        if !heading_hit {
            continue;
        }
        // Collect following non-empty lines until a blank line ends the block.
        let mut body: Vec<&str> = Vec::new();
        for l in &lines[i + 1..] {
            if l.trim().is_empty() {
                if body.is_empty() {
                    continue; // skip blank line right after the heading
                }
                break;
            }
            body.push(l.trim());
            if body.join(" ").len() > 1500 {
                break;
            }
        }
        if !body.is_empty() {
            return Some((body.join(" "), line.trim().to_string()));
        }
    }
    None
}

/// `lat,lng` or `lat lng` decimal pairs in the text → GeoJSON points.
fn find_coordinate_pairs(text: &str) -> Vec<(f64, f64, String)> {
    let mut out = Vec::new();
    for token in text.split([';', '\n']) {
        let nums: Vec<f64> = token
            .split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+'))
            .filter(|s| s.contains('.'))
            .filter_map(|s| s.parse::<f64>().ok())
            .collect();
        if nums.len() >= 2 {
            let (lat, lng) = (nums[0], nums[1]);
            if (-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lng) {
                out.push((lat, lng, token.trim().to_string()));
            }
        }
    }
    out
}

/// Heuristic suggestions for every field of `schema` that `text` can fill.
pub fn extract_fields(schema: &Value, text: &str) -> Vec<AssistSuggestionDto> {
    let mut out = Vec::new();
    let dates = find_dates(text);
    for (key, label, ty) in schema_fields(schema) {
        let kl = format!("{} {}", key.to_lowercase(), label.to_lowercase());
        let suggestion = match ty.as_str() {
            "date" => dates.first().map(|(_, d)| (json!(d), 0.7, d.clone())),
            "daterange" => {
                if dates.len() >= 2 {
                    Some((
                        json!({"start": dates[0].1, "end": dates[1].1}),
                        0.7,
                        format!("{} – {}", dates[0].1, dates[1].1),
                    ))
                } else {
                    dates
                        .first()
                        .map(|(_, d)| (json!({"start": d, "end": d}), 0.5, d.clone()))
                }
            }
            "sites" => {
                let points = find_coordinate_pairs(text);
                if points.is_empty() {
                    None
                } else {
                    let sites: Vec<Value> = points
                        .iter()
                        .enumerate()
                        .map(|(i, (lat, lng, _))| {
                            json!({
                                "name": format!("Site {}", i + 1),
                                "geometry": {"type": "Point", "coordinates": [lng, lat]},
                            })
                        })
                        .collect();
                    Some((
                        Value::Array(sites),
                        0.5,
                        points
                            .iter()
                            .map(|(_, _, s)| s.as_str())
                            .collect::<Vec<_>>()
                            .join("; "),
                    ))
                }
            }
            "text" | "textarea" => {
                // The document title fills the project-title field, not a
                // personal title ("Dr") field such as `applicant_title`.
                let is_project_title = key == "title"
                    || key.ends_with("research_title")
                    || label.to_lowercase().starts_with("title of");
                if is_project_title {
                    first_line(text).map(|(v, s)| (json!(v), 0.9, s))
                } else if kl.contains("organisation") || kl.contains("institution") {
                    find_organisation(text).map(|(v, s)| (json!(v), 0.8, s))
                } else {
                    // Heading-keyed paragraphs for narrative fields.
                    let keys: Vec<&str> = [
                        "aims",
                        "objectives",
                        "methods",
                        "methodology",
                        "abstract",
                        "summary",
                        "description",
                        "outputs",
                        "timeline",
                        "budget",
                        "safety",
                        "risk",
                        "data",
                    ]
                    .iter()
                    .copied()
                    .filter(|k| kl.contains(k))
                    .collect();
                    if !keys.is_empty() {
                        section_by_heading(text, &keys).map(|(v, s)| (json!(v), 0.85, s))
                    } else {
                        None
                    }
                }
            }
            _ => None,
        };
        if let Some((value, confidence, excerpt)) = suggestion {
            out.push(AssistSuggestionDto {
                field_key: key,
                value,
                confidence,
                source_excerpt: excerpt.chars().take(300).collect(),
            });
        }
    }
    out
}

/// Extractive summary: title + the first sentence of each narrative answer,
/// in schema order. Deterministic — purely a re-arrangement of the answers.
pub fn summarize_answers(schema: &Value, answers: &Value, title: &str, summary: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !title.is_empty() {
        parts.push(format!("{title}."));
    }
    if !summary.is_empty() {
        parts.push(first_sentence(summary));
    }
    // Narrative answers only (identity fields like name/address add noise).
    for (key, label, _ty) in schema_fields(schema)
        .into_iter()
        .filter(|(key, _, ty)| ty == "textarea" && key != "address")
    {
        let Some(v) = answers.get(&key).and_then(|v| v.as_str()) else {
            continue;
        };
        let sentence = first_sentence(v);
        if sentence.is_empty() {
            continue;
        }
        let short_label = label.split('(').next().unwrap_or(&label).trim();
        parts.push(format!("{short_label}: {sentence}"));
        if parts.len() >= 7 {
            break;
        }
    }
    parts.join(" ")
}

fn first_sentence(text: &str) -> String {
    let trimmed = text.trim();
    for (i, c) in trimmed.char_indices() {
        if c == '.' && i + 1 < trimmed.len() {
            let next = trimmed[i + 1..].chars().next().unwrap_or(' ');
            if next.is_whitespace() {
                return trimmed[..=i].chars().take(240).collect();
            }
        }
    }
    trimmed.chars().take(240).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_iso_and_named_dates() {
        let text = "Fieldwork from 2026-03-04 to 2026-03-21; backup 15 April 2026.";
        let dates = find_dates(text);
        assert!(dates.iter().any(|(_, d)| d == "2026-03-04"));
        assert!(dates.iter().any(|(_, d)| d == "2026-03-21"));
        assert!(dates.iter().any(|(_, d)| d == "2026-04-15"));
    }

    #[test]
    fn extracts_heading_sections() {
        let text =
            "My Project\n\nObjectives:\n1. Measure coral.\n2. Map reefs.\n\nMethods:\nTransects.";
        let (body, _) = section_by_heading(text, &["objectives"]).expect("objectives found");
        assert!(body.contains("Measure coral"));
        assert!(!body.contains("Transects"));
    }

    #[test]
    fn deterministic_and_bounded() {
        let schema = serde_json::json!({"sections":[{"key":"s","title":"S","fields":[
            {"key":"research_title","label":"Title of proposed research","type":"text","required":true},
            {"key":"dates","label":"Dates","type":"daterange","required":true},
            {"key":"objectives","label":"Objectives","type":"textarea","required":true}
        ]}]});
        let text = "Coral survey\n\nObjectives:\nCount things.\n\nDates 2026-01-01 to 2026-02-01";
        let a = extract_fields(&schema, text);
        let b = extract_fields(&schema, text);
        assert_eq!(a.len(), b.len());
        assert!(a.iter().any(|s| s.field_key == "research_title"));
        assert!(a.iter().any(|s| s.field_key == "dates"));
        assert!(a.iter().any(|s| s.field_key == "objectives"));
    }
}
