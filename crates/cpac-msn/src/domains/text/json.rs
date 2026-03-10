// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! JSON domain handler with semantic field extraction.
//!
//! Supports both single-document JSON and JSONL (newline-delimited JSON).

use crate::domain::{Domain, DomainInfo, ExtractionResult};
use cpac_types::{CpacError, CpacResult};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// JSON domain handler.
///
/// Extracts repeated field names and structure from JSON data.
/// Target compression: 50-100x on repetitive JSON.
pub struct JsonDomain;

impl JsonDomain {
    // The helpers below (`build_field_index`, `extract_field_names`, `extract_single`,
    // `compact_json`) are kept for backward-compatible reconstruction of CP2 frames
    // that were produced by older CPAC builds using single-doc JSON extraction.
    // They are not called on the compress path any more.
    #[allow(dead_code)]
    /// Build a (`field_map`, `field_names`) pair from field-count data.
    #[allow(clippy::cast_possible_truncation)]
    fn build_field_index(
        field_counts: &HashMap<String, u32>,
    ) -> (HashMap<String, u32>, Vec<String>) {
        let mut repeated: Vec<String> = field_counts
            .iter()
            .filter(|(_, &c)| c >= 2)
            .map(|(n, _)| n.clone())
            .collect();
        repeated.sort();
        let mut map = HashMap::new();
        for (idx, name) in repeated.iter().enumerate() {
            map.insert(name.clone(), idx as u32); // bounded by field count, << u32::MAX
        }
        (map, repeated)
    }

