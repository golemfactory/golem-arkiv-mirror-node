//! The error of the genesis tooling (bundle authoring, prefill import,
//! prebuilt state, adoption): the operator-facing message.

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GenesisToolError(String);

impl fmt::Display for GenesisToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for GenesisToolError {}

impl From<String> for GenesisToolError {
    fn from(message: String) -> Self {
        Self(message)
    }
}

impl From<&str> for GenesisToolError {
    fn from(message: &str) -> Self {
        Self(message.to_owned())
    }
}
