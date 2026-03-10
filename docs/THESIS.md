# CPAC: Constraint-Projected Adaptive Compression

### A Structured, Adaptive Data-Reduction Engine for Modern Compression Pipelines

MERKURIAL  
March 2026

**Patent Notice:**  
CPAC is patent pending: *U.S. Provisional Patent Application No. 63/980,251, filed February 11, 2026.*

---

# Abstract

Modern lossless compression systems are highly effective at exploiting
statistical redundancy in byte streams, but they generally remain indifferent
to the semantic structure of the data they compress. This limitation becomes
increasingly important in contemporary workloads dominated by structured logs,
cloud configuration bundles, telemetry streams, API payloads, archives, and
typed binary formats, where a substantial fraction of redundancy exists above
the byte level in repeated keys, schema elements, field relationships, and
other structural constraints.

This thesis presents **Constraint-Projected Adaptive Compression (CPAC)**, a
multi-stage adaptive data-reduction system that combines structural analysis,
semantic normalization, reversible transform composition, multi-backend entropy
coding, streaming, archiving, and integrated cryptographic protection within a
single architecture. CPAC does not replace established entropy coders such as
Zstandard, Brotli, gzip, or LZMA. Instead, it improves their operating
conditions by transforming data into representations that are more compressible,
more structured, and more amenable to backend specialization.

CPAC begins with a lightweight **Structural Summary Record (SSR)** pass that
measures entropy, ASCII ratio, byte-distribution properties, and domain
signals to determine whether input should follow a generic or structure-aware
path. When exploitable structure is present, **Multi-Scale Normalization
(MSN)** extracts repeated semantic elements from formats such as JSON, CSV,
XML, YAML, MessagePack, CBOR, Protocol Buffers, and common log types. CPAC
then applies an adaptive DAG of reversible transforms, selects an appropriate
entropy backend, and packages the result into a self-describing frame,
streaming, archive, or encrypted container format. The system also includes
parallel block compression, memory-mapped I/O for large files, dictionary
support, profiling and calibration tools, SIMD acceleration, hardware-aware
execution paths, and hybrid post-quantum encryption and signature support.

CPAC is therefore better understood not as a single compressor, but as a
constraint-projected adaptive compression architecture: a system that uses
structural knowledge, transform diversity, backend routing, and deployment
awareness to outperform static one-size-fits-all compression strategies on
heterogeneous modern data.

---

# 1. Introduction

Lossless compression remains a foundational systems primitive. Storage
platforms, distributed services, databases, archives, backup systems, and
network protocols all rely on compression to reduce cost, improve throughput,
and constrain operational footprint. For decades, the dominant model of
general-purpose compression has been remarkably successful: treat the input as
a byte stream, detect repeated substrings and statistical regularities, and
encode them compactly using a backend entropy coder.

That model works well, but it is also fundamentally incomplete.

A substantial fraction of modern data is not arbitrary byte noise. It is
structured, repetitive, and constrained. JSON records repeat field names.
CSV exports repeat column identities and type patterns. XML documents repeat
tags and attributes. Log streams repeat timestamps, message templates, and
format scaffolding. Typed binary formats such as Protocol Buffers, CBOR, and
MessagePack encode schema-regular data whose redundancy is only partially
visible at the raw-byte level. Cloud configuration bundles contain both strong
within-file structure and extensive cross-file similarity. These are not
special cases; they are increasingly the default operating environment for
modern infrastructure.

Traditional compressors can exploit repeated bytes in such data, but they
cannot directly reason about what parts of the representation are structurally
implied, what parts are independent, and what preprocessing path best suits
the domain. They compress what the bytes look like; they do not natively model
what the data means.

Constraint-Projected Adaptive Compression (CPAC) addresses this gap by
treating compression as both a **structure problem** and a **pipeline-selection
problem**.

Its central principle is:

> encode only the independent information required to reconstruct a valid
> structured state, while repeated, derived, or constrained components are
> normalized, projected, or regenerated during decompression.

This principle leads naturally to a broader systems view of compression.
Instead of hard-coding a single strategy for every file type, CPAC analyzes
the input, routes it through an appropriate processing track, optionally
extracts semantic structure, composes reversible transforms, selects an entropy
backend, and emits the result in a format suitable for single-file,
block-parallel, streaming, archival, or encrypted workflows.

CPAC is therefore not merely a new compressor. It is an **adaptive
data-reduction engine** designed for heterogeneous workloads and modern
deployment realities.