    /// Extract field names from JSON value recursively.
    #[allow(dead_code)]
    fn extract_field_names(value: &Value, fields: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                for (key, val) in map {
                    fields.push(key.clone());
                    Self::extract_field_names(val, fields);
                }
            }
            Value::Array(arr) => {
                for val in arr {
                    Self::extract_field_names(val, fields);
                }
            }
            _ => {}
        }
    }

    /// Compact JSON by extracting repeated field names.
    fn compact_json(value: &Value, field_map: &HashMap<String, u32>) -> Value {
        match value {
            Value::Object(map) => {
                let mut new_map = serde_json::Map::new();
                for (key, val) in map {
                    // Replace field name with index if it appears multiple times
                    let new_key = if let Some(&idx) = field_map.get(key) {
                        format!("${idx}")
                    } else {
                        key.clone()
                    };
                    new_map.insert(new_key, Self::compact_json(val, field_map));
                }
                Value::Object(new_map)
            }
            Value::Array(arr) => Value::Array(
                arr.iter()
                    .map(|v| Self::compact_json(v, field_map))
                    .collect(),
            ),
            other => other.clone(),
        }
    }

    /// Reconstruct JSON by restoring field names from index.
    fn expand_json(value: &Value, field_names: &[String]) -> Value {
        match value {
            Value::Object(map) => {
                let mut new_map = serde_json::Map::new();
                for (key, val) in map {
                    let new_key = key
                        .strip_prefix('$')
                        .and_then(|s| s.parse::<usize>().ok())
                        .and_then(|idx| field_names.get(idx).cloned())
                        .unwrap_or_else(|| key.clone());
                    new_map.insert(new_key, Self::expand_json(val, field_names));
                }
                Value::Object(new_map)
            }
            Value::Array(arr) => Value::Array(
                arr.iter()
                    .map(|v| Self::expand_json(v, field_names))
                    .collect(),
            ),
            other => other.clone(),
        }
    }

    /// Internal: extract a single-document JSON block.
    #[allow(dead_code)]
    fn extract_single(data: &[u8], value: &Value) -> CpacResult<ExtractionResult> {
        let mut all_field_names = Vec::new();
        Self::extract_field_names(value, &mut all_field_names);
        let mut field_counts: HashMap<String, u32> = HashMap::new();
        for name in &all_field_names {
            *field_counts.entry(name.clone()).or_insert(0) += 1;
        }
        let (field_map, repeated_fields) = Self::build_field_index(&field_counts);
        let compacted = Self::compact_json(value, &field_map);
        let residual = serde_json::to_vec(&compacted)
            .map_err(|e| CpacError::CompressFailed(format!("text.json serialize: {e}")))?;
        let mut fields = HashMap::new();
        fields.insert(
            "field_names".to_string(),
            Value::Array(repeated_fields.into_iter().map(Value::String).collect()),
        );
        fields.insert(
            "original_size".to_string(),
            Value::Number(data.len().into()),
        );
        Ok(ExtractionResult {
            fields,
            residual,
            metadata: HashMap::new(),
            domain_id: "text.json".to_string(),
        })
    }

    // -----------------------------------------------------------------------
    // Single-doc key dedup (byte-level, preserves whitespace for zstd)
    // -----------------------------------------------------------------------

    /// Count how many bytes a key-dedup pass would save.
    ///
    /// Returns `(repeated_keys, total_byte_savings)`.  The caller uses this to
    /// gate `detect()` confidence: only enable MSN when savings are meaningful.
    fn keydedup_savings(data: &[u8]) -> (usize, usize) {
        let positions = Self::scan_json_key_positions(data);
        let mut freq: HashMap<&[u8], usize> = HashMap::new();
        for &(start, end) in &positions {
            *freq.entry(&data[start..end]).or_insert(0) += 1;
        }
        let mut repeated: Vec<(&[u8], usize)> = freq
            .into_iter()
            .filter(|(_, count)| *count >= 2)
            .collect();
        repeated.sort_by(|a, b| b.1.cmp(&a.1));

        let mut total_savings: usize = 0;
        for (i, (key_bytes, count)) in repeated.iter().enumerate() {
            let orig_len = key_bytes.len(); // includes quotes: "key"
            let token_len = Self::token_len(i); // length of "$xx" token
            if orig_len > token_len {
                total_savings += (orig_len - token_len) * count;
            }
        }
        (repeated.len(), total_savings)
    }

    /// Byte-level single-doc key dedup extraction.
    ///
    /// Scans the raw JSON bytes for repeated key strings (`"key":`), replaces
    /// them with short index tokens (`"$00":`, `"$01":`, …), and returns the
    /// modified byte stream as the residual.  All whitespace, indentation,
    /// and value formatting is preserved so zstd still benefits from those
    /// repetition patterns.
    fn extract_single_keydedup(data: &[u8]) -> CpacResult<ExtractionResult> {
        let positions = Self::scan_json_key_positions(data);

        // Count frequency of each key string (including quotes).
        let mut freq: HashMap<Vec<u8>, usize> = HashMap::new();
        for &(start, end) in &positions {
            *freq.entry(data[start..end].to_vec()).or_insert(0) += 1;
        }

        // Build dedup map: repeated keys sorted by frequency (descending).
        let mut repeated: Vec<(Vec<u8>, usize)> = freq
            .into_iter()
            .filter(|(_, count)| *count >= 2)
            .collect();
        repeated.sort_by(|a, b| b.1.cmp(&a.1));

        // Map: original key bytes (with quotes) → index
        let key_to_idx: HashMap<Vec<u8>, usize> = repeated
            .iter()
            .enumerate()
            .map(|(i, (k, _))| (k.clone(), i))
            .collect();

        // Extract key names (without quotes) for metadata.
        let field_names: Vec<String> = repeated
            .iter()
            .map(|(k, _)| {
                // k includes quotes: "keyname" → keyname
                String::from_utf8_lossy(&k[1..k.len() - 1]).into_owned()
            })
            .collect();

        // Build residual: replace key strings with short tokens.
        // Process positions in reverse order so byte offsets stay valid.
        let mut residual = data.to_vec();
        let mut sorted_positions: Vec<(usize, usize)> = positions
            .iter()
            .filter(|&&(s, e)| key_to_idx.contains_key(&data[s..e]))
            .copied()
            .collect();
        sorted_positions.sort_by(|a, b| b.0.cmp(&a.0)); // reverse order

        for (start, end) in sorted_positions {
            let key_bytes = data[start..end].to_vec();
            if let Some(&idx) = key_to_idx.get(&key_bytes) {
                let token = Self::make_token(idx);
                residual.splice(start..end, token.into_iter());
            }
        }

        let mut fields = HashMap::new();
        fields.insert(
            "field_names".to_string(),
            Value::Array(field_names.into_iter().map(Value::String).collect()),
        );
        fields.insert(
            "format".to_string(),
            Value::String("single_keydedup".to_string()),
        );
        fields.insert(
            "original_size".to_string(),
            Value::Number(data.len().into()),
        );

        Ok(ExtractionResult {
            fields,
            residual,
            metadata: HashMap::new(),
            domain_id: "text.json".to_string(),
        })
    }

    /// Reconstruct single-doc JSON from key-dedup residual.
    fn reconstruct_single_keydedup(
        residual: &[u8],
        field_names: &[String],
    ) -> CpacResult<Vec<u8>> {
        // Scan the residual for token key positions and replace back.
        let positions = Self::scan_json_key_positions(residual);
        let mut output = residual.to_vec();

        // Process in reverse order to preserve offsets.
        let mut restore_positions: Vec<(usize, usize, usize)> = Vec::new(); // (start, end, key_idx)
        for &(start, end) in &positions {
            if let Some(idx) = Self::parse_token(&residual[start..end]) {
                if idx < field_names.len() {
                    restore_positions.push((start, end, idx));
                }
            }
        }
        restore_positions.sort_by(|a, b| b.0.cmp(&a.0)); // reverse order

        for (start, end, idx) in restore_positions {
            let original_key = format!("\"{}\"" , field_names[idx]);
            output.splice(start..end, original_key.bytes());
        }

        Ok(output)
    }

    /// Scan JSON bytes for key string positions.
    ///
    /// Returns `(start, end)` byte offsets for each `"key"` string that is
    /// followed by optional whitespace and a colon.  The range includes the
    /// enclosing double-quotes.
    fn scan_json_key_positions(data: &[u8]) -> Vec<(usize, usize)> {
        let mut positions = Vec::new();
        let mut i = 0;
        while i < data.len() {
            if data[i] == b'"' {
                let str_start = i;
                i += 1;
                // Scan to closing quote, handling escapes.
                while i < data.len() {
                    if data[i] == b'\\' {
                        i += 2; // skip escaped char
                        continue;
                    }
                    if data[i] == b'"' {
                        let str_end = i + 1; // past closing quote
                        i += 1;
                        // Check if followed by optional whitespace + colon.
                        let mut j = i;
                        while j < data.len() && data[j].is_ascii_whitespace() {
                            j += 1;
                        }
                        if j < data.len() && data[j] == b':' {
                            positions.push((str_start, str_end));
                        }
                        break;
                    }
                    i += 1;
                }
            } else {
                i += 1;
            }
        }
        positions
    }

    /// Length of the token string for key index `idx` (including quotes).
    fn token_len(idx: usize) -> usize {
        // Tokens: "$00" through "$ff" = 5 bytes (with quotes)
        // "$100" through "$fff" = 6 bytes
        if idx < 256 {
            5 // "$xx"
        } else {
            6 // "$xxx"
        }
    }

    /// Build the token bytes for key index `idx` (with quotes).
    fn make_token(idx: usize) -> Vec<u8> {
        if idx < 256 {
            format!("\"${:02x}\"", idx).into_bytes()
        } else {
            format!("\"${:03x}\"", idx).into_bytes()
        }
    }

    /// Parse a token string back to its key index.
    /// Input is the quoted string including quotes, e.g. `"$0a"`.
    /// Returns `None` if not a valid token.
    fn parse_token(quoted: &[u8]) -> Option<usize> {
        if quoted.len() < 4 || quoted[0] != b'"' || quoted[quoted.len() - 1] != b'"' {
            return None;
        }
        let inner = &quoted[1..quoted.len() - 1];
        if inner.first() != Some(&b'$') {
            return None;
        }
        let hex = &inner[1..];
        if hex.len() < 2 || hex.len() > 3 {
            return None;
        }
        if !hex.iter().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        usize::from_str_radix(
            std::str::from_utf8(hex).ok()?,
            16,
        )
        .ok()
    }

    /// Build a columnar residual from parsed JSONL rows.
    ///
    /// Wire format:
    /// ```text
    /// [0x02 magic: 1B]
    /// [num_cols: u32 LE]
    /// [row_count: u32 LE]
    /// For each column (in field_names order):
    ///   [col_data_len: u32 LE]
    ///   [col_data: newline-separated JSON values]
    /// ```
    /// Integer-only columns are delta-encoded: first value absolute,
    /// subsequent values as signed deltas, space-separated.
    #[allow(clippy::cast_possible_truncation)] // lengths bounded by document size, << u32::MAX
    fn build_columnar_residual(rows: &[Value], field_names: &[String]) -> CpacResult<Vec<u8>> {
        let num_cols = field_names.len();
        let row_count = rows.len();

        // Extract per-column values.
        let mut columns: Vec<Vec<Value>> = vec![Vec::with_capacity(row_count); num_cols];

        for row in rows {
            if let Value::Object(map) = row {
                for (fi, fname) in field_names.iter().enumerate() {
                    columns[fi].push(map.get(fname).cloned().unwrap_or(Value::Null));
                }
            } else {
                // Non-object rows: fill all columns with null.
                for col in &mut columns {
                    col.push(Value::Null);
                }
            }
        }

        // Encode each column.
        let mut encoded_cols: Vec<Vec<u8>> = Vec::with_capacity(num_cols);
        for col in &columns {
            let is_all_int = col
                .iter()
                .all(|v| matches!(v, Value::Number(n) if n.is_i64() || n.is_u64()));
            let mut buf = Vec::new();
            if is_all_int && !col.is_empty() {
                // Delta-encode consecutive integers.
                let mut prev: i64 = 0;
                for (i, v) in col.iter().enumerate() {
                    let cur = v.as_i64().unwrap_or(0);
                    if i == 0 {
                        let s = cur.to_string();
                        buf.extend_from_slice(s.as_bytes());
                        prev = cur;
                    } else {
                        let delta = cur - prev;
                        buf.push(b' ');
                        let s = delta.to_string();
                        buf.extend_from_slice(s.as_bytes());
                        prev = cur;
                    }
                }
            } else {
                // Encode as JSON values, newline-separated.
                for (i, v) in col.iter().enumerate() {
                    if i > 0 {
                        buf.push(b'\n');
                    }
                    let s = serde_json::to_vec(v).map_err(|e| {
                        CpacError::CompressFailed(format!("text.json col encode: {e}"))
                    })?;
                    buf.extend_from_slice(&s);
                }
            }
            encoded_cols.push(buf);
        }

        // Write wire format. Lengths are bounded by document size, safely < u32::MAX.
        let total_col_bytes: usize = encoded_cols.iter().map(|c| 4 + c.len()).sum();
        let mut out = Vec::with_capacity(1 + 4 + 4 + total_col_bytes);
        out.push(0x02u8); // columnar magic
        out.extend_from_slice(&(num_cols as u32).to_le_bytes());
        out.extend_from_slice(&(row_count as u32).to_le_bytes());
        for col_buf in &encoded_cols {
            out.extend_from_slice(&(col_buf.len() as u32).to_le_bytes());
            out.extend_from_slice(col_buf);
        }
        Ok(out)
    }

    /// Reconstruct rows from a columnar residual.
    #[allow(clippy::cast_possible_truncation)] // u32 wire values fit usize on 32+ bit targets
    fn reconstruct_columnar(
        residual: &[u8],
        field_names: &[String],
        trailing_newline: bool,
    ) -> CpacResult<Vec<u8>> {
        if residual.len() < 9 {
            return Err(CpacError::DecompressFailed(
                "text.json columnar: truncated header".into(),
            ));
        }
        let num_cols =
            u32::from_le_bytes([residual[1], residual[2], residual[3], residual[4]]) as usize;
        let row_count =
            u32::from_le_bytes([residual[5], residual[6], residual[7], residual[8]]) as usize;
        let mut cursor = 9usize;

        if num_cols != field_names.len() {
            return Err(CpacError::DecompressFailed(
                "text.json columnar: column count mismatch".into(),
            ));
        }

        // Decode each column.
        let mut columns: Vec<Vec<Value>> = Vec::with_capacity(num_cols);
        for _fi in 0..num_cols {
            if cursor + 4 > residual.len() {
                return Err(CpacError::DecompressFailed(
                    "text.json columnar: truncated col len".into(),
                ));
            }
            let col_len = u32::from_le_bytes([
                residual[cursor],
                residual[cursor + 1],
                residual[cursor + 2],
                residual[cursor + 3],
            ]) as usize;
            cursor += 4;
            if cursor + col_len > residual.len() {
                return Err(CpacError::DecompressFailed(
                    "text.json columnar: truncated col data".into(),
                ));
            }
            let col_bytes = &residual[cursor..cursor + col_len];
            cursor += col_len;

            // Detect integer delta-encoding: only digits, '-', ' '.
            let is_delta = !col_bytes.is_empty()
                && col_bytes
                    .iter()
                    .all(|&b| b.is_ascii_digit() || b == b'-' || b == b' ');

            let col_values: Vec<Value> = if is_delta {
                let text = std::str::from_utf8(col_bytes)
                    .map_err(|e| CpacError::DecompressFailed(format!("text.json col utf8: {e}")))?;
                let mut vals = Vec::with_capacity(row_count);
                let mut running: i64 = 0;
                for (i, token) in text.split(' ').enumerate() {
                    let n: i64 = token.parse().map_err(|e| {
                        CpacError::DecompressFailed(format!("text.json col delta parse: {e}"))
                    })?;
                    if i == 0 {
                        running = n;
                    } else {
                        running += n;
                    }
                    vals.push(Value::Number(running.into()));
                }
                vals
            } else {
                let mut vals = Vec::with_capacity(row_count);
                for chunk in col_bytes.split(|&b| b == b'\n') {
                    if chunk.is_empty() {
                        continue;
                    }
                    let v: Value = serde_json::from_slice(chunk).map_err(|e| {
                        CpacError::DecompressFailed(format!("text.json col parse: {e}"))
                    })?;
                    vals.push(v);
                }
                vals
            };
            columns.push(col_values);
        }

        // Zip columns back to rows.
        let mut output: Vec<u8> = Vec::new();
        for row_idx in 0..row_count {
            let mut map = serde_json::Map::new();
            for (fi, fname) in field_names.iter().enumerate() {
                let v = columns
                    .get(fi)
                    .and_then(|col| col.get(row_idx))
                    .cloned()
                    .unwrap_or(Value::Null);
                if v != Value::Null {
                    map.insert(fname.clone(), v);
                }
            }
            let row_bytes = serde_json::to_vec(&Value::Object(map)).map_err(|e| {
                CpacError::DecompressFailed(format!("text.json row serialize: {e}"))
            })?;
            output.extend_from_slice(&row_bytes);
            output.push(b'\n');
        }
        if !trailing_newline && output.last() == Some(&b'\n') {
            output.pop();
        }
        Ok(output)
    }

    /// Internal: extract JSONL (newline-delimited JSON) blocks using columnar layout.
    ///
    /// Strict: any non-empty line that fails JSON parsing causes this to return
    /// an error, which callers interpret as "not JSONL, use passthrough".
    fn extract_jsonl(data: &[u8]) -> CpacResult<ExtractionResult> {
        // Parse every non-empty line; fail on the first invalid line.
        let lines: Vec<Value> = data
            .split(|&b| b == b'\n')
            .enumerate()
            .filter_map(|(i, raw)| {
                let l = raw.strip_suffix(b"\r").unwrap_or(raw);
                if l.is_empty() {
                    None
                } else {
                    Some(serde_json::from_slice::<Value>(l).map_err(|e| {
                        CpacError::CompressFailed(format!("text.json: JSONL line {i} invalid: {e}"))
                    }))
                }
            })
            .collect::<CpacResult<Vec<Value>>>()?;
        if lines.is_empty() {
            return Err(CpacError::CompressFailed(
                "text.json: data is not valid JSON or JSONL".into(),
            ));
        }
        // Collect field names in first-occurrence document order to preserve JSON key ordering.
        let mut repeated_fields: Vec<String> = Vec::new();
        {
            let mut seen: HashSet<String> = HashSet::new();
            for val in &lines {
                if let Value::Object(map) = val {
                    for key in map.keys() {
                        if seen.insert(key.clone()) {
                            repeated_fields.push(key.clone());
                        }
                    }
                }
            }
        }
        // Build columnar residual.
        let residual = Self::build_columnar_residual(&lines, &repeated_fields)?;
        let trailing_newline = data.last() == Some(&b'\n');
        let mut fields = HashMap::new();
        fields.insert(
            "field_names".to_string(),
            Value::Array(repeated_fields.into_iter().map(Value::String).collect()),
        );
        fields.insert("format".to_string(), Value::String("jsonl".to_string()));
        fields.insert(
            "original_size".to_string(),
            Value::Number(data.len().into()),
        );
        fields.insert(
            "trailing_newline".to_string(),
            Value::Bool(trailing_newline),
        );
        Ok(ExtractionResult {
            fields,
            residual,
            metadata: HashMap::new(),
            domain_id: "text.json".to_string(),
        })
    }
}

