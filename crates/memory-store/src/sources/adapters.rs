use crate::{Error, Result};
use memory_domain::sources::{Locator, SourceKind, ToolEvent};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;

pub trait SourceAdapter {
    fn kind(&self) -> SourceKind;
    fn media_type(&self) -> &'static str;
    fn normalize(&self, bytes: &[u8]) -> Result<Vec<u8>>;
    fn read(&self, snapshot: &[u8], locator: &Locator) -> Result<Value>;
}

pub struct DocumentAdapter;
impl SourceAdapter for DocumentAdapter {
    fn kind(&self) -> SourceKind {
        SourceKind::Document
    }
    fn media_type(&self) -> &'static str {
        "text/plain; charset=utf-8"
    }
    fn normalize(&self, bytes: &[u8]) -> Result<Vec<u8>> {
        text(bytes)?;
        Ok(bytes.to_vec())
    }
    fn read(&self, snapshot: &[u8], locator: &Locator) -> Result<Value> {
        let Locator::Document {
            start_line,
            end_line,
            ..
        } = locator
        else {
            return Err(wrong_locator());
        };
        let lines: Vec<_> = text(snapshot)?.lines().collect();
        let range = line_range(*start_line, *end_line, lines.len())?;
        Ok(Value::String(lines[range].join("\n")))
    }
}

#[derive(Serialize, Deserialize)]
struct Table {
    columns: Vec<String>,
    rows: Vec<Vec<String>>,
}
pub struct TableAdapter;
impl SourceAdapter for TableAdapter {
    fn kind(&self) -> SourceKind {
        SourceKind::Table
    }
    fn media_type(&self) -> &'static str {
        "application/json"
    }
    fn normalize(&self, bytes: &[u8]) -> Result<Vec<u8>> {
        let mut reader = csv::Reader::from_reader(bytes);
        let columns: Vec<_> = reader
            .headers()
            .map_err(csv_error)?
            .iter()
            .map(str::to_owned)
            .collect();
        let unique: HashSet<_> = columns.iter().collect();
        if columns.is_empty()
            || unique.len() != columns.len()
            || columns.iter().any(|name| name.trim().is_empty())
        {
            return Err(Error::Invalid(
                "Table headers must be non-empty and distinct".into(),
            ));
        }
        let rows = reader
            .records()
            .map(|row| Ok(row.map_err(csv_error)?.iter().map(str::to_owned).collect()))
            .collect::<Result<_>>()?;
        Ok(serde_json::to_vec(&Table { columns, rows })?)
    }
    fn read(&self, snapshot: &[u8], locator: &Locator) -> Result<Value> {
        let Locator::Table { row, column } = locator else {
            return Err(wrong_locator());
        };
        let table: Table = serde_json::from_slice(snapshot)?;
        let index = row.checked_sub(1).ok_or_else(wrong_locator)? as usize;
        let values = table.rows.get(index).ok_or(Error::NotFound)?;
        if values.len() != table.columns.len() {
            return Err(Error::Invalid(
                "Snapshot row does not match its headers".into(),
            ));
        }
        if let Some(column) = column {
            let index = table
                .columns
                .iter()
                .position(|name| name == column)
                .ok_or(Error::NotFound)?;
            Ok(Value::String(values[index].clone()))
        } else {
            Ok(Value::Object(
                table
                    .columns
                    .into_iter()
                    .zip(values.iter().cloned().map(Value::String))
                    .collect(),
            ))
        }
    }
}

pub struct ModelAdapter;
impl SourceAdapter for ModelAdapter {
    fn kind(&self) -> SourceKind {
        SourceKind::Model
    }
    fn media_type(&self) -> &'static str {
        "application/json"
    }
    fn normalize(&self, bytes: &[u8]) -> Result<Vec<u8>> {
        let value: Value = serde_json::from_slice(bytes)?;
        let entities = value
            .get("entities")
            .and_then(Value::as_object)
            .ok_or_else(|| Error::Invalid("Model snapshot needs an entities object".into()))?;
        if entities
            .values()
            .any(|entity| !entity.get("properties").is_some_and(Value::is_object))
        {
            return Err(Error::Invalid(
                "Each model entity needs a properties object".into(),
            ));
        }
        Ok(serde_json::to_vec(&value)?)
    }
    fn read(&self, snapshot: &[u8], locator: &Locator) -> Result<Value> {
        let Locator::Model { entity, property } = locator else {
            return Err(wrong_locator());
        };
        let model: Value = serde_json::from_slice(snapshot)?;
        model
            .get("entities")
            .and_then(|entities| entities.get(entity))
            .and_then(|entity| entity.get("properties"))
            .and_then(|properties| properties.get(property))
            .cloned()
            .ok_or(Error::NotFound)
    }
}

#[derive(Serialize, Deserialize)]
struct Trajectory {
    schema_version: TrajectoryVersion,
    events: Vec<ToolEvent>,
}
#[derive(Serialize, Deserialize)]
enum TrajectoryVersion {
    #[serde(rename = "memory-tool-events/1")]
    V1,
}
pub struct ToolEventAdapter;
impl SourceAdapter for ToolEventAdapter {
    fn kind(&self) -> SourceKind {
        SourceKind::ToolEvents
    }
    fn media_type(&self) -> &'static str {
        "application/json"
    }
    fn normalize(&self, bytes: &[u8]) -> Result<Vec<u8>> {
        let trajectory: Trajectory = serde_json::from_slice(bytes)?;
        let mut ids = HashSet::new();
        for event in &trajectory.events {
            if event.event_id.is_empty() || event.kind.is_empty() || !ids.insert(&event.event_id) {
                return Err(Error::Invalid("Events need distinct IDs and a kind".into()));
            }
        }
        Ok(serde_json::to_vec(&trajectory)?)
    }
    fn read(&self, snapshot: &[u8], locator: &Locator) -> Result<Value> {
        let Locator::Events { start, end } = locator else {
            return Err(wrong_locator());
        };
        let trajectory: Trajectory = serde_json::from_slice(snapshot)?;
        let range = line_range(*start, *end, trajectory.events.len())?;
        Ok(json!(trajectory.events[range]))
    }
}

fn text(bytes: &[u8]) -> Result<&str> {
    std::str::from_utf8(bytes).map_err(|_| {
        Error::Invalid(
            "Document adapter expects UTF-8 text; native binary files need an extraction adapter"
                .into(),
        )
    })
}
fn wrong_locator() -> Error {
    Error::Invalid("Locator is incompatible with this source".into())
}
fn line_range(start: u32, end: u32, length: usize) -> Result<std::ops::Range<usize>> {
    if start == 0 || start > end || end as usize > length {
        return Err(wrong_locator());
    }
    Ok((start - 1) as usize..end as usize)
}
fn csv_error(error: csv::Error) -> Error {
    Error::Invalid(error.to_string())
}