---

# 2. Background and Related Work

## 2.1 General-Purpose Compression Algorithms

The current compression ecosystem spans a wide speed–ratio design space.

| Compressor | Best fit | Speed/ratio profile | Typical use case |
|---|---|---|---|
| **zstd** | general-purpose / cloud | wide tuning range; fast decode | files, backups, storage systems, pipelines |
| **gzip / deflate** | compatibility | moderate speed and ratio | `.gz`, HTTP content encoding, legacy tooling |
| **brotli** | web distribution | slower compression, denser text output | HTML, CSS, JS, fonts, static assets |
| **lz4** | realtime / low latency | extremely fast compression and decompression | caches, databases, streaming, hot-path I/O |
| **snappy** | analytics | prioritizes speed over ratio | data pipelines, large-scale analytics |
| **xz / lzma2** | archival | slow compression, high ratio | package distribution, cold storage |
| **bzip2** | legacy archival | older ecosystem, often stronger than gzip | existing `.bz2` workflows |
| **lzfse** | Apple ecosystem | deflate-like ratio at higher speed | Apple-oriented general-purpose compression |
| **lzo** | fast legacy | very fast, lower ratio | embedded and older Linux workflows |
| **zpaq** | niche maximum compression | extremely slow, very high ratio | experimental archival use |
| **ppmd** | text-heavy compression | slow, strong on text | text-centric archival workloads |
| **lzip** | reliability-oriented archival | slower but strong | archival workflows favoring `.lz` ecosystem |

These tools are commonly evaluated on benchmark corpora such as Calgary,
Canterbury, and Silesia. They remain extremely valuable, but they are still
predominantly **format-agnostic**: they operate on opaque byte streams and do
not natively exploit semantic structure.

## 2.2 Structured Compression

A range of systems have attempted to improve on the byte-stream model through:

- schema-aware compression in databases,
- columnar compression in analytic storage engines,
- format-specific compressors for structured text and binary formats,
- preprocessing transforms prior to entropy coding,
- dictionary training and solid archive strategies for homogeneous corpora.

These approaches can achieve substantial gains in specific domains, but they
often remain narrow in scope, require explicit format declarations, or fail to
unify compression, archiving, streaming, and security into a single deployable
system.

CPAC takes a broader position. It combines structure-aware preprocessing,
adaptive transform composition, backend routing, archive logic, streaming,
profiling, and cryptographic protection into one architecture.

---

# 3. CPAC Design Overview

CPAC is organized as a modular, multi-stage pipeline.

Figure 1 illustrates the end-to-end architecture. The system first performs a
lightweight structural analysis pass. Structured inputs are routed to a
semantic normalization stage, while opaque or weakly structured inputs bypass
that stage. Both paths then enter a transform DAG, after which an entropy
backend is selected and the result is packaged into the appropriate CPAC wire
format.

```
markdown
                     +--------------------+
                     |     Input Data     |
                     +---------+----------+
                               |
                               v
                    +---------------------+
                    |  SSR Analysis       |
                    |  (entropy, ASCII,   |
                    |  domain detection)  |
                    +---------+-----------+
                              |
                              v
                   +----------------------+
                   |   Track Selection    |
                   +----------+-----------+
                              |
           +------------------+------------------+
           |                                     |
           v                                     v
 +----------------------+           +----------------------+
 |  Structured Track    |           |   Generic Track      |
 |  MSN Normalization   |           |  Direct Compression  |
 +----------+-----------+           +----------+-----------+
            |                                  |
            v                                  v
    +----------------------+        +----------------------+
    | Constraint-Aware     |        | Raw Byte Stream      |
    | Representation       |        |                      |
    +----------+-----------+        +----------+-----------+
               |                               |
               +---------------+---------------+
                               |
                               v
                    +----------------------+
                    |  Transform DAG       |
                    |  (adaptive pipeline) |
                    +----------+-----------+
                               |
                               v
                    +----------------------+
                    | Adaptive Backend     |
                    | Selection            |
                    +----------+-----------+
                               |
         +-----------+----------+----------+-----------+
         |           |                     |           |
         v           v                     v           v
     +------+    +--------+            +-------+    +------+
     | Zstd |    | Brotli |            | Gzip  |    | LZMA |
     +------+    +--------+            +-------+    +------+
          \          |                     |          /
           \         |                     |         /
            +--------+---------------------+--------+
                               |
                               v
                     +---------------------+
                     |  CPAC Container     |
                     |  (CP / CP2 / CPBL   |
                     |   CS / CPAR / CPHE) |
                     +----------+----------+
                                |
                                v
                       +------------------+
                       | Compressed Output|
                       +------------------+
```

