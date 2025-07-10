#[derive(Debug)]
pub enum LazyError {
    Io(std::io::Error),
    Decode(bincode::error::DecodeError),
    Encode(bincode::error::EncodeError),
    Join(tokio::task::JoinError),
}

impl std::fmt::Display for LazyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for LazyError {}

impl From<std::io::Error> for LazyError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<bincode::error::DecodeError> for LazyError {
    fn from(e: bincode::error::DecodeError) -> Self {
        Self::Decode(e)
    }
}

impl From<bincode::error::EncodeError> for LazyError {
    fn from(e: bincode::error::EncodeError) -> Self {
        Self::Encode(e)
    }
}

impl From<tokio::task::JoinError> for LazyError {
    fn from(e: tokio::task::JoinError) -> Self {
        Self::Join(e)
    }
}
