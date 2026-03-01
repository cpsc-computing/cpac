# CPAC Wire Format Specification

Version: 1.0
Copyright (c) 2026 BitConcepts, LLC. All rights reserved.

All multi-byte integers are little-endian unless noted otherwise.

## 1. CP — Standard CPAC Frame

Magic: `"CP"` (0x43 0x50)

```
Offset  Size  Field
──────  ────  ─────────────────────────────
0       2     magic ("CP")
2       1     version (1)
3       2     flags (reserved, 0x0000)
5       1     backend_id
6       4     original_size (LE u32)
10      2     dag_descriptor_len (LE u16)
12      N     dag_descriptor (N = dag_descriptor_len)
12+N    ...   compressed_payload
```

Minimum header: 12 bytes.

### Backend IDs

- `0x00` — Raw (passthrough)
- `0x01` — Zstd
- `0x02` — Brotli

### DAG Descriptor

When `dag_descriptor_len > 0`, the descriptor encodes the transform chain:

```
Offset  Size  Field
──────  ────  ─────────────────────
0       1     transform_count
1       N     transform_ids (1 byte each)
1+N     ...   per-transform: meta_len (LE u16) + meta_bytes
```

Transform IDs are defined in `cpac-dag/src/registry.rs`.

## 2. CPBL — Block-Parallel Frame

Magic: `"CPBL"` (0x43 0x50 0x42 0x4C)

```
Offset  Size  Field
──────  ────  ─────────────────────────────
0       4     magic ("CPBL")
4       1     version (1)
5       4     block_count (LE u32)
9       8     original_size (LE u64)
17      4×N   block_size_table (LE u32 per block)
17+4N   ...   block_payloads (concatenated)
```

Each block payload is a complete CP frame, independently decompressible.
Default block size: 1 MiB. Auto-engaged for inputs > 256 KiB.

## 3. TP — Transform Preprocess Frame

Magic: `"TP"` (0x54 0x50)

```
Offset  Size  Field
──────  ────  ─────────────────────────────
0       2     magic ("TP")
2       1     version (1)
3       1     transform_count
4       N     transform_ids (1 byte each)
4+N     ...   per-transform: param_len (LE u16) + params
...     ...   payload (transformed data)
```

### Transform IDs (TP frame)

- `0x01` — Transpose (params: element_width as LE u16)
- `0x02` — FloatSplit (params: none, self-framed)
- `0x03` — FieldLZ (params: none, self-framed)
- `0x04` — ROLZ (params: none)

Transforms are applied in order during compression and reversed during
decompression. If no TP magic is present, data is treated as raw.

## 4. CS — Streaming Frame

Magic: `"CS"` (0x43 0x53)

```
Offset  Size  Field
──────  ────  ─────────────────────────────
0       2     magic ("CS")
2       1     version (1)
3       4     num_blocks (LE u32)
7       8     original_size (LE u64)
15      4     block_size (LE u32)
19      ...   per-block: compressed_len (LE u32) + block_data
```

Each block is independently compressed via the standard CP pipeline.
Supports both sequential and parallel decompression.

## 5. CPHE — Hybrid Post-Quantum Encryption Frame

Magic: `"CPHE"` (0x43 0x50 0x48 0x45)

```
Offset  Size  Field
──────  ────  ─────────────────────────────
0       4     magic ("CPHE")
4       1     version (1)
5       32    ephemeral_x25519_public
37      2     mlkem_ciphertext_len (LE u16)
39      M     mlkem_ciphertext (M = mlkem_ciphertext_len)
39+M    1     aead_nonce_len
40+M    K     aead_nonce (K = aead_nonce_len)
40+M+K  ...   aead_ciphertext (ChaCha20-Poly1305)
```

Key derivation: HKDF-SHA256 over concatenation of X25519 shared secret
and ML-KEM-768 shared secret, with salt `"CPHE-hybrid-salt"` and info
`"CPHE-hybrid-v1"`.

## 6. CPAR — Multi-File Archive

Magic: `"CPAR"` (0x43 0x50 0x41 0x52)

```
Offset  Size  Field
──────  ────  ─────────────────────────────
0       4     magic ("CPAR")
4       1     version (1)
5       1     flags (reserved, 0x00)
6       4     num_entries (LE u32)
10      ...   entries (sequential)
```

Each entry:

```
Offset  Size  Field
──────  ────  ─────────────────────────────
0       2     path_len (LE u16)
2       P     path (UTF-8, forward slashes)
2+P     8     original_size (LE u64)
10+P    8     compressed_size (LE u64)
18+P    1     flags (reserved)
19+P    8     timestamp (Unix epoch seconds, LE u64)
27+P    C     compressed_data (C = compressed_size, CP frame)
```

## Version History

- v1 (2026-03-01) — Initial specification for all frame types.
