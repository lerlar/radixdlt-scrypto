use crate::*;
use sbor::*;

/// Represents type of non-fungible id
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Sbor, LegacyDescribe)]
pub enum NonFungibleIdType {
    String,
    Integer,
    Bytes,
    UUID,
}
