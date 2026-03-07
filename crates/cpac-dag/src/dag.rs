// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Transform DAG — ordered pipeline of transforms that executes forward/backward.

use std::sync::Arc;

use cpac_transforms::{TransformContext, TransformNode};
use cpac_types::{CpacError, CpacResult, CpacType};

use crate::registry::TransformRegistry;
use cpac_cas::{analyze_column, recommend_transforms};

/// Metadata chain produced during forward execution: (`transform_id`, `metadata_bytes`) per step.
pub type MetaChain = Vec<(u8, Vec<u8>)>;

/// A compiled DAG: an ordered list of transform nodes.
///
/// Forward execution applies transforms in order.
/// Backward execution (decode) reverses the order.
#[derive(Clone)]
pub struct TransformDAG {
    /// Ordered transform steps.
    steps: Vec<Arc<dyn TransformNode>>,
}

impl TransformDAG {
    /// Create an empty (passthrough) DAG.
    #[must_use]
    pub fn passthrough() -> Self {
        Self { steps: Vec::new() }
    }

    /// Create a DAG from an explicit list of transforms.
    #[must_use]
    pub fn from_steps(steps: Vec<Arc<dyn TransformNode>>) -> Self {
        Self { steps }
    }

    /// Compile a DAG from a list of transform names using the registry.
    pub fn compile(registry: &TransformRegistry, transform_names: &[&str]) -> CpacResult<Self> {
        let mut steps = Vec::with_capacity(transform_names.len());
        for &name in transform_names {
            let node = registry
                .get_by_name(name)
                .ok_or_else(|| CpacError::Transform(format!("unknown transform: {name}")))?;
            steps.push(Arc::clone(node));
        }
        Ok(Self { steps })
    }

    /// Compile a DAG from transform IDs (used during decompression).
    pub fn compile_from_ids(registry: &TransformRegistry, ids: &[u8]) -> CpacResult<Self> {
        let mut steps = Vec::with_capacity(ids.len());
        for &id in ids {
            let node = registry
                .get_by_id(id)
                .ok_or_else(|| CpacError::Transform(format!("unknown transform id: {id}")))?;
            steps.push(Arc::clone(node));
        }
        Ok(Self { steps })
    }

    /// Execute the DAG in the forward (compression) direction.
    ///
    /// Returns the final output and a list of (`transform_id`, metadata) pairs
    /// for the decoder.
    pub fn execute_forward(
        &self,
        input: CpacType,
        ctx: &TransformContext,
    ) -> CpacResult<(CpacType, MetaChain)> {
        let mut current = input;
        let mut meta_chain = Vec::with_capacity(self.steps.len());
        for step in &self.steps {
            let (output, meta) = step.encode(current, ctx)?;
            meta_chain.push((step.id(), meta));
            current = output;
        }
        Ok((current, meta_chain))
    }

    /// Execute the DAG in the backward (decompression) direction.
    ///
    /// Transforms are applied in reverse order with their corresponding metadata.
    pub fn execute_backward(
        &self,
        input: CpacType,
        meta_chain: &[(u8, Vec<u8>)],
    ) -> CpacResult<CpacType> {
        if meta_chain.len() != self.steps.len() {
            return Err(CpacError::Transform(format!(
                "DAG has {} steps but got {} metadata entries",
                self.steps.len(),
                meta_chain.len()
            )));
        }
        let mut current = input;
        for (step, (_id, meta)) in self.steps.iter().zip(meta_chain.iter()).rev() {
            current = step.decode(current, meta)?;
        }
        Ok(current)
    }

    /// Auto-select applicable transforms based on input and context.
    ///
    /// Tries each transform in the registry, keeping those with positive gain.
    /// Returns a new DAG with the selected transforms.
    #[must_use]
    pub fn auto_select(
        registry: &TransformRegistry,
        input: &CpacType,
        ctx: &TransformContext,
    ) -> Self {
        let mut candidates: Vec<(f64, Arc<dyn TransformNode>)> = Vec::new();
        for name in registry.names() {
            if let Some(node) = registry.get_by_name(name) {
                if let Some(gain) = node.estimate_gain(input, ctx) {
                    if gain > 0.0 {
                        candidates.push((gain, Arc::clone(node)));
                    }
                }
            }
        }
        // Sort by gain descending
        candidates.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        let steps: Vec<Arc<dyn TransformNode>> =
            candidates.into_iter().map(|(_, node)| node).collect();
        Self { steps }
    }

    /// Compile a DAG guided by CAS constraint analysis.
    ///
    /// Analyzes `data` for constraints, maps them to transform recommendations,
    /// and builds a chain from the transforms found in `registry`.
    /// Unknown transform names (e.g. `const_elim`) are silently skipped.
    pub fn compile_with_cas(
        registry: &TransformRegistry,
        data: &CpacType,
        column_name: &str,
    ) -> CpacResult<Self> {
        let constraints = analyze_column(column_name, data);
        let recs = recommend_transforms(&constraints);
        let mut steps = Vec::new();
        for rec in &recs {
            if let Some(node) = registry.get_by_name(&rec.name) {
                steps.push(Arc::clone(node));
            }
            // Skip unknown names (e.g. const_elim sentinel)
        }
        Ok(Self { steps })
    }

    /// Number of steps in the DAG.
    #[must_use]
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    /// Whether the DAG is empty (passthrough).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Get the ordered list of transform IDs.
    #[must_use]
    pub fn transform_ids(&self) -> Vec<u8> {
        self.steps.iter().map(|s| s.id()).collect()
    }