**Figure 1.** CPAC system architecture. Input data first undergoes Structural
Summary Record (SSR) analysis, which determines whether the data follows a
structure-aware or generic compression path. Structured inputs are processed
through Multi-Scale Normalization (MSN) to extract semantic redundancy, while
generic inputs bypass this stage. Both paths converge at the adaptive transform
DAG, after which CPAC selects an appropriate entropy backend and packages the
result into one of several container formats.

The remainder of the thesis examines these stages in turn.

---

# 4. Structural Summary Record (SSR)

SSR is the gatekeeper for the entire system. It always runs first, operates in
a single linear pass, and is designed to be inexpensive enough that its
overhead is negligible relative to the compression decision it enables.

At a high level, SSR computes:

* Shannon entropy estimate,
* ASCII ratio,
* byte-distribution characteristics,
* coarse domain hints,
* structural viability score.

Conceptually, SSR answers three questions:

1. Does the input have exploitable structure?
2. What broad domain does it resemble?
3. Which pipeline family is likely to provide the best speed–ratio outcome?

Based on these signals, SSR routes input into one of two tracks:

| Track       | Description                  |
| ----------- | ---------------------------- |
| **Track 1** | structure-aware processing   |
| **Track 2** | generic adaptive compression |

This design prevents a common failure mode of structure-aware systems:
incurring parsing and metadata overhead on data that is fundamentally
unstructured. If SSR routes to Track 2, CPAC behaves like a high-performance
adaptive general-purpose compressor. If SSR routes to Track 1, the richer
semantic path is activated.

---

# 5. Multi-Scale Normalization (MSN)

MSN is CPAC’s semantic extraction layer. It operates only when SSR selects the
structured-data path, making the relationship between the two stages strictly
sequential rather than competitive: SSR is the cheap filter, MSN is the deeper
extractor.

MSN works by:

1. auto-detecting the data format,
2. extracting repeated structural elements,
3. preserving the residual values,
4. storing compact metadata needed for deterministic reconstruction.

For structured formats, repeated tokens often dominate the representation.
MSN attacks that redundancy directly. Instead of repeatedly encoding field
names, tags, headers, or message templates, it stores them once and compresses
primarily the unique value stream.

CPAC supports structure-aware handling across a broad set of domains,
including:

* JSON and JSON Lines,
* CSV and related tabular text,
* XML and HTML-like markup,
* YAML,
* MessagePack,
* CBOR,
* Protocol Buffers,
* syslog,
* Apache logs,
* JSON logs and other structured log formats.

This stage is one of CPAC’s central differentiators. On sufficiently large and
repetitive structured datasets, semantic extraction reduces redundancy that
generic entropy coders can observe only indirectly.

CPAC also preserves practical selectivity: MSN is not forced onto every file.
Small files, already compressed media, random data, executables, and similar
inputs can bypass it when the overhead would not be justified.

---

# 6. Transform Pipeline and Constraint Projection

After SSR and optional MSN, CPAC applies a reversible transform pipeline
described through a Directed Acyclic Graph (DAG). The DAG model treats
transforms as composable building blocks rather than as a fixed preprocessing
sequence.

CPAC includes a broad transform vocabulary spanning text, numeric, binary, and
pattern-oriented operations. Across the full system, this includes transforms
such as:

* delta encoding,
* ZigZag,
* transpose,
* byte-plane separation,
* float splitting,
* tokenize,
* prefix stripping,
* deduplication,
* reduced-offset LZ (ROLZ),
* BWT chains,
* range packing,
* field-oriented transforms,
* projection-oriented transforms.

Built-in profiles such as **Auto**, **Fast**, **Balanced**, **Max**, and
**Text** allow transform composition to reflect workload priorities, while the
Auto profile is guided by SSR analysis and profiling feedback.

This is where the “constraint-projected” identity of CPAC becomes substantive.
The system does not merely reorder bytes for better entropy coding; it attempts
to map data toward a representation in which only the independent degrees of
freedom remain expensive to encode. Range constraints, enums, constants,
monotonicity, and functional relationships can all contribute to transform
selection and structured reduction.

Constraint projection in CPAC is therefore both conceptual and operational:
data is analyzed for what must be stored, what can be normalized, and what can
be reconstructed under discovered constraints.

