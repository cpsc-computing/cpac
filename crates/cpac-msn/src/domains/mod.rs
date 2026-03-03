// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Domain handler implementations.

pub mod binary;
pub mod logs;
pub mod passthrough;
pub mod text;

pub use binary::MsgPackDomain;
pub use logs::{ApacheDomain, JsonLogDomain, SyslogDomain};
pub use passthrough::PassthroughDomain;
pub use text::{CsvDomain, JsonDomain, XmlDomain};
