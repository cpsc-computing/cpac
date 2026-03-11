// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Cloud streaming output adapters.
//!
//! Provides a trait-based abstraction for streaming compressed output to cloud
//! object stores (S3, GCS, Azure Blob) via multipart upload.  Each backend is
//! gated behind an optional feature flag; when no cloud feature is enabled the
//! module still exposes [`CloudTarget`] and [`CloudUrl`] so callers can parse
//! URLs and get a clear error.
//!
//! # URL Scheme
//! | Prefix    | Backend          | Feature flag    |
//! |-----------|------------------|-----------------|
//! | `s3://`   | Amazon S3        | `cloud-s3`      |
//! | `gs://`   | Google Cloud     | `cloud-gcs`     |
//! | `az://`   | Azure Blob       | `cloud-azure`   |
//! | `file://`  | Local passthrough| *(always)*      |

use cpac_types::{CpacError, CpacResult};
use std::io::{self, Write};

// ---------------------------------------------------------------------------
// Parsed cloud URL
// ---------------------------------------------------------------------------

/// Parsed cloud-storage URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudUrl {
    pub scheme: CloudScheme,
    pub bucket: String,
    pub key: String,
}

/// Recognised URL schemes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudScheme {
    S3,
    Gcs,
    Azure,
    File,
}

impl CloudUrl {
    /// Parse a URL string like `s3://bucket/path/to/key`.
    ///
    /// # Errors
    /// Returns `CpacError::Other` when the scheme is unrecognised or the URL
    /// cannot be split into bucket + key.
    pub fn parse(url: &str) -> CpacResult<Self> {
        let (scheme, rest) = if let Some(r) = url.strip_prefix("s3://") {
            (CloudScheme::S3, r)
        } else if let Some(r) = url.strip_prefix("gs://") {
            (CloudScheme::Gcs, r)
        } else if let Some(r) = url.strip_prefix("az://") {
            (CloudScheme::Azure, r)
        } else if let Some(r) = url.strip_prefix("file://") {
            return Ok(Self {
                scheme: CloudScheme::File,
                bucket: String::new(),
                key: r.to_string(),
            });
        } else {
            return Err(CpacError::Other(format!(
                "unsupported cloud URL scheme: {url}"
            )));
        };

        let (bucket, key) = rest
            .split_once('/')
            .ok_or_else(|| CpacError::Other(format!("cloud URL missing key: {url}")))?;
        if bucket.is_empty() || key.is_empty() {
            return Err(CpacError::Other(format!(
                "cloud URL has empty bucket or key: {url}"
            )));
        }
        Ok(Self {
            scheme,
            bucket: bucket.to_string(),
            key: key.to_string(),
        })
    }

    /// Human-readable scheme label.
    #[must_use]
    pub fn scheme_label(&self) -> &'static str {
        match self.scheme {
            CloudScheme::S3 => "Amazon S3",
            CloudScheme::Gcs => "Google Cloud Storage",
            CloudScheme::Azure => "Azure Blob Storage",
            CloudScheme::File => "local file",
        }
    }
}

// ---------------------------------------------------------------------------
// Cloud target trait
// ---------------------------------------------------------------------------

/// Trait for streaming cloud upload targets.
///
/// Implementors should perform multipart (chunked) upload to their respective
/// object store, buffering at most `part_size` bytes before flushing.
pub trait CloudTarget: Write + Send {
    /// Initialise the multipart upload session.
    fn begin(&mut self) -> CpacResult<()>;
    /// Flush the current part and complete the upload.
    fn complete(&mut self) -> CpacResult<()>;
    /// Abort the upload (best-effort cleanup).
    fn abort(&mut self) -> CpacResult<()>;
    /// Minimum part size supported by the backend (bytes).
    fn min_part_size(&self) -> usize;
}

// ---------------------------------------------------------------------------
// Local file passthrough (always available)
// ---------------------------------------------------------------------------

/// Local file output target – trivially implements [`CloudTarget`].
pub struct FileTarget {
    path: String,
    writer: Option<std::fs::File>,
}

impl FileTarget {
    pub fn new(path: &str) -> Self {
        Self {
            path: path.to_string(),
            writer: None,
        }
    }
}

impl CloudTarget for FileTarget {
    fn begin(&mut self) -> CpacResult<()> {
        let f = std::fs::File::create(&self.path)
            .map_err(|e| CpacError::IoError(format!("failed to create {}: {e}", self.path)))?;
        self.writer = Some(f);
        Ok(())
    }

    fn complete(&mut self) -> CpacResult<()> {
        if let Some(ref mut w) = self.writer {
            w.flush().map_err(|e| CpacError::IoError(e.to_string()))?;
        }
        self.writer = None;
        Ok(())
    }

