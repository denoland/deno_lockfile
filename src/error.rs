// Copyright 2018-2024 the Deno authors. All rights reserved. MIT license.

use thiserror::Error;

use crate::transforms::TransformError;

#[derive(Debug)]
pub struct LockfileError {
  pub filename: String,
  pub reason: LockfileErrorReason,
}

impl std::error::Error for LockfileError {}

impl std::fmt::Display for LockfileError {
  fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
    match &self.reason {
      LockfileErrorReason::Empty => write!(f, "Unable to read lockfile. Lockfile was empty at '{}'.", self.filename),
      LockfileErrorReason::ParseError(e) => write!(f, "Unable to parse contents of lockfile '{}': {:#}.", self.filename, e),
      LockfileErrorReason::DeserializationError(e) => write!(f, "Unable to deserialize lockfile '{}': {:#}.", self.filename, e),
      LockfileErrorReason::UnsupportedVersion { version } => write!(f, "Unsupported lockfile version '{}'. Try upgrading Deno or recreating the lockfile at '{}'.", version, self.filename),
      LockfileErrorReason::TransformError(e) => write!(f, "Unable to upgrade old lockfile '{}': {:#}.", self.filename, e),
    }
  }
}

#[derive(Debug)]
pub enum LockfileErrorReason {
  Empty,
  ParseError(monch::ParseErrorFailureError),
  DeserializationError(DeserializationError),
  UnsupportedVersion { version: String },
  TransformError(TransformError),
}

impl From<TransformError> for LockfileErrorReason {
  fn from(e: TransformError) -> Self {
    LockfileErrorReason::TransformError(e)
  }
}

#[derive(Debug, Error)]
pub enum DeserializationError {
  #[error("Invalid {0} section: {1:#}")]
  FailedDeserializing(&'static str, serde_json::Error),
  #[error("Invalid npm package '{0}'. Lockfile may be corrupt or you might be using an old version of Deno.")]
  InvalidNpmPackageId(String),
  #[error("Invalid npm package dependency '{0}'. Lockfile may be corrupt or you might be using an old version of Deno.")]
  InvalidNpmPackageDependency(String),
  #[error("npm package '{0}' was not found and could not have its version resolved. Lockfile may be corrupt.")]
  MissingPackage(String),
}
