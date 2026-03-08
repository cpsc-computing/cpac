// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Entropy conditioning: byte classification and stream partitioning.
//!
//! Splits data into entropy-homogeneous streams so each can be compressed
//! independently with better symbol distribution stability.
//!
//! Four byte classes:
//! - **Structural**: delimiters, brackets, colons, equals, etc.
//! - **Numeric**: digits, decimal points, sign chars, exponent markers
//! - **Text**: printable ASCII not in the above categories
//! - **HighEntropy**: non-ASCII, control chars, binary data

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc
)]

/// Byte class for entropy conditioning.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ByteClass {
    /// Structural delimiters: `{}[]()<>,:;=&|!@#` and whitespace outside strings.
    Structural = 0,
    /// Numeric: `0-9`, `.`, `+`, `-`, `e`, `E`, `x`, `X` (in numeric contexts).
    Numeric = 1,
    /// Text: printable ASCII not classified as Structural or Numeric.
    Text = 2,
    /// High-entropy: non-ASCII, control chars, binary data.
    HighEntropy = 3,
}

impl ByteClass {
    /// Number of distinct classes.
    pub const COUNT: usize = 4;

    /// Convert from discriminant byte.
    #[must_use]
    pub fn from_u8(b: u8) -> Option<Self> {
        match b {
            0 => Some(ByteClass::Structural),
            1 => Some(ByteClass::Numeric),
            2 => Some(ByteClass::Text),
            3 => Some(ByteClass::HighEntropy),
            _ => None,
        }
    }
}

/// Classification context for context-aware byte classification.
#[derive(Clone, Debug, Default)]
struct ClassifyContext {
    in_string: bool,
    prev_byte: u8,
}

/// Classify a single byte given context.
fn classify_byte(b: u8, ctx: &ClassifyContext) -> ByteClass {
    // Inside quoted strings, everything is Text (except non-ASCII)
    if ctx.in_string {
        return if b.is_ascii() {
            ByteClass::Text
        } else {
            ByteClass::HighEntropy
        };
    }

    // Non-ASCII or control chars (except common whitespace)
    if !b.is_ascii() || (b < 0x20 && b != b'\n' && b != b'\r' && b != b'\t' && b != b' ') {
        return ByteClass::HighEntropy;
    }

    // Structural characters
    if matches!(
        b,
        b'{' | b'}'
            | b'['
            | b']'
            | b'('
            | b')'
            | b'<'
            | b'>'
            | b','
            | b':'
            | b';'
            | b'='
            | b'&'
            | b'|'
            | b'\n'
            | b'\r'
            | b'\t'
    ) {
        return ByteClass::Structural;
    }

    // Numeric: digits, and contextual numeric chars
    if b.is_ascii_digit() {
        return ByteClass::Numeric;
    }
    // Decimal point, sign, or exponent marker adjacent to digits
    if matches!(b, b'.' | b'+' | b'-' | b'e' | b'E')
        && ctx.prev_byte.is_ascii_digit()
    {
        return ByteClass::Numeric;
    }
    // Hex markers
    if matches!(b, b'x' | b'X') && ctx.prev_byte == b'0' {
        return ByteClass::Numeric;
    }
    // Hex digits after 0x
    if b.is_ascii_hexdigit() && !b.is_ascii_digit() && ctx.prev_byte.is_ascii_hexdigit() {
        return ByteClass::Numeric;
    }

    // Space: classify as Structural (whitespace between tokens)
    if b == b' ' {
        return ByteClass::Structural;
    }

    // Everything else that's printable ASCII is Text
    ByteClass::Text
}

/// Classify all bytes in the data, returning a class per byte.
#[must_use]
pub fn classify(data: &[u8]) -> Vec<ByteClass> {
    let mut classes = Vec::with_capacity(data.len());
    let mut ctx = ClassifyContext::default();
    let mut escape_next = false;

    for &b in data {
        if escape_next {
            escape_next = false;
            classes.push(if ctx.in_string {
                ByteClass::Text
            } else {
                classify_byte(b, &ctx)
            });
            ctx.prev_byte = b;
            continue;
        }

        if b == b'\\' && ctx.in_string {
            escape_next = true;
            classes.push(ByteClass::Text);
            ctx.prev_byte = b;
            continue;
        }

        if b == b'"' {
            ctx.in_string = !ctx.in_string;
        }

        let class = classify_byte(b, &ctx);
        classes.push(class);
        ctx.prev_byte = b;
    }

    classes
}

/// Result of stream partitioning.
#[derive(Clone, Debug)]
pub struct PartitionResult {
    /// Streams in class order (Structural, Numeric, Text, HighEntropy).
    /// Empty streams are included (length 0) to preserve index stability.
    pub streams: [Vec<u8>; ByteClass::COUNT],
    /// RLE-encoded position map: sequence of (class_id, run_length) pairs.
    /// Used to reconstruct original byte order during merge.
    pub position_map: Vec<(u8, u32)>,
}

/// Partition data into entropy-homogeneous streams.
///
/// Returns streams + position map for reconstruction.
#[must_use]
pub fn partition(data: &[u8]) -> PartitionResult {
    let classes = classify(data);
    let mut streams: [Vec<u8>; ByteClass::COUNT] = Default::default();
    let mut position_map: Vec<(u8, u32)> = Vec::new();

    for (i, (&b, &class)) in data.iter().zip(classes.iter()).enumerate() {
        let class_id = class as u8;
        streams[class_id as usize].push(b);

        // RLE: extend current run or start new one
        if i > 0 && !position_map.is_empty() && position_map.last().unwrap().0 == class_id {
            position_map.last_mut().unwrap().1 += 1;
        } else {
            position_map.push((class_id, 1));
        }
    }

    PartitionResult {
        streams,
        position_map,
    }
}

