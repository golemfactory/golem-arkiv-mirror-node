//! The `health` a node reports in its status.

/// `StorageFenced` and `ChainConflict` are terminal: the node stops
/// advancing until an operator intervenes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Health {
    Paused,
    Running,
    Following,
    Stalled,
    StorageFenced,
    ChainConflict,
}

impl Health {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Paused => "paused",
            Self::Running => "running",
            Self::Following => "following",
            Self::Stalled => "stalled",
            Self::StorageFenced => "storage-fenced",
            Self::ChainConflict => "chain-conflict",
        }
    }
}

impl serde::Serialize for Health {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}
