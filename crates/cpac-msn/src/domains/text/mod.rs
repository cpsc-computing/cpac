// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Text format domain handlers.

pub mod csv;
pub mod json;
pub mod xml;
pub mod yaml;

pub use csv::CsvDomain;
pub use json::JsonDomain;
pub use xml::XmlDomain;
pub use yaml::YamlDomain;
