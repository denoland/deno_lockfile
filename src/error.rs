// Copyright 2018-2024 the Deno authors. All rights reserved. MIT license.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum LockfileError {
  #[error(transparent)]
  Io(#[from] std::io::Error),

  #[error("Unable to read lockfile. {0}")]
  ReadError(String),

  #[error("Unable to parse contents of lockfile. {0}: {1:#}")]
  ParseError(String, serde_json::Error),

  #[error("Unsupported lockfile version '{0}'. Try upgrading Deno or recreating the lockfile.")]
  UnsupportedVersion(String),
}
