// Copyright 2018-2024 the Deno authors. MIT license.

use thiserror::Error;

use crate::transforms::TransformError;

#[derive(Debug)]
pub struct LockfileError {
  pub file_path: String,
  pub reason: LockfileErrorReason,
}

impl std::error::Error for LockfileError {}

impl std::fmt::Display for LockfileError {
  fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
    match &self.reason {
      LockfileErrorReason::Empty => write!(f, "Unable to read lockfile. Lockfile was empty at '{}'.", self.file_path),
      LockfileErrorReason::ParseError(e) => write!(f, "Unable to parse contents of lockfile '{}': {:#}.", self.file_path, e),
      LockfileErrorReason::DeserializationError(e) => write!(f, "Unable to deserialize lockfile '{}': {:#}.", self.file_path, e),
      LockfileErrorReason::UnsupportedVersion { version } => write!(f, "Unsupported lockfile version '{}'. Try upgrading Deno or recreating the lockfile at '{}'.", version, self.file_path),
      LockfileErrorReason::TransformError(e) => write!(f, "Unable to upgrade old lockfile '{}': {:#}.", self.file_path, e),
    }
  }
}

#[derive(Debug)]
pub enum LockfileErrorReason {
  Empty,
  ParseError(serde_json::Error),
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
  #[error("Invalid package specifier '{0}'. Lockfile may be corrupt or you might be using an old version of Deno.")]
  InvalidPackageSpecifier(String),
  #[error("Invalid package specifier version '{version}' for '{specifier}'. Lockfile may be corrupt or you might be using an old version of Deno.")]
  InvalidPackageSpecifierVersion { specifier: String, version: String },
  #[error("Invalid jsr dependency '{dependency}' for '{package}'. Lockfile may be corrupt or you might be using an old version of Deno.")]
  InvalidJsrDependency { package: String, dependency: String },
  #[error("npm package '{0}' was not found and could not have its version resolved. Lockfile may be corrupt.")]
  MissingPackage(String),
}
