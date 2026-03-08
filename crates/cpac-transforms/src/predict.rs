// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Prediction transform.
//!
//! Auto-selects the best predictor (delta-1, delta-2, or context-2) for
//! the input data and encodes residuals. The predictor ID and any model
//! parameters (e.g., context table) are stored in metadata for decode.
//!
//! Wire format (metadata):
//! `[predictor_id: 1][model_data...]`
//!
//! For Delta1/Delta2: no model data.
//! For Context2: 65536-byte context table.

use cpac_types::{CpacError, CpacResult, CpacType, TypeTag};

use crate::traits::{TransformContext, TransformNode};

/// Transform ID for predict (wire format).
pub const TRANSFORM_ID: u8 = 24;

/// Minimum data size to attempt prediction.
const MIN_SIZE: usize = 64;

// ---------------------------------------------------------------------------
// TransformNode implementation
// ---------------------------------------------------------------------------

/// Prediction transform node.
pub struct PredictTransform;

impl TransformNode for PredictTransform {
    fn name(&self) -> &str {
        "predict"
    }

    fn id(&self) -> u8 {
        TRANSFORM_ID
    }

    fn accepts(&self) -> &[TypeTag] {
        &[TypeTag::Serial]
    }

    fn produces(&self) -> TypeTag {
        TypeTag::Serial
    }

    fn estimate_gain(&self, input: &CpacType, ctx: &TransformContext) -> Option<f64> {
        match input {
            CpacType::Serial(data) if data.len() >= MIN_SIZE => {
                // Prediction helps on data with temporal/sequential correlation
                // and moderate entropy. High entropy = random, prediction useless.
                if ctx.entropy_estimate > 1.0 && ctx.entropy_estimate < 6.5 {
                    let (_, residual_entropy) = cpac_predict::select_best(data);
                    let improvement = ctx.entropy_estimate - residual_entropy;
                    if improvement > 0.5 {
                        Some(improvement * 2.0)
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn encode(&self, input: CpacType, _ctx: &TransformContext) -> CpacResult<(CpacType, Vec<u8>)> {
        match input {
            CpacType::Serial(data) => {
                if data.len() < MIN_SIZE {
                    return Ok((CpacType::Serial(data), Vec::new()));
                }

                let (predictor_id, _entropy) = cpac_predict::select_best(&data);

                let (residuals, meta) = match predictor_id {
                    cpac_predict::PredictorId::Delta1 => {
                        let residuals = cpac_predict::encode_delta1(&data);
                        (residuals, vec![predictor_id as u8])
                    }
                    cpac_predict::PredictorId::Delta2 => {
                        let residuals = cpac_predict::encode_delta2(&data);
                        (residuals, vec![predictor_id as u8])
                    }
                    cpac_predict::PredictorId::Context2 => {
                        let (residuals, table) = cpac_predict::encode_context2(&data);
                        // Context2 metadata is large (64KB table) — only use if
                        // the residual entropy improvement justifies it.
                        if table.len() >= data.len() / 2 {
                            // Table too large relative to data — fall back to delta1
                            let residuals = cpac_predict::encode_delta1(&data);
                            (residuals, vec![cpac_predict::PredictorId::Delta1 as u8])
                        } else {
                            let mut meta = Vec::with_capacity(1 + table.len());
                            meta.push(predictor_id as u8);
                            meta.extend_from_slice(&table);
                            (residuals, meta)
                        }
                    }
                };

                Ok((CpacType::Serial(residuals), meta))
            }
            _ => Err(CpacError::Transform(
                "predict: expected Serial input".into(),
            )),
        }
    }

    fn decode(&self, input: CpacType, metadata: &[u8]) -> CpacResult<CpacType> {
        match input {
            CpacType::Serial(residuals) => {
                if metadata.is_empty() {
                    return Ok(CpacType::Serial(residuals));
                }

                let predictor_id = cpac_predict::PredictorId::from_u8(metadata[0]).ok_or_else(
                    || CpacError::Transform(format!("predict: unknown predictor id {}", metadata[0])),
                )?;

                let decoded = match predictor_id {
                    cpac_predict::PredictorId::Delta1 => {
                        cpac_predict::decode_delta1(&residuals)
                    }
                    cpac_predict::PredictorId::Delta2 => {
                        cpac_predict::decode_delta2(&residuals)
                    }
                    cpac_predict::PredictorId::Context2 => {
                        if metadata.len() < 1 + 65536 {
                            return Err(CpacError::Transform(
                                "predict: context2 metadata too short".into(),
                            ));
                        }
                        let table = &metadata[1..1 + 65536];
                        cpac_predict::decode_context2(&residuals, table)
                    }
                };

                Ok(CpacType::Serial(decoded))
            }
            _ => Err(CpacError::Transform(
                "predict: expected Serial input".into(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_counter_data() {
        let t = PredictTransform;
        let data: Vec<u8> = (0..500).map(|i| (i % 256) as u8).collect();
        let ctx = TransformContext {
            entropy_estimate: 4.0,
            ascii_ratio: 0.3,
            data_size: data.len(),
        };
        let input = CpacType::Serial(data.clone());
        let (encoded, meta) = t.encode(input, &ctx).unwrap();
        assert!(!meta.is_empty());
        let decoded = t.decode(encoded, &meta).unwrap();
        match decoded {
            CpacType::Serial(d) => assert_eq!(d, data),
            _ => panic!("expected Serial"),
        }
    }

    #[test]
    fn roundtrip_text() {
        let t = PredictTransform;
        let data = b"AAABBBCCCDDDEEEFFFGGGHHHIIIJJJKKK".repeat(5);
        let ctx = TransformContext {
            entropy_estimate: 3.0,
            ascii_ratio: 1.0,
            data_size: data.len(),
        };
        let input = CpacType::Serial(data.clone());
        let (encoded, meta) = t.encode(input, &ctx).unwrap();
        assert!(!meta.is_empty());
        let decoded = t.decode(encoded, &meta).unwrap();
        match decoded {
            CpacType::Serial(d) => assert_eq!(d, data),
            _ => panic!("expected Serial"),
        }
    }

    #[test]
    fn empty_passthrough() {
        let t = PredictTransform;
        let ctx = TransformContext {
            entropy_estimate: 0.0,
            ascii_ratio: 0.0,
            data_size: 0,
        };
        let input = CpacType::Serial(Vec::new());
        let (encoded, meta) = t.encode(input, &ctx).unwrap();
        assert!(meta.is_empty());
        let decoded = t.decode(encoded, &meta).unwrap();
        match decoded {
            CpacType::Serial(d) => assert!(d.is_empty()),
            _ => panic!("expected Serial"),
        }
    }
}