/// Merge streams back into original byte order using the position map.
///
/// # Errors
///
/// Returns error if position map references more bytes than available in streams.
pub fn merge(result: &PartitionResult) -> cpac_types::CpacResult<Vec<u8>> {
    let total_len: usize = result.streams.iter().map(|s| s.len()).sum();
    let mut out = Vec::with_capacity(total_len);
    let mut offsets = [0usize; ByteClass::COUNT];

    for &(class_id, run_len) in &result.position_map {
        let cid = class_id as usize;
        if cid >= ByteClass::COUNT {
            return Err(cpac_types::CpacError::Transform(
                "conditioning: invalid class id in position map".into(),
            ));
        }
        for _ in 0..run_len {
            if offsets[cid] >= result.streams[cid].len() {
                return Err(cpac_types::CpacError::Transform(
                    "conditioning: position map overflows stream".into(),
                ));
            }
            out.push(result.streams[cid][offsets[cid]]);
            offsets[cid] += 1;
        }
    }

    Ok(out)
}

/// Serialize a `PartitionResult` into a compact binary format.
///
/// Format:
/// `[num_runs: 4 LE][runs: (class_id:1, run_len:4 LE)...][stream_lens: 4×4 LE][streams...]`
#[must_use]
pub fn serialize_partition(result: &PartitionResult) -> Vec<u8> {
    let mut out = Vec::new();

    // Position map
    out.extend_from_slice(&(result.position_map.len() as u32).to_le_bytes());
    for &(class_id, run_len) in &result.position_map {
        out.push(class_id);
        out.extend_from_slice(&run_len.to_le_bytes());
    }

    // Stream lengths
    for stream in &result.streams {
        out.extend_from_slice(&(stream.len() as u32).to_le_bytes());
    }

    // Stream data
    for stream in &result.streams {
        out.extend_from_slice(stream);
    }

    out
}

/// Deserialize a `PartitionResult` from the compact binary format.
pub fn deserialize_partition(data: &[u8]) -> cpac_types::CpacResult<PartitionResult> {
    if data.len() < 4 {
        return Err(cpac_types::CpacError::Transform(
            "conditioning: data too short for header".into(),
        ));
    }

    let num_runs =
        u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    let mut offset = 4;

    let mut position_map = Vec::with_capacity(num_runs);
    for _ in 0..num_runs {
        if offset + 5 > data.len() {
            return Err(cpac_types::CpacError::Transform(
                "conditioning: truncated position map".into(),
            ));
        }
        let class_id = data[offset];
        offset += 1;
        let run_len = u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);
        offset += 4;
        position_map.push((class_id, run_len));
    }

    // Stream lengths
    if offset + 4 * ByteClass::COUNT > data.len() {
        return Err(cpac_types::CpacError::Transform(
            "conditioning: truncated stream lengths".into(),
        ));
    }
    let mut stream_lens = [0usize; ByteClass::COUNT];
    for sl in &mut stream_lens {
        *sl = u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]) as usize;
        offset += 4;
    }

    // Stream data
    let mut streams: [Vec<u8>; ByteClass::COUNT] = Default::default();
    for (i, stream) in streams.iter_mut().enumerate() {
        let len = stream_lens[i];
        if offset + len > data.len() {
            return Err(cpac_types::CpacError::Transform(
                "conditioning: truncated stream data".into(),
            ));
        }
        *stream = data[offset..offset + len].to_vec();
        offset += len;
    }

    Ok(PartitionResult {
        streams,
        position_map,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_json() {
        let data = br#"{"key": 123, "val": 4.5}"#;
        let classes = classify(data);
        // '{' should be Structural
        assert_eq!(classes[0], ByteClass::Structural);
        // '"' introduces string context
        // 'k' inside string should be Text
        assert_eq!(classes[2], ByteClass::Text);
        // '1' should be Numeric
        assert_eq!(classes[8], ByteClass::Numeric);
    }

    #[test]
    fn roundtrip_partition_merge() {
        let data = br#"{"name": "Alice", "age": 30, "score": 95.5}"#;
        let result = partition(data);
        let merged = merge(&result).unwrap();
        assert_eq!(merged, data);
    }

    #[test]
    fn roundtrip_serialize_deserialize() {
        let data = br#"[1, 2, 3, "hello", {"x": 99}]"#;
        let result = partition(data);
        let serialized = serialize_partition(&result);
        let deserialized = deserialize_partition(&serialized).unwrap();
        let merged = merge(&deserialized).unwrap();
        assert_eq!(merged, data);
    }

    #[test]
    fn empty_data() {
        let result = partition(b"");
        assert!(result.position_map.is_empty());
        for s in &result.streams {
            assert!(s.is_empty());
        }
        let merged = merge(&result).unwrap();
        assert!(merged.is_empty());
    }

    #[test]
    fn binary_data_high_entropy() {
        let data: Vec<u8> = (128..200).collect();
        let classes = classify(&data);
        assert!(classes.iter().all(|c| *c == ByteClass::HighEntropy));
    }

    #[test]
    fn numeric_stream_separation() {
        let data = b"value=12345,count=67890";
        let result = partition(data);
        // Numeric stream should contain all digits
        let numeric = &result.streams[ByteClass::Numeric as usize];
        assert!(numeric.iter().all(|b| b.is_ascii_digit()));
        assert_eq!(numeric.len(), 10); // 12345 + 67890
    }

    #[test]
    fn roundtrip_with_escapes() {
        let data = br#"{"msg": "hello \"world\""}"#;
        let result = partition(data);
        let merged = merge(&result).unwrap();
        assert_eq!(merged, data);
    }
}