    /// Get the ordered list of transform names.
    #[must_use]
    pub fn transform_names(&self) -> Vec<&str> {
        self.steps.iter().map(|s| s.name()).collect()
    }
}

/// Serialize a DAG descriptor to bytes for the frame.
///
/// Format: `[count:1][ids...][per-step: meta_len:2 LE + meta_bytes]`.
#[must_use]
pub fn serialize_dag_descriptor(meta_chain: &[(u8, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(meta_chain.len() as u8);
    for (id, _) in meta_chain {
        out.push(*id);
    }
    for (_, meta) in meta_chain {
        out.extend_from_slice(&(meta.len() as u16).to_le_bytes());
        out.extend_from_slice(meta);
    }
    out
}

/// Deserialize a DAG descriptor from bytes.
///
/// Returns `(transform_ids, metadata_per_step, bytes_consumed)`.
pub fn deserialize_dag_descriptor(data: &[u8]) -> CpacResult<(Vec<u8>, Vec<Vec<u8>>, usize)> {
    if data.is_empty() {
        return Err(CpacError::Transform("empty DAG descriptor".into()));
    }
    let count = data[0] as usize;
    let mut offset = 1;
    if offset + count > data.len() {
        return Err(CpacError::Transform("truncated DAG descriptor".into()));
    }
    let ids: Vec<u8> = data[offset..offset + count].to_vec();
    offset += count;
    let mut metas = Vec::with_capacity(count);
    for _ in 0..count {
        if offset + 2 > data.len() {
            return Err(CpacError::Transform(
                "truncated DAG descriptor metadata".into(),
            ));
        }
        let meta_len = u16::from_le_bytes([data[offset], data[offset + 1]]) as usize;
        offset += 2;
        if offset + meta_len > data.len() {
            return Err(CpacError::Transform(
                "truncated DAG descriptor metadata payload".into(),
            ));
        }
        metas.push(data[offset..offset + meta_len].to_vec());
        offset += meta_len;
    }
    Ok((ids, metas, offset))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passthrough_roundtrip() {
        let dag = TransformDAG::passthrough();
        assert!(dag.is_empty());
        let ctx = TransformContext {
            entropy_estimate: 4.0,
            ascii_ratio: 0.5,
            data_size: 100,
        };
        let input = CpacType::Serial(vec![1, 2, 3]);
        let (output, meta) = dag.execute_forward(input.clone(), &ctx).unwrap();
        assert!(meta.is_empty());
        let restored = dag.execute_backward(output, &meta).unwrap();
        match (input, restored) {
            (CpacType::Serial(a), CpacType::Serial(b)) => assert_eq!(a, b),
            _ => panic!("type mismatch"),
        }
    }

    #[test]
    fn compile_from_names() {
        let reg = TransformRegistry::with_builtins();
        let dag = TransformDAG::compile(&reg, &["delta", "zigzag"]).unwrap();
        assert_eq!(dag.len(), 2);
        assert_eq!(dag.transform_names(), vec!["delta", "zigzag"]);
    }

    #[test]
    fn compile_unknown_fails() {
        let reg = TransformRegistry::with_builtins();
        assert!(TransformDAG::compile(&reg, &["nonexistent"]).is_err());
    }

    #[test]
    fn cas_guided_monotonic_int() {
        let reg = TransformRegistry::with_builtins();
        let data = CpacType::IntColumn {
            values: (0..200).collect(),
            original_width: 8,
        };
        let dag = TransformDAG::compile_with_cas(&reg, &data, "id").unwrap();
        let names = dag.transform_names();
        // Monotonic stride-1 int column should get delta + zigzag + range_pack
        assert!(names.contains(&"delta"), "expected delta in {:?}", names);
        assert!(names.contains(&"zigzag"), "expected zigzag in {:?}", names);
        assert!(names.contains(&"range_pack"), "expected range_pack in {:?}", names);
    }

    #[test]
    fn cas_guided_string_enum() {
        let reg = TransformRegistry::with_builtins();
        let data = CpacType::StringColumn {
            values: vec!["A", "B", "C", "A", "B", "A", "C", "B"]
                .into_iter()
                .map(String::from)
                .collect(),
            total_bytes: 8,
        };
        let dag = TransformDAG::compile_with_cas(&reg, &data, "status").unwrap();
        let names = dag.transform_names();
        assert!(names.contains(&"vocab"), "expected vocab in {:?}", names);
    }

    #[test]
    fn cas_guided_empty_serial() {
        let reg = TransformRegistry::with_builtins();
        let data = CpacType::Serial(vec![1, 2, 3]);
        let dag = TransformDAG::compile_with_cas(&reg, &data, "raw").unwrap();
        // Serial data yields no CAS constraints → empty DAG
        assert!(dag.is_empty());
    }

    #[test]
    fn descriptor_roundtrip() {
        let meta_chain = vec![(1u8, vec![0x10, 0x20]), (4u8, vec![])];
        let encoded = serialize_dag_descriptor(&meta_chain);
        let (ids, metas, consumed) = deserialize_dag_descriptor(&encoded).unwrap();
        assert_eq!(ids, vec![1, 4]);
        assert_eq!(metas, vec![vec![0x10, 0x20], vec![]]);
        assert_eq!(consumed, encoded.len());
    }
}