---

# 7. Adaptive Backend Routing

CPAC deliberately separates preprocessing from entropy coding so that it can
leverage mature backends rather than reinvent them as a monolith.

The current backend set comprises:

* **Raw** passthrough,
* **Zstd**,
* **Brotli**,
* **gzip**,
* **LZMA**.

This diversity is important because the optimal codec depends on the data and
the workload objective. Zstd is often the balanced choice for speed and decode
performance. Brotli is valuable for ratio-sensitive structured text. gzip
remains strategically important for compatibility. LZMA occupies the
high-ratio archival niche. Raw passthrough prevents wasted effort on
already-compressed or incompressible data.

CPAC uses SSR signals, transform context, file size, and profiling feedback to
choose among these backends dynamically. It is therefore not “one more entropy
coder,” but an adaptive routing layer across multiple entropy coders.

---

# 8. Wire Formats, Streaming, and Archiving

CPAC defines a family of wire formats rather than a single output container.

The core formats include:

* **CP** — baseline single-frame format,
* **CP2** — MSN-augmented frame format,
* **CPBL** — block-parallel format,
* **TP** — transform preprocess frame,
* **CS** — streaming frame,
* **CPAR** — multi-file archive format,
* **CPHE** — hybrid post-quantum encryption frame,
* **CPCE** — compressed-and-encrypted container mode.

This format family is central to the system’s thesis position. Many
compressors stop at “produce compressed bytes.” CPAC instead supports:

* single-file compression,
* structure-aware MSN compression,
* independently decompressible parallel blocks,
* bounded-memory streaming,
* multi-file archiving with metadata preservation,
* encrypted and signed containers.

The archive layer is especially important for real-world infrastructure
bundles. CPAR supports both per-file and solid-style modes, allowing CPAC to
exploit cross-file redundancy when beneficial.

---

# 9. Security and Post-Quantum Cryptography

CPAC integrates cryptography into the pipeline rather than relegating it to an
external toolchain concern. This makes compression, packaging, and protection
part of a single data-reduction workflow.

The security model includes:

* AEAD encryption via **ChaCha20-Poly1305** and **AES-256-GCM**,
* password-based encryption using **Argon2id**,
* hybrid **X25519 + ML-KEM-768** key establishment,
* post-quantum signatures using **ML-DSA-65**,
* classical signature support for interoperability and hybrid modes,
* self-describing algorithm identifiers and negotiation-ready format design.

The hybrid encryption model is especially important during the post-quantum
transition period. Combining classical and post-quantum primitives provides
defense in depth while maintaining operational flexibility.

This integration matters because real infrastructure rarely stops at
compression. Data must also be protected at rest and in transit. CPAC treats
that requirement as native, not incidental.

---

# 10. Streaming, Memory, and Datacenter Execution

CPAC is designed for practical deployment in environments where files are
large, memory budgets are bounded, and throughput matters.

Its execution model includes:

* bounded-memory streaming via the **CS** format,
* configurable streaming block sizes,
* parallel block compression,
* automatic memory-mapped I/O for large files,
* adjustable thread counts and memory caps,
* presets tuned for different operational priorities.

These capabilities make CPAC usable in settings such as:

* log ingestion pipelines,
* backup and archival services,
* cloud object-storage workflows,
* containerized batch systems,
* serverless or memory-constrained execution environments.

CPAC also exposes C/C++ FFI and Python integration paths, enabling embedding in
broader systems without requiring an all-Rust deployment.

---

# 11. Hardware Acceleration and Performance Infrastructure

A key point from the original thesis is that CPAC is not merely “future-ready”
for acceleration; it already includes meaningful performance infrastructure and
is explicitly architected for datacenter execution.

This occurs at three levels.

## 11.1 SIMD Acceleration

CPAC includes runtime SIMD dispatch across:

* AVX-512,
* AVX2,
* SSE4.1,
* SSE2,
* NEON,
* scalar fallback.

SIMD-accelerated kernels are implemented for operations such as delta
encoding/decoding, ZigZag transforms, and transpose paths. This allows CPAC to
use the best available instruction set at runtime with no recompilation.

## 11.2 Hardware-Aware Execution Paths

Beyond SIMD, CPAC includes a broader hardware-acceleration architecture
spanning:

* **Intel QAT**,
* **Intel IAA**,
* **GPU compute**,
* **AMD Xilinx FPGA**,
* **ARM SVE2**.

