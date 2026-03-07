// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Log format domain handlers.

pub mod apache;
pub mod bgl;
pub mod healthapp;
pub mod hpc;
pub mod http;
pub mod java;
pub mod json_log;
pub mod openstack;
pub mod proxifier;
pub mod syslog;
pub mod w3c;

pub use apache::ApacheDomain;
pub use bgl::BglLogDomain;
pub use healthapp::HealthAppDomain;
pub use hpc::HpcLogDomain;
pub use http::HttpDomain;
pub use java::JavaLogDomain;
pub use json_log::JsonLogDomain;
pub use openstack::OpenStackLogDomain;
pub use proxifier::ProxifierDomain;
pub use syslog::SyslogDomain;
pub use w3c::W3cLogDomain;
