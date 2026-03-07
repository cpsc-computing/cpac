// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Domain handler implementations.

pub mod binary;
pub mod logs;
pub mod passthrough;
pub mod text;

pub use binary::{AvroDomain, CborDomain, MsgPackDomain, ProtobufDomain};
pub use logs::{
    ApacheDomain, BglLogDomain, HealthAppDomain, HpcLogDomain, HttpDomain, JavaLogDomain,
    JsonLogDomain, OpenStackLogDomain, ProxifierDomain, SyslogDomain, W3cLogDomain,
};
pub use passthrough::PassthroughDomain;
pub use text::{CsvDomain, JsonDomain, XmlDomain, YamlDomain};
