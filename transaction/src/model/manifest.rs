use crate::model::BasicInstruction;
use radix_engine_interface::*;

#[derive(Debug, Clone, PartialEq, Eq, ScryptoSbor)]
pub struct TransactionManifest {
    pub instructions: Vec<BasicInstruction>,
    pub blobs: Vec<Vec<u8>>,
}