Accelerator selection is integrated into the system interface and host-capable
execution model. This positions CPAC for modern datacenter hardware, where
compression acceleration increasingly ships as part of the platform itself.

## 11.3 Performance Engineering Infrastructure

CPAC also includes system-level performance tooling, including:

* profile-guided optimization,
* corpus management and automated benchmark support,
* multi-platform CI,
* determinism validation,
* regression and golden-vector testing,
* large-scale roundtrip verification.

Taken together, these elements make CPAC not just algorithmically adaptive, but
operationally performance-oriented.

---

# 12. Profiling, Calibration, and Continuous Improvement

One of CPAC’s strongest system-level differentiators is its closed-loop
improvement model.

Traditional compressors are largely static: they expose levels and tunables,
but the fundamental algorithm remains fixed. CPAC, by contrast, is explicitly
designed to improve over time through:

* new domain handlers,
* new transforms,
* improved routing logic,
* better calibration thresholds,
* dictionary training,
* hardware-path maturation,
* profiling-driven feedback into defaults and heuristics.

The built-in profiling engine can run multiple trial configurations against a
file or corpus, identify the gap between the default and the best-performing
configuration, and convert those findings into routing or development
decisions. Cross-file analysis further supports decisions about solid archive
mode, dictionary opportunities, and workload-specific specialization.

This matters because it changes the character of the system. CPAC’s benchmark
results are not only measurements of present performance; they are also signals
for systematic future improvement. In that sense, CPAC has a compounding
advantage that static compressors do not.

---

# 13. Evaluation

CPAC is evaluated on both standard academic corpora and real-world structured
datasets.

The standard benchmark set includes corpora such as:

* Calgary,
* Canterbury,
* Silesia,
* large text benchmarks including Wikipedia-derived workloads.

Beyond these, CPAC is intended for evaluation on practical infrastructure
datasets including:

* structured and semi-structured logs,
* API response archives,
* cloud configuration bundles,
* telemetry streams,
* binary typed records,
* archives with strong cross-file redundancy.

These workloads are especially important because they stress precisely the
forms of redundancy CPAC is designed to exploit: semantic repetition,
schema regularity, cross-record stability, and workload-dependent backend
selection.

A full benchmark chapter can then report ratio, compression throughput,
decompression throughput, archive effectiveness, and cryptographic overhead
under both structured and generic workloads.

---

# 14. Discussion

CPAC demonstrates that compression can benefit substantially from explicitly
modeling structure, constraints, and deployment context.

Several aspects distinguish it from conventional compressors:

1. it analyzes data before deciding how to compress it,
2. it can extract semantic redundancy rather than only byte redundancy,
3. it composes reversible transforms through a DAG rather than a fixed path,
4. it routes across multiple entropy backends,
5. it integrates streaming, archiving, and encryption into one architecture,
6. it is designed to improve continuously through profiling and calibration,
7. it is architected for hardware-aware execution.

This broader systems view is important. Modern operators rarely need “a codec”
in isolation. They need a practical data-reduction engine that can handle
heterogeneous files, multi-file bundles, streaming pipelines, bounded-memory
operation, interoperability, and security. CPAC is designed around that
reality.

---

# 15. Conclusion

Constraint-Projected Adaptive Compression introduces a structured,
adaptive, and deployment-aware approach to lossless data reduction.

By combining structural analysis, semantic normalization, reversible transform
composition, backend routing, streaming, archiving, and integrated
cryptographic protection, CPAC extends compression beyond the traditional
fixed-algorithm model. It treats redundancy not only as a property of bytes,
but also as a property of structure, constraints, and workload context.

CPAC is therefore best understood not as a standalone codec, but as an
adaptive compression architecture for modern data.

---

# References

1. Collet, Y. and Skibiński, P. “Smaller and faster data compression with Zstandard.” Meta Engineering Blog, 2016.
2. RFC 7932. “Brotli Compressed Data Format.” IETF.
3. LZ4 Project. “LZ4 - Extremely fast compression.”
4. Tukaani Project. “XZ Utils.”
5. NIST FIPS 203. “Module-Lattice-Based Key-Encapsulation Mechanism Standard.”
6. NIST FIPS 204. “Module-Lattice-Based Digital Signature Standard.”
7. W3C. “PNG Specification.”
8. Additional benchmark corpora references: Calgary Corpus, Canterbury Corpus, Silesia Corpus.