impl Domain for JsonDomain {
    fn info(&self) -> DomainInfo {
        DomainInfo {
            id: "text.json",
            name: "JSON",
            extensions: &[".json", ".jsonl"],
            mime_types: &["application/json", "text/json"],
            magic_bytes: &[b"{", b"["],
        }
    }

    fn detect(&self, data: &[u8], filename: Option<&str>) -> f64 {
        if let Some(fname) = filename {
            if std::path::Path::new(fname)
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("jsonl"))
            {
                return 0.95;
            }
            // For .json extension: only beneficial if the content is JSONL
            // (columnar transform helps) OR single-doc with enough key repetition
            // for byte-level key dedup.
            if std::path::Path::new(fname)
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("json"))
            {
                // Check if it's JSONL in a .json file (first line = valid JSON object)
                let nl = memchr::memchr(b'\n', data).unwrap_or(data.len());
                let first_line = data[..nl].strip_suffix(b"\r").unwrap_or(&data[..nl]);
                if !first_line.is_empty()
                    && serde_json::from_slice::<serde_json::Value>(first_line).is_ok()
                    && serde_json::from_slice::<serde_json::Value>(data).is_err()
                {
                    return 0.9; // JSONL with .json extension
                }
                // Single-doc .json: check if key dedup would save enough bytes.
                let (repeated_keys, byte_savings) = Self::keydedup_savings(data);
                if repeated_keys >= 3 && byte_savings > data.len() / 20 {
                    return 0.7; // Enough key repetition for byte-level dedup
                }
                return 0.2;
            }
        }

        // Content-based detection (no filename or unrecognised extension).
        let start = data
            .iter()
            .position(|b| !b.is_ascii_whitespace())
            .unwrap_or(data.len());
        if start >= data.len() {
            return 0.0;
        }
        let first_byte = data[start];
        if first_byte != b'{' && first_byte != b'[' {
            return 0.0;
        }

        // Single-doc JSON: check if byte-level key dedup helps.
        if serde_json::from_slice::<Value>(data).is_ok() {
            let (repeated_keys, byte_savings) = Self::keydedup_savings(data);
            if repeated_keys >= 3 && byte_savings > data.len() / 20 {
                return 0.7;
            }
            return 0.2;
        }

        // Check if it's JSONL: first line must be a valid JSON object.
        let nl = memchr::memchr(b'\n', data).unwrap_or(data.len());
        // Guard: if the file starts with newlines (start > nl), the first line
        // is empty — not JSONL.
        if start > nl {
            return 0.0;
        }
        let first_line = data[start..nl]
            .strip_suffix(b"\r")
            .unwrap_or(&data[start..nl]);
        if !first_line.is_empty() && serde_json::from_slice::<Value>(first_line).is_ok() {
            return 0.85; // Likely JSONL
        }

        0.0 // Starts with JSON magic but unparseable and not JSONL
    }

    fn extract(&self, data: &[u8]) -> CpacResult<ExtractionResult> {
        // Try JSONL first (multi-line, columnar extraction).
        if let Ok(result) = Self::extract_jsonl(data) {
            return Ok(result);
        }
        // Single-doc: use byte-level key dedup (preserves whitespace).
        Self::extract_single_keydedup(data)
    }

    fn extract_with_fields(
        &self,
        data: &[u8],
        fields: &HashMap<String, serde_json::Value>,
    ) -> CpacResult<ExtractionResult> {
        let field_names_value = fields.get("field_names").ok_or_else(|| {
            CpacError::CompressFailed("text.json: missing field_names in metadata".into())
        })?;

        let field_names: Vec<String> = if let Value::Array(arr) = field_names_value {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        } else {
            return Err(CpacError::CompressFailed(
                "text.json: invalid field_names format".into(),
            ));
        };

        let is_jsonl = fields.get("format").and_then(Value::as_str) == Some("jsonl");

        if is_jsonl {
            // Parse lines and produce columnar residual (same format as extract_jsonl).
            let lines: Vec<Value> = data
                .split(|&b| b == b'\n')
                .enumerate()
                .filter_map(|(i, raw)| {
                    let l = raw.strip_suffix(b"\r").unwrap_or(raw);
                    if l.is_empty() {
                        None
                    } else {
                        Some(serde_json::from_slice::<Value>(l).map_err(|e| {
                            CpacError::CompressFailed(format!(
                                "text.json JSONL line parse: {i} {e}"
                            ))
                        }))
                    }
                })
                .collect::<CpacResult<Vec<Value>>>()?;
            let residual = Self::build_columnar_residual(&lines, &field_names)?;
            let mut result_fields = fields.clone();
            result_fields.insert(
                "original_size".to_string(),
                Value::Number(data.len().into()),
            );
            result_fields.insert(
                "trailing_newline".to_string(),
                Value::Bool(data.last() == Some(&b'\n')),
            );
            return Ok(ExtractionResult {
                fields: result_fields,
                residual,
                metadata: HashMap::new(),
                domain_id: "text.json".to_string(),
            });
        }

        // Single-document JSON path.
        let value: Value = serde_json::from_slice(data)
            .map_err(|e| CpacError::CompressFailed(format!("text.json parse error: {e}")))?;
        let mut field_map = HashMap::new();
        #[allow(clippy::cast_possible_truncation)]
        for (idx, name) in field_names.iter().enumerate() {
            field_map.insert(name.clone(), idx as u32); // bounded by field count, << u32::MAX
        }
        let compacted = Self::compact_json(&value, &field_map);
        let residual = serde_json::to_vec(&compacted)
            .map_err(|e| CpacError::CompressFailed(format!("text.json serialize error: {e}")))?;
        let mut result_fields = HashMap::new();
        result_fields.insert("field_names".to_string(), field_names_value.clone());
        result_fields.insert(
            "original_size".to_string(),
            Value::Number(data.len().into()),
        );
        Ok(ExtractionResult {
            fields: result_fields,
            residual,
            metadata: HashMap::new(),
            domain_id: "text.json".to_string(),
        })
    }

    fn reconstruct(&self, result: &ExtractionResult) -> CpacResult<Vec<u8>> {
        let field_names_value = result
            .fields
            .get("field_names")
            .ok_or_else(|| CpacError::DecompressFailed("text.json: missing field_names".into()))?;
        let field_names: Vec<String> = if let Value::Array(arr) = field_names_value {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        } else {
            return Err(CpacError::DecompressFailed(
                "text.json: invalid field_names format".into(),
            ));
        };

        let is_jsonl = result.fields.get("format").and_then(Value::as_str) == Some("jsonl");
        if is_jsonl {
            let trailing_newline = result
                .fields
                .get("trailing_newline")
                .and_then(Value::as_bool)
                .unwrap_or(true); // default true for backward compat
                                  // Columnar residual (produced by extract_jsonl) starts with 0x02.
            if result.residual.first() == Some(&0x02u8) {
                return Self::reconstruct_columnar(
                    &result.residual,
                    &field_names,
                    trailing_newline,
                );
            }
            // Legacy row-oriented JSONL path (backward compat for old frames).
            let mut output: Vec<u8> = Vec::with_capacity(result.residual.len());
            for raw_line in result.residual.split(|&b| b == b'\n') {
                let line = raw_line.strip_suffix(b"\r").unwrap_or(raw_line);
                if line.is_empty() {
                    continue;
                }
                let compacted: Value = serde_json::from_slice(line).map_err(|e| {
                    CpacError::DecompressFailed(format!("text.json JSONL parse: {e}"))
                })?;
                let expanded = Self::expand_json(&compacted, &field_names);
                let bytes = serde_json::to_vec(&expanded).map_err(|e| {
                    CpacError::DecompressFailed(format!("text.json JSONL serialize: {e}"))
                })?;
                output.extend_from_slice(&bytes);
                output.push(b'\n');
            }
            if !trailing_newline && output.last() == Some(&b'\n') {
                output.pop();
            }
            return Ok(output);
        }

        // Single-doc key dedup path (byte-level, preserves whitespace).
        let is_keydedup = result
            .fields
            .get("format")
            .and_then(Value::as_str)
            == Some("single_keydedup");
        if is_keydedup {
            return Self::reconstruct_single_keydedup(&result.residual, &field_names);
        }

        // Legacy single-document JSON path (re-serialization).
        let compacted: Value = serde_json::from_slice(&result.residual)
            .map_err(|e| CpacError::DecompressFailed(format!("text.json parse error: {e}")))?;
        let expanded = Self::expand_json(&compacted, &field_names);
        serde_json::to_vec(&expanded)
            .map_err(|e| CpacError::DecompressFailed(format!("text.json serialize error: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_domain_detection() {
        let domain = JsonDomain;

        // Single-doc JSON: confidence below min_confidence (0.5) — passthrough
        assert!(domain.detect(br#"{"key": "value"}"#, None) < 0.5);
        assert!(domain.detect(br#"[1, 2, 3]"#, None) < 0.5);

        // .json extension single-doc: below min_confidence
        assert!(domain.detect(br#"{"x":1}"#, Some("test.json")) < 0.5);

        // JSONL: first line is valid JSON, whole file is not single-doc
        let jsonl = b"{\"a\":1}\n{\"a\":2}\n{\"a\":3}\n";
        assert!(domain.detect(jsonl, None) > 0.7);

        // .jsonl extension
        assert!(domain.detect(b"", Some("events.jsonl")) > 0.9);

        // Non-JSON
        assert!(domain.detect(b"plain text", None) < 0.1);
    }

    #[test]
    fn json_domain_jsonl_detection() {
        let domain = JsonDomain;
        // Filename-based JSONL detection
        assert!(domain.detect(b"", Some("events.jsonl")) > 0.9);
        // Content-based JSONL detection (first line is valid JSON object, whole is not)
        let jsonl = b"{\"a\":1}\n{\"a\":2}\n{\"a\":3}\n";
        let confidence = domain.detect(jsonl, None);
        assert!(confidence > 0.7, "expected > 0.7, got {confidence}");
    }

    #[test]
    fn json_domain_jsonl_roundtrip() {
        let domain = JsonDomain;
        let data = b"{\"user\":\"alice\",\"score\":100}\n{\"user\":\"bob\",\"score\":200}\n{\"user\":\"charlie\",\"score\":150}\n";

        let result = domain.extract(data).unwrap();

        // Should detect JSONL format
        assert_eq!(
            result
                .fields
                .get("format")
                .and_then(serde_json::Value::as_str),
            Some("jsonl")
        );
        // Residual should be smaller (field names extracted)
        assert!(
            result.residual.len() < data.len(),
            "residual {} >= original {}",
            result.residual.len(),
            data.len()
        );

        let reconstructed = domain.reconstruct(&result).unwrap();
        // Compare parsed objects line by line
        let orig_lines: Vec<serde_json::Value> = data
            .split(|&b| b == b'\n')
            .filter(|l| !l.is_empty())
            .map(|l| serde_json::from_slice(l).unwrap())
            .collect();
        let recon_lines: Vec<serde_json::Value> = reconstructed
            .split(|&b| b == b'\n')
            .filter(|l| !l.is_empty())
            .map(|l| serde_json::from_slice(l).unwrap())
            .collect();
        assert_eq!(orig_lines, recon_lines);
    }

    #[test]
    fn json_domain_jsonl_extract_with_fields() {
        let domain = JsonDomain;
        // Two-record JSONL blocks so extract() takes the JSONL path (not single-doc).
        let block1 = b"{\"ts\":\"2026-01-01\",\"level\":\"INFO\",\"msg\":\"start\"}\n\
{\"ts\":\"2026-01-01\",\"level\":\"DEBUG\",\"msg\":\"init\"}\n";
        let block2 = b"{\"ts\":\"2026-01-02\",\"level\":\"WARN\",\"msg\":\"slow\"}\n\
{\"ts\":\"2026-01-02\",\"level\":\"ERROR\",\"msg\":\"fail\"}\n";

        let detection = domain.extract(block1).unwrap();
        assert_eq!(
            detection
                .fields
                .get("format")
                .and_then(serde_json::Value::as_str),
            Some("jsonl"),
            "expected JSONL format to be detected"
        );

        // Apply same field map to block2
        let result2 = domain
            .extract_with_fields(block2, &detection.fields)
            .unwrap();
        let recon2 = domain.reconstruct(&result2).unwrap();

        // Compare line-by-line as parsed JSON
        let orig_lines: Vec<serde_json::Value> = block2
            .split(|&b| b == b'\n')
            .filter(|l| !l.is_empty())
            .map(|l| serde_json::from_slice(l).unwrap())
            .collect();
        let recon_lines: Vec<serde_json::Value> = recon2
            .split(|&b| b == b'\n')
            .filter(|l| !l.is_empty())
            .map(|l| serde_json::from_slice(l).unwrap())
            .collect();
        assert_eq!(orig_lines, recon_lines);
    }

    /// Verify the residual uses the 0x02 columnar magic byte.
    #[test]
    fn json_domain_jsonl_columnar_magic() {
        let domain = JsonDomain;
        let data = b"{\"a\":1,\"b\":2}\n{\"a\":3,\"b\":4}\n";
        let result = domain.extract(data).unwrap();
        assert_eq!(
            result
                .fields
                .get("format")
                .and_then(serde_json::Value::as_str),
            Some("jsonl")
        );
        assert_eq!(
            result.residual.first(),
            Some(&0x02u8),
            "columnar magic 0x02 expected"
        );
    }

    /// Integer-only columns must be delta-encoded (space-separated tokens, all digits/'-').
    #[test]
    fn json_domain_jsonl_numeric_delta() {
        let domain = JsonDomain;
        // scores are 100, 200, 150 → delta: "100 100 -50"
        let data = b"{\"user\":\"alice\",\"score\":100}\n{\"user\":\"bob\",\"score\":200}\n{\"user\":\"charlie\",\"score\":150}\n";
        let result = domain.extract(data).unwrap();
        assert_eq!(result.residual.first(), Some(&0x02u8));

        // Locate the score column (field_names are sorted alphabetically: ["score", "user"]).
        let field_names = result
            .fields
            .get("field_names")
            .and_then(|v| v.as_array())
            .unwrap();
        let score_col_idx = field_names
            .iter()
            .position(|v| v.as_str() == Some("score"))
            .unwrap();

        // Parse the wire format to reach the score column's bytes.
        let r = &result.residual;
        let num_cols = u32::from_le_bytes([r[1], r[2], r[3], r[4]]) as usize;
        assert_eq!(num_cols, 2);
        let mut cursor = 9usize;
        let mut col_bufs: Vec<&[u8]> = Vec::with_capacity(num_cols);
        for _ in 0..num_cols {
            let col_len =
                u32::from_le_bytes([r[cursor], r[cursor + 1], r[cursor + 2], r[cursor + 3]])
                    as usize;
            cursor += 4;
            col_bufs.push(&r[cursor..cursor + col_len]);
            cursor += col_len;
        }
        let score_bytes = col_bufs[score_col_idx];
        // Must be all digits, '-', or ' ' (delta encoding).
        assert!(
            score_bytes
                .iter()
                .all(|&b| b.is_ascii_digit() || b == b'-' || b == b' '),
            "score column should be delta-encoded, got: {:?}",
            std::str::from_utf8(score_bytes)
        );
        // Full roundtrip still correct.
        let reconstructed = domain.reconstruct(&result).unwrap();
        let orig: Vec<serde_json::Value> = data
            .split(|&b| b == b'\n')
            .filter(|l| !l.is_empty())
            .map(|l| serde_json::from_slice(l).unwrap())
            .collect();
        let recon: Vec<serde_json::Value> = reconstructed
            .split(|&b| b == b'\n')
            .filter(|l| !l.is_empty())
            .map(|l| serde_json::from_slice(l).unwrap())
            .collect();
        assert_eq!(orig, recon);
    }

    /// Single-doc JSON key dedup: byte-perfect roundtrip.
    #[test]
    fn json_single_doc_keydedup_roundtrip() {
        // CloudFormation-like template with repeated keys.
        let data = br#"{
  "AWSTemplateFormatVersion": "2010-09-09",
  "Resources": {
    "MyVPC": {
      "Type": "AWS::EC2::VPC",
      "Properties": {
        "CidrBlock": "10.0.0.0/16"
      }
    },
    "MySubnet": {
      "Type": "AWS::EC2::Subnet",
      "Properties": {
        "VpcId": {"Ref": "MyVPC"},
        "CidrBlock": "10.0.1.0/24"
      }
    },
    "MyInstance": {
      "Type": "AWS::EC2::Instance",
      "Properties": {
        "SubnetId": {"Ref": "MySubnet"},
        "InstanceType": "t3.micro"
      }
    },
    "MySecurityGroup": {
      "Type": "AWS::EC2::SecurityGroup",
      "Properties": {
        "GroupDescription": "Allow SSH",
        "VpcId": {"Ref": "MyVPC"}
      }
    },
    "MyRole": {
      "Type": "AWS::IAM::Role",
      "Properties": {
        "AssumeRolePolicyDocument": {
          "Version": "2012-10-17",
          "Statement": [{"Effect": "Allow"}]
        }
      }
    }
  }
}"#;

        let result = JsonDomain::extract_single_keydedup(data).unwrap();
        assert_eq!(
            result.fields.get("format").and_then(|v| v.as_str()),
            Some("single_keydedup")
        );
        // Residual should be smaller than original.
        assert!(
            result.residual.len() < data.len(),
            "residual {} should be < original {}",
            result.residual.len(),
            data.len()
        );

        // Byte-perfect roundtrip.
        let field_names: Vec<String> = result
            .fields
            .get("field_names")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        let reconstructed =
            JsonDomain::reconstruct_single_keydedup(&result.residual, &field_names).unwrap();
        assert_eq!(
            data.as_slice(),
            reconstructed.as_slice(),
            "byte-perfect roundtrip failed"
        );
    }

    /// Single-doc key dedup detection: enough repetition → high confidence.
    #[test]
    fn json_single_doc_keydedup_detection() {
        let domain = JsonDomain;
        // Template with 5+ repeated keys should get confidence > 0.5.
        let data = br#"{
  "Resources": {
    "A": {"Type": "X", "Properties": {"CidrBlock": "a"}},
    "B": {"Type": "Y", "Properties": {"CidrBlock": "b"}},
    "C": {"Type": "Z", "Properties": {"CidrBlock": "c"}},
    "D": {"Type": "W", "Properties": {"CidrBlock": "d"}},
    "E": {"Type": "V", "Properties": {"CidrBlock": "e"}}
  }
}"#;
        let conf = domain.detect(data, Some("template.json"));
        assert!(conf > 0.5, "expected > 0.5, got {conf}");
    }

    /// Key dedup scanner correctly identifies key positions.
    #[test]
    fn json_keydedup_scan_positions() {
        let data = br#"{"name": "alice", "age": 30}"#;
        let positions = JsonDomain::scan_json_key_positions(data);
        assert_eq!(positions.len(), 2);
        assert_eq!(&data[positions[0].0..positions[0].1], b"\"name\"");
        assert_eq!(&data[positions[1].0..positions[1].1], b"\"age\"");
    }

    /// Token roundtrip: make_token → parse_token.
    #[test]
    fn json_keydedup_token_roundtrip() {
        for idx in [0, 1, 15, 127, 255, 256, 4095] {
            let token = JsonDomain::make_token(idx);
            let parsed = JsonDomain::parse_token(&token);
            assert_eq!(parsed, Some(idx), "token roundtrip failed for idx {idx}");
        }
    }

    /// Trailing-newline flag is respected: data without a trailing '\n' should
    /// reconstruct without one.
    #[test]
    fn json_domain_jsonl_columnar_trailing_newline_preserved() {
        let domain = JsonDomain;
        // No trailing newline.
        let data = b"{\"x\":1,\"y\":\"a\"}\n{\"x\":2,\"y\":\"b\"}";
        let result = domain.extract(data).unwrap();
        assert_eq!(result.residual.first(), Some(&0x02u8));
        assert_eq!(
            result
                .fields
                .get("trailing_newline")
                .and_then(serde_json::Value::as_bool),
            Some(false)
        );
        let reconstructed = domain.reconstruct(&result).unwrap();
        assert!(
            !reconstructed.ends_with(b"\n"),
            "reconstructed data should not end with newline"
        );
        // With trailing newline.
        let data2 = b"{\"x\":3,\"y\":\"c\"}\n{\"x\":4,\"y\":\"d\"}\n";
        let result2 = domain.extract(data2).unwrap();
        let recon2 = domain.reconstruct(&result2).unwrap();
        assert!(
            recon2.ends_with(b"\n"),
            "reconstructed data should end with newline"
        );
    }
}