    fn abort(&mut self) -> CpacResult<()> {
        self.writer = None;
        let _ = std::fs::remove_file(&self.path);
        Ok(())
    }

    fn min_part_size(&self) -> usize {
        0 // no minimum
    }
}

impl Write for FileTarget {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.writer
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "not started"))
            .and_then(|w| w.write(buf))
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Some(ref mut w) = self.writer {
            w.flush()
        } else {
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// S3 adapter (feature: cloud-s3)
// ---------------------------------------------------------------------------

#[cfg(feature = "cloud-s3")]
pub mod s3 {
    //! Amazon S3 multipart upload adapter.
    //!
    //! Uses the `aws-sdk-s3` crate via a Tokio runtime. Requires
    //! `AWS_REGION`, `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY` (or
    //! instance-role credentials).
    use super::*;

    /// Default S3 multipart part size: 8 MB (minimum is 5 MB).
    pub const DEFAULT_PART_SIZE: usize = 8 * 1024 * 1024;

    /// S3 streaming upload target.
    pub struct S3Target {
        bucket: String,
        key: String,
        part_size: usize,
        buffer: Vec<u8>,
        part_number: i32,
        upload_id: Option<String>,
        parts: Vec<(i32, String)>, // (part_number, etag)
        runtime: tokio::runtime::Runtime,
        client: aws_sdk_s3::Client,
    }

    impl S3Target {
        /// Create an S3 target from a parsed [`CloudUrl`].
        ///
        /// Initialises the AWS SDK client using ambient credentials.
        pub fn from_url(url: &CloudUrl) -> CpacResult<Self> {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| CpacError::Other(format!("tokio init: {e}")))?;
            let config = rt.block_on(aws_config::load_defaults(
                aws_config::BehaviorVersion::latest(),
            ));
            let client = aws_sdk_s3::Client::new(&config);
            Ok(Self {
                bucket: url.bucket.clone(),
                key: url.key.clone(),
                part_size: DEFAULT_PART_SIZE,
                buffer: Vec::with_capacity(DEFAULT_PART_SIZE),
                part_number: 0,
                upload_id: None,
                parts: Vec::new(),
                runtime: rt,
                client,
            })
        }

        fn flush_part(&mut self) -> CpacResult<()> {
            if self.buffer.is_empty() {
                return Ok(());
            }
            let uid = self
                .upload_id
                .as_ref()
                .ok_or_else(|| CpacError::Other("upload not started".into()))?
                .clone();
            self.part_number += 1;
            let pn = self.part_number;
            let body = aws_sdk_s3::primitives::ByteStream::from(self.buffer.clone());
            let resp = self
                .runtime
                .block_on(
                    self.client
                        .upload_part()
                        .bucket(&self.bucket)
                        .key(&self.key)
                        .upload_id(&uid)
                        .part_number(pn)
                        .body(body)
                        .send(),
                )
                .map_err(|e| CpacError::Other(format!("S3 upload_part: {e}")))?;
            let etag = resp.e_tag().unwrap_or_default().to_string();
            self.parts.push((pn, etag));
            self.buffer.clear();
            Ok(())
        }
    }

    impl CloudTarget for S3Target {
        fn begin(&mut self) -> CpacResult<()> {
            let resp = self
                .runtime
                .block_on(
                    self.client
                        .create_multipart_upload()
                        .bucket(&self.bucket)
                        .key(&self.key)
                        .send(),
                )
                .map_err(|e| CpacError::Other(format!("S3 create_multipart: {e}")))?;
            self.upload_id = resp.upload_id().map(str::to_string);
            Ok(())
        }

        fn complete(&mut self) -> CpacResult<()> {
            self.flush_part()?;
            let uid = self
                .upload_id
                .take()
                .ok_or_else(|| CpacError::Other("no upload id".into()))?;
            use aws_sdk_s3::types::{CompletedMultipartUpload, CompletedPart};
            let parts: Vec<CompletedPart> = self
                .parts
                .iter()
                .map(|(pn, etag)| {
                    CompletedPart::builder()
                        .part_number(*pn)
                        .e_tag(etag)
                        .build()
                })
                .collect();
            let upload = CompletedMultipartUpload::builder()
                .set_parts(Some(parts))
                .build();
            self.runtime
                .block_on(
                    self.client
                        .complete_multipart_upload()
                        .bucket(&self.bucket)
                        .key(&self.key)
                        .upload_id(&uid)
                        .multipart_upload(upload)
                        .send(),
                )
                .map_err(|e| CpacError::Other(format!("S3 complete: {e}")))?;
            Ok(())
        }

        fn abort(&mut self) -> CpacResult<()> {
            if let Some(uid) = self.upload_id.take() {
                let _ = self.runtime.block_on(
                    self.client
                        .abort_multipart_upload()
                        .bucket(&self.bucket)
                        .key(&self.key)
                        .upload_id(&uid)
                        .send(),
                );
            }
            Ok(())
        }

        fn min_part_size(&self) -> usize {
            5 * 1024 * 1024 // S3 minimum
        }
    }

    impl Write for S3Target {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.buffer.extend_from_slice(buf);
            if self.buffer.len() >= self.part_size {
                self.flush_part()
                    .map_err(|e| io::Error::other(e.to_string()))?;
            }
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            // Don't flush partial parts — S3 requires >= 5MB except for last part
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// GCS adapter (feature: cloud-gcs)
// ---------------------------------------------------------------------------

#[cfg(feature = "cloud-gcs")]
pub mod gcs {
    //! Google Cloud Storage resumable upload adapter.
    //!
    //! Uses the `google-cloud-storage` crate. Requires
    //! `GOOGLE_APPLICATION_CREDENTIALS` or GCE metadata credentials.
    use super::*;

    /// Default GCS part size: 8 MB (minimum recommended is 5 MB).
    pub const DEFAULT_PART_SIZE: usize = 8 * 1024 * 1024;

    /// GCS streaming upload target (stub — depends on `google-cloud-storage`).
    #[allow(dead_code)]
    pub struct GcsTarget {
        bucket: String,
        key: String,
        part_size: usize,
        buffer: Vec<u8>,
    }

    impl GcsTarget {
        pub fn from_url(url: &CloudUrl) -> CpacResult<Self> {
            Ok(Self {
                bucket: url.bucket.clone(),
                key: url.key.clone(),
                part_size: DEFAULT_PART_SIZE,
                buffer: Vec::with_capacity(DEFAULT_PART_SIZE),
            })
        }
    }

    impl CloudTarget for GcsTarget {
        fn begin(&mut self) -> CpacResult<()> {
            // TODO: Initiate GCS resumable upload session
            Err(CpacError::Other(
                "GCS upload not yet wired — enable cloud-gcs feature with google-cloud-storage dep"
                    .into(),
            ))
        }

        fn complete(&mut self) -> CpacResult<()> {
            Err(CpacError::Other("GCS upload not initialised".into()))
        }

        fn abort(&mut self) -> CpacResult<()> {
            self.buffer.clear();
            Ok(())
        }

        fn min_part_size(&self) -> usize {
            5 * 1024 * 1024
        }
    }

    impl Write for GcsTarget {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.buffer.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// Azure Blob adapter (feature: cloud-azure)
// ---------------------------------------------------------------------------

#[cfg(feature = "cloud-azure")]
pub mod azure {
    //! Azure Blob Storage block-upload adapter.
    //!
    //! Uses the `azure_storage_blobs` crate. Requires
    //! `AZURE_STORAGE_ACCOUNT` + `AZURE_STORAGE_KEY` (or managed-identity).
    use super::*;

    /// Default Azure block size: 4 MB (max 100 MB for put_block).
    pub const DEFAULT_BLOCK_SIZE: usize = 4 * 1024 * 1024;

    /// Azure Blob streaming upload target (stub — depends on `azure_storage_blobs`).
    #[allow(dead_code)]
    pub struct AzureTarget {
        container: String,
        blob: String,
        block_size: usize,
        buffer: Vec<u8>,
    }

    impl AzureTarget {
        /// `bucket` maps to Azure container name, `key` to blob name.
        pub fn from_url(url: &CloudUrl) -> CpacResult<Self> {
            Ok(Self {
                container: url.bucket.clone(),
                blob: url.key.clone(),
                block_size: DEFAULT_BLOCK_SIZE,
                buffer: Vec::with_capacity(DEFAULT_BLOCK_SIZE),
            })
        }
    }

    impl CloudTarget for AzureTarget {
        fn begin(&mut self) -> CpacResult<()> {
            // TODO: Azure SDK initialisation
            Err(CpacError::Other(
                "Azure upload not yet wired — enable cloud-azure feature with azure_storage_blobs dep"
                    .into(),
            ))
        }

        fn complete(&mut self) -> CpacResult<()> {
            Err(CpacError::Other("Azure upload not initialised".into()))
        }

        fn abort(&mut self) -> CpacResult<()> {
            self.buffer.clear();
            Ok(())
        }

        fn min_part_size(&self) -> usize {
            0 // Azure has no minimum block size
        }
    }

    impl Write for AzureTarget {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.buffer.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// Factory: create target from URL
// ---------------------------------------------------------------------------

/// Create a [`CloudTarget`] from a URL string.
///
/// Dispatches on scheme (`s3://`, `gs://`, `az://`, `file://`).
///
/// # Errors
/// Returns `CpacError::Other` when the required feature is not compiled in.
pub fn open_target(url: &str) -> CpacResult<Box<dyn CloudTarget>> {
    let parsed = CloudUrl::parse(url)?;
    match parsed.scheme {
        CloudScheme::File => Ok(Box::new(FileTarget::new(&parsed.key))),

        #[cfg(feature = "cloud-s3")]
        CloudScheme::S3 => Ok(Box::new(s3::S3Target::from_url(&parsed)?)),
        #[cfg(not(feature = "cloud-s3"))]
        CloudScheme::S3 => Err(CpacError::Other(
            "S3 support requires the `cloud-s3` feature flag".into(),
        )),

        #[cfg(feature = "cloud-gcs")]
        CloudScheme::Gcs => Ok(Box::new(gcs::GcsTarget::from_url(&parsed)?)),
        #[cfg(not(feature = "cloud-gcs"))]
        CloudScheme::Gcs => Err(CpacError::Other(
            "GCS support requires the `cloud-gcs` feature flag".into(),
        )),

        #[cfg(feature = "cloud-azure")]
        CloudScheme::Azure => Ok(Box::new(azure::AzureTarget::from_url(&parsed)?)),
        #[cfg(not(feature = "cloud-azure"))]
        CloudScheme::Azure => Err(CpacError::Other(
            "Azure support requires the `cloud-azure` feature flag".into(),
        )),
    }
}

// ---------------------------------------------------------------------------
// Streaming compress → cloud convenience
// ---------------------------------------------------------------------------

/// Compress `data` and stream the output to a cloud target.
///
/// 1. Opens the target via [`open_target`].
/// 2. Compresses in blocks via [`super::compress_streaming`].
/// 3. Writes the frame to the target in `part_size` chunks.
///
/// # Errors
/// Propagates cloud I/O and compression errors.
pub fn compress_to_cloud(
    data: &[u8],
    config: &cpac_types::CompressConfig,
    block_size: usize,
    parallel: bool,
    target_url: &str,
) -> CpacResult<usize> {
    let frame = super::compress_streaming(data, config, block_size, parallel)?;
    let mut target = open_target(target_url)?;
    target.begin()?;

    // Write in 4 MB chunks to respect part-size minimums
    let chunk_sz = target.min_part_size().max(4 * 1024 * 1024);
    for chunk in frame.data.chunks(chunk_sz) {
        target
            .write_all(chunk)
            .map_err(|e| CpacError::IoError(e.to_string()))?;
    }

    target.complete()?;
    Ok(frame.compressed_size)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_s3_url() {
        let url = CloudUrl::parse("s3://my-bucket/path/to/file.cpac").unwrap();
        assert_eq!(url.scheme, CloudScheme::S3);
        assert_eq!(url.bucket, "my-bucket");
        assert_eq!(url.key, "path/to/file.cpac");
    }

    #[test]
    fn parse_gs_url() {
        let url = CloudUrl::parse("gs://bucket/key.cpac").unwrap();
        assert_eq!(url.scheme, CloudScheme::Gcs);
        assert_eq!(url.bucket, "bucket");
    }

    #[test]
    fn parse_az_url() {
        let url = CloudUrl::parse("az://container/blob.cpac").unwrap();
        assert_eq!(url.scheme, CloudScheme::Azure);
        assert_eq!(url.bucket, "container");
        assert_eq!(url.key, "blob.cpac");
    }

    #[test]
    fn parse_file_url() {
        let url = CloudUrl::parse("file:///tmp/out.cpac").unwrap();
        assert_eq!(url.scheme, CloudScheme::File);
        assert_eq!(url.key, "/tmp/out.cpac");
    }

    #[test]
    fn parse_bad_scheme() {
        assert!(CloudUrl::parse("ftp://foo/bar").is_err());
    }

    #[test]
    fn parse_missing_key() {
        assert!(CloudUrl::parse("s3://bucket-only").is_err());
    }

    #[test]
    fn file_target_roundtrip() {
        let dir = std::env::temp_dir();
        let path = dir.join("cpac_cloud_test.bin");
        let url = format!("file://{}", path.display());
        let mut t = open_target(&url).unwrap();
        t.begin().unwrap();
        t.write_all(b"hello cloud").unwrap();
        t.complete().unwrap();
        let data = std::fs::read(&path).unwrap();
        assert_eq!(data, b"hello cloud");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn s3_not_compiled_error() {
        // Without cloud-s3 feature, should get a descriptive error
        #[cfg(not(feature = "cloud-s3"))]
        {
            let r = open_target("s3://bucket/key");
            assert!(r.is_err());
            let msg = r.err().unwrap().to_string();
            assert!(msg.contains("cloud-s3"));
        }
    }
}
