use std::fmt;

#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    EmptyBuffer,
    UnknownMessageType(u8),
    MalformedData,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::EmptyBuffer => write!(f, "attempted to parse empty input"),
            ParseError::UnknownMessageType(t) => write!(f, "unknown message type {t}"),
            ParseError::MalformedData => write!(f, "malformed input data"),
        }
    }
}

impl std::error::Error for ParseError {}
