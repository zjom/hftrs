use crate::types::{OrderId, Quantity};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BookError {
    DuplicateOrder(OrderId),
    UnknownOrder(OrderId),
    OverExecute {
        id: OrderId,
        got: Quantity,
        avail: Quantity,
    },
    OverCancel {
        id: OrderId,
        got: Quantity,
        avail: Quantity,
    },
}

impl fmt::Display for BookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BookError::DuplicateOrder(id) => write!(f, "order {id} already exists"),
            BookError::UnknownOrder(id) => write!(f, "order {id} not found"),
            BookError::OverExecute { id, got, avail } => write!(
                f,
                "execute qty {got} exceeds resting qty {avail} for order {id}"
            ),
            BookError::OverCancel { id, got, avail } => write!(
                f,
                "cancel qty {got} exceeds resting qty {avail} for order {id}"
            ),
        }
    }
}

impl std::error::Error for BookError {}

pub type Result<T> = std::result::Result<T, BookError>;
