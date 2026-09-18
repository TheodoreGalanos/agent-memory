use memory_domain::records::{MemoryQuery, ValidTime};

pub(crate) fn interval(time: &ValidTime) -> (i64, Option<i64>, Option<i64>) {
    match time {
        ValidTime::Unknown => (0, None, None),
        ValidTime::Interval { from, to } => (
            1,
            from.map(|t| t.timestamp_millis()),
            to.map(|t| t.timestamp_millis()),
        ),
    }
}

pub(crate) fn filter(sql: &mut String, values: &mut Vec<String>, query: &MemoryQuery) {
    if let Some(cutoff) = query.recorded_as_of {
        values.push(cutoff.to_string());
        let n = values.len();
        sql.push_str(&format!(" AND v.recorded_from <= CAST(${n} AS BIGINT) AND (v.recorded_to IS NULL OR v.recorded_to > CAST(${n} AS BIGINT))"));
    } else {
        sql.push_str(" AND v.recorded_to IS NULL");
    }
    if let Some(at) = query.valid_at {
        values.push(at.timestamp_millis().to_string());
        let n = values.len();
        sql.push_str(&format!(" AND v.valid_known=1 AND (v.valid_from IS NULL OR v.valid_from <= CAST(${n} AS BIGINT)) AND (v.valid_to IS NULL OR v.valid_to > CAST(${n} AS BIGINT))"));
    }
}
