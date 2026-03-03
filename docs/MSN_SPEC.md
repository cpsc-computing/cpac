# MSN Wire Format Specification

## Overview

Multi-Scale Normalization (MSN) metadata is stored in CP2 frame format. The metadata contains extracted semantic fields and domain information, while the residual flows through the normal compression pipeline.

## CP2 Frame Format (with MSN)

```
Offset  Size  Field
------  ----  -----
0       2     Magic: "CP" (0x43 0x50)
2       1     Version: 0x02 (CP2)
3       2     Flags (little-endian u16)
5       1     Backend ID (0=Raw, 1=Zstd, 2=Brotli, 3=Gzip, 4=Lzma)
6       4     Original size (little-endian u32)
10      2     DAG descriptor length (little-endian u16)
12      2     MSN metadata length (little-endian u16)
14      N     DAG descriptor (N bytes, from offset 10)
14+N    M     MSN metadata (M bytes, from offset 12)
14+N+M  ...   Compressed payload
```

## MSN Metadata Format

MSN metadata is JSON-serialized `MsnMetadata` structure:

```json
{
  "version": 1,
  "fields": {
    "field1": "value1",
    "field2": 123,
    ...
  },
  "applied": true,
  "domain_id": "text.json",
  "confidence": 0.85
}
```

### Fields

- **version** (u8): MSN format version. Currently 1.
  - Defaults to 1 if missing (backward compatibility)
  - Future versions may change field structure
  
- **fields** (HashMap<String, JsonValue>): Extracted semantic fields
  - Domain-specific key-value pairs
  - Used for reconstruction during decompression
  - Examples: column headers (CSV), field names (JSON), tags (XML)
  
- **applied** (bool): Whether MSN was actually applied
  - `true`: MSN extraction succeeded, residual is normalized
  - `false`: MSN passthrough, residual is original data
  
- **domain_id** (Option<String>): Domain handler used
  - Format: `"category.type"` (e.g., `"text.json"`, `"log.apache"`)
  - `null` if MSN not applied
  - See Domain Registry for valid IDs
  
- **confidence** (f64): Auto-detection confidence score
  - Range: 0.0 to 1.0
  - Higher = more certain domain match
  - Typical threshold: 0.5

## Compression Pipeline

### Compress (with MSN enabled)

```
Input Data
    ↓
SSR Analysis → Track Selection
    ↓
MSN Extract (if Track 1) → MsnResult { fields, residual }
    ↓
Residual → Transforms → Entropy Coding → Compressed Payload
    ↓
MsnMetadata (fields only) → JSON Serialize → MSN Metadata Bytes
    ↓
CP2 Frame: [Header | DAG | MSN Metadata | Compressed Payload]
```

### Decompress

```
CP2 Frame
    ↓
Decode Frame → Header + MSN Metadata + Compressed Payload
    ↓
Decompress Payload → Entropy Decode → Reverse Transforms → Residual
    ↓
Deserialize MSN Metadata → MsnMetadata
    ↓
MSN Reconstruct: MsnMetadata + Residual → Original Data
```

## Version History

### Version 1 (Current)

- Initial MSN format
- JSON-serialized metadata
- Support for 11 domain handlers:
  - Text: JSON, CSV, XML, YAML
  - Binary: MessagePack, CBOR, Protobuf
  - Logs: Syslog, Apache, JSON Log, Passthrough

## Domain Handler IDs

Domain handlers are registered with unique IDs:

| Domain ID       | Description                  | Target Ratio |
|-----------------|------------------------------|--------------|
| `passthrough`   | No-op (Track 2)              | 1x           |
| `text.json`     | JSON objects                 | >50x         |
| `text.csv`      | CSV/TSV files                | >20x         |
| `text.xml`      | XML/HTML documents           | >15x         |
| `text.yaml`     | YAML documents               | >15x         |
| `binary.msgpack`| MessagePack binary           | >30x         |
| `binary.cbor`   | CBOR binary                  | >30x         |
| `binary.protobuf`| Protocol Buffers            | >40x         |
| `log.syslog`    | RFC 5424 syslog              | >20x         |
| `log.apache`    | Apache Common/Combined logs  | >25x         |
| `log.json`      | JSON Lines (JSONL) logs      | >50x         |

## Backward Compatibility

- CP (v1) frames: No MSN metadata, `msn_metadata` field is empty
- CP2 (v2) frames: May have MSN metadata
- Decompressor auto-detects frame version from byte 2
- Missing `version` field in metadata defaults to 1

## Example

### Input Data (50 bytes)
```json
{"name":"Alice","age":30}
{"name":"Bob","age":25}
```

### MSN Extraction
- Domain: `log.json` (JSONL)
- Confidence: 0.80
- Fields: `{"name": ["Alice", "Bob"], "age": [30, 25]}`
- Residual: 10 bytes (after field extraction)

### CP2 Frame
```
[Header: 14 bytes]
  Version: 2
  Backend: Zstd (1)
  Original size: 50
  MSN metadata length: 85

[MSN Metadata: 85 bytes]
  {"version":1,"fields":{"name":["Alice","Bob"],"age":[30,25]},"applied":true,"domain_id":"log.json","confidence":0.8}

[Compressed Payload: ~8 bytes]
  Zstd-compressed residual (10 bytes → 8 bytes)

Total: 14 + 85 + 8 = 107 bytes
```

**Note**: In this example, MSN increases total size (50→107 bytes) because:
1. Small input (50 bytes)
2. Metadata overhead (85 bytes)
3. MSN is designed for large datasets with repetitive structure

For a 5KB version of the same pattern:
- Without MSN: ~200 bytes (25x)
- With MSN: ~180 bytes (28x) - MSN wins due to amortized metadata

## Implementation Notes

- MSN metadata is **not compressed** in the frame (stored as raw JSON)
- Future optimization: compress MSN metadata for large field sets
- Residual size should be significantly smaller than original to justify MSN
- Typical use case: logs, API responses, exports with 1000+ repetitive records
