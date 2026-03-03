// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Binary format domain handlers.

pub mod avro;
pub mod cbor;
pub mod msgpack;
pub mod protobuf;

pub use avro::AvroDomain;
pub use cbor::CborDomain;
pub use msgpack::MsgPackDomain;
pub use protobuf::ProtobufDomain;
