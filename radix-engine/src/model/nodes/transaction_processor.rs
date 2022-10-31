use sbor::rust::borrow::Cow;
use scrypto::resource::AuthZoneDrainInput;
use transaction::errors::IdAllocationError;
use transaction::model::*;
use transaction::validation::*;

use crate::engine::*;
use crate::fee::FeeReserve;
use crate::model::resolve_native_function;
use crate::model::resolve_native_method;
use crate::model::{InvokeError, WorktopSubstate};
use crate::model::{
    WorktopAssertContainsAmountInput, WorktopAssertContainsInput,
    WorktopAssertContainsNonFungiblesInput, WorktopDrainInput, WorktopPutInput,
    WorktopTakeAllInput, WorktopTakeAmountInput, WorktopTakeNonFungiblesInput,
};
use crate::types::*;

#[derive(Debug, TypeId, Encode, Decode)]
pub struct TransactionProcessorRunInput<'a> {
    pub instructions: Cow<'a, Vec<Instruction>>,
}

#[derive(Debug, Clone, PartialEq, Eq, TypeId, Encode, Decode)]
pub enum TransactionProcessorError {
    InvalidRequestData(DecodeError),
    InvalidMethod,
    BucketNotFound(BucketId),
    ProofNotFound(ProofId),
    NativeFunctionNotFound(NativeFunctionIdent),
    NativeMethodNotFound(NativeMethodIdent),
    IdAllocationError(IdAllocationError),
}

impl<'b> NativeExecutable for TransactionProcessorRunInput<'b> {
    type Output = Vec<Vec<u8>>;

    fn execute<'s, 'a, Y, R>(
        invocation: Self,
        system_api: &mut Y,
    ) -> Result<(Vec<Vec<u8>>, CallFrameUpdate), RuntimeError>
    where
        Y: SystemApi<'s, R>
            + Invokable<ScryptoInvocation>
            + InvokableNative<'a>
            + Invokable<NativeMethodInvocation>,
        R: FeeReserve,
    {
        TransactionProcessor::static_main(invocation, system_api)
            .map(|rtn| (rtn, CallFrameUpdate::empty()))
            .map_err(|e| e.into())
    }
}

impl<'a> NativeInvocation for TransactionProcessorRunInput<'a> {
    fn info(&self) -> NativeInvocationInfo {
        let mut node_refs_to_copy = HashSet::new();
        // TODO: Remove serialization
        let value = ScryptoValue::from_typed(self);
        for global_address in value.global_references() {
            node_refs_to_copy.insert(RENodeId::Global(global_address));
        }

        // TODO: This can be refactored out once any type in sbor is implemented
        for instruction in self.instructions.as_ref() {
            match instruction {
                Instruction::CallFunction { args, .. }
                | Instruction::CallMethod { args, .. }
                | Instruction::CallNativeFunction { args, .. }
                | Instruction::CallNativeMethod { args, .. } => {
                    let scrypto_value =
                        ScryptoValue::from_slice(&args).expect("Invalid CALL arguments");
                    for global_address in scrypto_value.global_references() {
                        node_refs_to_copy.insert(RENodeId::Global(global_address));
                    }
                }
                _ => {}
            }
        }
        node_refs_to_copy.insert(RENodeId::Global(GlobalAddress::Resource(RADIX_TOKEN)));
        node_refs_to_copy.insert(RENodeId::Global(GlobalAddress::System(EPOCH_MANAGER)));
        node_refs_to_copy.insert(RENodeId::Global(GlobalAddress::Resource(
            ECDSA_SECP256K1_TOKEN,
        )));
        node_refs_to_copy.insert(RENodeId::Global(GlobalAddress::Resource(
            EDDSA_ED25519_TOKEN,
        )));
        node_refs_to_copy.insert(RENodeId::Global(GlobalAddress::Package(ACCOUNT_PACKAGE)));

        NativeInvocationInfo::Function(
            NativeFunction::TransactionProcessor(TransactionProcessorFunction::Run),
            CallFrameUpdate {
                nodes_to_move: vec![],
                node_refs_to_copy,
            },
        )
    }
}

pub struct TransactionProcessor {}

impl TransactionProcessor {
    fn replace_node_id(
        node_id: RENodeId,
        proof_id_mapping: &mut HashMap<ProofId, ProofId>,
        bucket_id_mapping: &mut HashMap<BucketId, BucketId>,
    ) -> Result<RENodeId, InvokeError<TransactionProcessorError>> {
        match node_id {
            RENodeId::Bucket(bucket_id) => bucket_id_mapping
                .get(&bucket_id)
                .cloned()
                .map(RENodeId::Bucket)
                .ok_or(InvokeError::Error(
                    TransactionProcessorError::BucketNotFound(bucket_id),
                )),
            RENodeId::Proof(proof_id) => proof_id_mapping
                .get(&proof_id)
                .cloned()
                .map(RENodeId::Proof)
                .ok_or(InvokeError::Error(
                    TransactionProcessorError::ProofNotFound(proof_id),
                )),
            _ => Ok(node_id),
        }
    }

    fn replace_ids(
        proof_id_mapping: &mut HashMap<ProofId, ProofId>,
        bucket_id_mapping: &mut HashMap<BucketId, BucketId>,
        mut value: ScryptoValue,
    ) -> Result<ScryptoValue, InvokeError<TransactionProcessorError>> {
        value
            .replace_ids(proof_id_mapping, bucket_id_mapping)
            .map_err(|e| match e {
                ScryptoValueReplaceError::BucketIdNotFound(bucket_id) => {
                    InvokeError::Error(TransactionProcessorError::BucketNotFound(bucket_id))
                }
                ScryptoValueReplaceError::ProofIdNotFound(proof_id) => {
                    InvokeError::Error(TransactionProcessorError::ProofNotFound(proof_id))
                }
            })?;
        Ok(value)
    }

    fn process_expressions<'s, 'a, Y, R>(
        args: ScryptoValue,
        system_api: &mut Y,
    ) -> Result<ScryptoValue, InvokeError<TransactionProcessorError>>
    where
        Y: SystemApi<'s, R>
            + Invokable<ScryptoInvocation>
            + InvokableNative<'a>
            + Invokable<NativeMethodInvocation>,
        R: FeeReserve,
    {
        let mut value = args.dom;
        for (expression, path) in args.expressions {
            match expression.0.as_str() {
                "ENTIRE_WORKTOP" => {
                    let buckets = system_api
                        .invoke(WorktopDrainInput {})
                        .map_err(InvokeError::Downstream)?;

                    let val = path
                        .get_from_value_mut(&mut value)
                        .expect("Failed to locate an expression value using SBOR path");
                    *val =
                        decode_any(&scrypto_encode(&buckets)).expect("Failed to decode Vec<Bucket>")
                }
                "ENTIRE_AUTH_ZONE" => {
                    let node_ids = system_api
                        .get_visible_node_ids()
                        .map_err(InvokeError::Downstream)?;
                    let auth_zone_node_id = node_ids
                        .into_iter()
                        .find(|n| matches!(n, RENodeId::AuthZoneStack(..)))
                        .expect("AuthZone does not exist");
                    let auth_zone_id = auth_zone_node_id.into();

                    let proofs = system_api
                        .invoke(AuthZoneDrainInput { auth_zone_id })
                        .map_err(InvokeError::Downstream)?;

                    let val = path
                        .get_from_value_mut(&mut value)
                        .expect("Failed to locate an expression value using SBOR path");
                    *val =
                        decode_any(&scrypto_encode(&proofs)).expect("Failed to decode Vec<Proof>")
                }
                _ => {} // no-op
            }
        }

        Ok(ScryptoValue::from_value(value)
            .expect("Value became invalid post expression transformation"))
    }

    pub fn static_main<'s, 'a, Y, R>(
        input: TransactionProcessorRunInput,
        system_api: &mut Y,
    ) -> Result<Vec<Vec<u8>>, InvokeError<TransactionProcessorError>>
    where
        Y: SystemApi<'s, R>
            + Invokable<ScryptoInvocation>
            + InvokableNative<'a>
            + Invokable<NativeMethodInvocation>,
        R: FeeReserve,
    {
        let mut proof_id_mapping = HashMap::new();
        let mut bucket_id_mapping = HashMap::new();
        let mut outputs = Vec::new();
        let mut id_allocator = IdAllocator::new(IdSpace::Transaction);

        let _worktop_id = system_api
            .create_node(RENode::Worktop(WorktopSubstate::new()))
            .map_err(InvokeError::Downstream)?;

        let owned_node_ids = system_api
            .get_visible_node_ids()
            .map_err(InvokeError::Downstream)?;
        let auth_zone_node_id = owned_node_ids
            .into_iter()
            .find(|n| matches!(n, RENodeId::AuthZoneStack(..)))
            .expect("AuthZone does not exist");
        let auth_zone_ref = auth_zone_node_id;
        let auth_zone_id: AuthZoneId = auth_zone_ref.into();

        for inst in input.instructions.as_ref() {
            let result = match inst {
                Instruction::TakeFromWorktop { resource_address } => id_allocator
                    .new_bucket_id()
                    .map_err(|e| {
                        InvokeError::Error(TransactionProcessorError::IdAllocationError(e))
                    })
                    .and_then(|new_id| {
                        system_api
                            .invoke(WorktopTakeAllInput {
                                resource_address: *resource_address,
                            })
                            .map_err(InvokeError::Downstream)
                            .map(|bucket| {
                                bucket_id_mapping.insert(new_id, bucket.0);
                                ScryptoValue::from_typed(&bucket)
                            })
                    }),
                Instruction::TakeFromWorktopByAmount {
                    amount,
                    resource_address,
                } => id_allocator
                    .new_bucket_id()
                    .map_err(|e| {
                        InvokeError::Error(TransactionProcessorError::IdAllocationError(e))
                    })
                    .and_then(|new_id| {
                        system_api
                            .invoke(WorktopTakeAmountInput {
                                amount: *amount,
                                resource_address: *resource_address,
                            })
                            .map_err(InvokeError::Downstream)
                            .map(|bucket| {
                                bucket_id_mapping.insert(new_id, bucket.0);
                                ScryptoValue::from_typed(&bucket)
                            })
                    }),
                Instruction::TakeFromWorktopByIds {
                    ids,
                    resource_address,
                } => id_allocator
                    .new_bucket_id()
                    .map_err(|e| {
                        InvokeError::Error(TransactionProcessorError::IdAllocationError(e))
                    })
                    .and_then(|new_id| {
                        system_api
                            .invoke(WorktopTakeNonFungiblesInput {
                                ids: ids.clone(),
                                resource_address: *resource_address,
                            })
                            .map_err(InvokeError::Downstream)
                            .map(|bucket| {
                                bucket_id_mapping.insert(new_id, bucket.0);
                                ScryptoValue::from_typed(&bucket)
                            })
                    }),
                Instruction::ReturnToWorktop { bucket_id } => bucket_id_mapping
                    .remove(bucket_id)
                    .map(|real_id| {
                        system_api
                            .invoke(WorktopPutInput {
                                bucket: scrypto::resource::Bucket(real_id),
                            })
                            .map(|rtn| ScryptoValue::from_typed(&rtn))
                            .map_err(InvokeError::Downstream)
                    })
                    .unwrap_or(Err(InvokeError::Error(
                        TransactionProcessorError::BucketNotFound(*bucket_id),
                    ))),
                Instruction::AssertWorktopContains { resource_address } => system_api
                    .invoke(WorktopAssertContainsInput {
                        resource_address: *resource_address,
                    })
                    .map(|rtn| ScryptoValue::from_typed(&rtn))
                    .map_err(InvokeError::Downstream),
                Instruction::AssertWorktopContainsByAmount {
                    amount,
                    resource_address,
                } => system_api
                    .invoke(WorktopAssertContainsAmountInput {
                        amount: *amount,
                        resource_address: *resource_address,
                    })
                    .map(|rtn| ScryptoValue::from_typed(&rtn))
                    .map_err(InvokeError::Downstream),
                Instruction::AssertWorktopContainsByIds {
                    ids,
                    resource_address,
                } => system_api
                    .invoke(WorktopAssertContainsNonFungiblesInput {
                        ids: ids.clone(),
                        resource_address: *resource_address,
                    })
                    .map(|rtn| ScryptoValue::from_typed(&rtn))
                    .map_err(InvokeError::Downstream),

                Instruction::PopFromAuthZone {} => id_allocator
                    .new_proof_id()
                    .map_err(|e| {
                        InvokeError::Error(TransactionProcessorError::IdAllocationError(e))
                    })
                    .and_then(|new_id| {
                        system_api
                            .invoke(AuthZonePopInput { auth_zone_id })
                            .map_err(InvokeError::Downstream)
                            .map(|proof| {
                                proof_id_mapping.insert(new_id, proof.0);
                                ScryptoValue::from_typed(&proof)
                            })
                    }),
                Instruction::ClearAuthZone => {
                    proof_id_mapping.clear();
                    system_api
                        .invoke(AuthZoneClearInput { auth_zone_id })
                        .map(|rtn| ScryptoValue::from_typed(&rtn))
                        .map_err(InvokeError::Downstream)
                }
                Instruction::PushToAuthZone { proof_id } => proof_id_mapping
                    .remove(proof_id)
                    .ok_or(InvokeError::Error(
                        TransactionProcessorError::ProofNotFound(*proof_id),
                    ))
                    .and_then(|real_id| {
                        system_api
                            .invoke(AuthZonePushInput {
                                auth_zone_id,
                                proof: scrypto::resource::Proof(real_id),
                            })
                            .map(|rtn| ScryptoValue::from_typed(&rtn))
                            .map_err(InvokeError::Downstream)
                    }),
                Instruction::CreateProofFromAuthZone { resource_address } => id_allocator
                    .new_proof_id()
                    .map_err(|e| {
                        InvokeError::Error(TransactionProcessorError::IdAllocationError(e))
                    })
                    .and_then(|new_id| {
                        system_api
                            .invoke(AuthZoneCreateProofInput {
                                resource_address: *resource_address,
                                auth_zone_id,
                            })
                            .map_err(InvokeError::Downstream)
                            .map(|proof| {
                                proof_id_mapping.insert(new_id, proof.0);
                                ScryptoValue::from_typed(&proof)
                            })
                    }),
                Instruction::CreateProofFromAuthZoneByAmount {
                    amount,
                    resource_address,
                } => id_allocator
                    .new_proof_id()
                    .map_err(|e| {
                        InvokeError::Error(TransactionProcessorError::IdAllocationError(e))
                    })
                    .and_then(|new_id| {
                        system_api
                            .invoke(AuthZoneCreateProofByAmountInput {
                                amount: *amount,
                                resource_address: *resource_address,
                                auth_zone_id,
                            })
                            .map_err(InvokeError::Downstream)
                            .map(|proof| {
                                proof_id_mapping.insert(new_id, proof.0);
                                ScryptoValue::from_typed(&proof)
                            })
                    }),
                Instruction::CreateProofFromAuthZoneByIds {
                    ids,
                    resource_address,
                } => id_allocator
                    .new_proof_id()
                    .map_err(|e| {
                        InvokeError::Error(TransactionProcessorError::IdAllocationError(e))
                    })
                    .and_then(|new_id| {
                        system_api
                            .invoke(AuthZoneCreateProofByIdsInput {
                                auth_zone_id,
                                ids: ids.clone(),
                                resource_address: *resource_address,
                            })
                            .map_err(InvokeError::Downstream)
                            .map(|proof| {
                                proof_id_mapping.insert(new_id, proof.0);
                                ScryptoValue::from_typed(&proof)
                            })
                    }),
                Instruction::CreateProofFromBucket { bucket_id } => id_allocator
                    .new_proof_id()
                    .map_err(|e| {
                        InvokeError::Error(TransactionProcessorError::IdAllocationError(e))
                    })
                    .and_then(|new_id| {
                        bucket_id_mapping
                            .get(bucket_id)
                            .cloned()
                            .map(|real_bucket_id| (new_id, real_bucket_id))
                            .ok_or(InvokeError::Error(
                                TransactionProcessorError::BucketNotFound(new_id),
                            ))
                    })
                    .and_then(|(new_id, real_bucket_id)| {
                        system_api
                            .invoke(BucketCreateProofInput {
                                bucket_id: real_bucket_id,
                            })
                            .map_err(InvokeError::Downstream)
                            .map(|proof| {
                                proof_id_mapping.insert(new_id, proof.0);
                                ScryptoValue::from_typed(&proof)
                            })
                    }),
                Instruction::CloneProof { proof_id } => id_allocator
                    .new_proof_id()
                    .map_err(|e| {
                        InvokeError::Error(TransactionProcessorError::IdAllocationError(e))
                    })
                    .and_then(|new_id| {
                        proof_id_mapping
                            .get(proof_id)
                            .cloned()
                            .map(|real_id| {
                                system_api
                                    .invoke(ProofCloneInput { proof_id: real_id })
                                    .map_err(InvokeError::Downstream)
                                    .map(|proof| {
                                        proof_id_mapping.insert(new_id, proof.0);
                                        ScryptoValue::from_typed(&proof)
                                    })
                            })
                            .unwrap_or(Err(InvokeError::Error(
                                TransactionProcessorError::ProofNotFound(*proof_id),
                            )))
                    }),
                Instruction::DropProof { proof_id } => proof_id_mapping
                    .remove(proof_id)
                    .map(|real_id| {
                        system_api
                            .drop_node(RENodeId::Proof(real_id))
                            .map(|_| ScryptoValue::unit())
                            .map_err(InvokeError::Downstream)
                    })
                    .unwrap_or(Err(InvokeError::Error(
                        TransactionProcessorError::ProofNotFound(*proof_id),
                    ))),
                Instruction::DropAllProofs => {
                    for (_, real_id) in proof_id_mapping.drain() {
                        system_api
                            .drop_node(RENodeId::Proof(real_id))
                            .map_err(InvokeError::Downstream)?;
                    }
                    system_api
                        .invoke(AuthZoneClearInput { auth_zone_id })
                        .map(|rtn| ScryptoValue::from_typed(&rtn))
                        .map_err(InvokeError::Downstream)
                }
                Instruction::CallFunction {
                    function_ident,
                    args,
                } => {
                    Self::replace_ids(
                        &mut proof_id_mapping,
                        &mut bucket_id_mapping,
                        ScryptoValue::from_slice(args).expect("Invalid CALL_FUNCTION arguments"),
                    )
                    .and_then(|args| Self::process_expressions(args, system_api))
                    .and_then(|args| {
                        system_api
                            .invoke(ScryptoInvocation::Function(function_ident.clone(), args))
                            .map_err(InvokeError::Downstream)
                    })
                    .and_then(|result| {
                        // Auto move into auth_zone
                        for (proof_id, _) in &result.proof_ids {
                            system_api
                                .invoke(AuthZonePushInput {
                                    auth_zone_id,
                                    proof: scrypto::resource::Proof(*proof_id),
                                })
                                .map(|rtn| ScryptoValue::from_typed(&rtn))
                                .map_err(InvokeError::Downstream)?;
                        }
                        // Auto move into worktop
                        for (bucket_id, _) in &result.bucket_ids {
                            system_api
                                .invoke(WorktopPutInput {
                                    bucket: scrypto::resource::Bucket(*bucket_id),
                                })
                                .map_err(InvokeError::Downstream)?;
                        }
                        Ok(result)
                    })
                }
                Instruction::CallMethod { method_ident, args } => {
                    Self::replace_ids(
                        &mut proof_id_mapping,
                        &mut bucket_id_mapping,
                        ScryptoValue::from_slice(args).expect("Invalid CALL_METHOD arguments"),
                    )
                    .and_then(|args| Self::process_expressions(args, system_api))
                    .and_then(|args| {
                        system_api
                            .invoke(ScryptoInvocation::Method(method_ident.clone(), args))
                            .map_err(InvokeError::Downstream)
                    })
                    .and_then(|result| {
                        // Auto move into auth_zone
                        for (proof_id, _) in &result.proof_ids {
                            system_api
                                .invoke(AuthZonePushInput {
                                    auth_zone_id,
                                    proof: scrypto::resource::Proof(*proof_id),
                                })
                                .map(|rtn| ScryptoValue::from_typed(&rtn))
                                .map_err(InvokeError::Downstream)?;
                        }
                        // Auto move into worktop
                        for (bucket_id, _) in &result.bucket_ids {
                            system_api
                                .invoke(WorktopPutInput {
                                    bucket: scrypto::resource::Bucket(*bucket_id),
                                })
                                .map_err(InvokeError::downstream)?;
                        }
                        Ok(result)
                    })
                }
                Instruction::PublishPackage { code, abi } => system_api
                    .invoke(PackagePublishInput {
                        code: code.clone(),
                        abi: abi.clone(),
                    })
                    .map(|address| ScryptoValue::from_typed(&address))
                    .map_err(InvokeError::Downstream),
                Instruction::CallNativeFunction {
                    function_ident,
                    args,
                } => {
                    Self::replace_ids(
                        &mut proof_id_mapping,
                        &mut bucket_id_mapping,
                        ScryptoValue::from_slice(args)
                            .expect("Invalid CALL_NATIVE_FUNCTION arguments"),
                    )
                    .and_then(|args| Self::process_expressions(args, system_api))
                    .and_then(|args| {
                        let native_function = resolve_native_function(
                            &function_ident.blueprint_name,
                            &function_ident.function_name,
                        )
                        .ok_or(InvokeError::Error(
                            TransactionProcessorError::NativeFunctionNotFound(
                                function_ident.clone(),
                            ),
                        ))?;
                        match native_function {
                            NativeFunction::EpochManager(EpochManagerFunction::Create) => {
                                let invocation: EpochManagerCreateInput = scrypto_decode(&args.raw)
                                    .map_err(|e| {
                                        InvokeError::Error(
                                            TransactionProcessorError::InvalidRequestData(e),
                                        )
                                    })?;
                                system_api
                                    .invoke(invocation)
                                    .map(|a| ScryptoValue::from_typed(&a))
                            }
                            NativeFunction::ResourceManager(
                                ResourceManagerFunction::BurnBucket,
                            ) => {
                                let invocation: ResourceManagerBurnInput =
                                    scrypto_decode(&args.raw).map_err(|e| {
                                        InvokeError::Error(
                                            TransactionProcessorError::InvalidRequestData(e),
                                        )
                                    })?;
                                system_api
                                    .invoke(invocation)
                                    .map(|a| ScryptoValue::from_typed(&a))
                            }
                            NativeFunction::ResourceManager(ResourceManagerFunction::Create) => {
                                let invocation: ResourceManagerCreateInput =
                                    scrypto_decode(&args.raw).map_err(|e| {
                                        InvokeError::Error(
                                            TransactionProcessorError::InvalidRequestData(e),
                                        )
                                    })?;
                                system_api
                                    .invoke(invocation)
                                    .map(|a| ScryptoValue::from_typed(&a))
                            }
                            NativeFunction::TransactionProcessor(
                                TransactionProcessorFunction::Run,
                            ) => {
                                let invocation: TransactionProcessorRunInput =
                                    scrypto_decode(&args.raw).map_err(|e| {
                                        InvokeError::Error(
                                            TransactionProcessorError::InvalidRequestData(e),
                                        )
                                    })?;
                                system_api
                                    .invoke(invocation)
                                    .map(|a| ScryptoValue::from_typed(&a))
                            }
                            NativeFunction::Package(PackageFunction::Publish) => {
                                let invocation: PackagePublishInput = scrypto_decode(&args.raw)
                                    .map_err(|e| {
                                        InvokeError::Error(
                                            TransactionProcessorError::InvalidRequestData(e),
                                        )
                                    })?;
                                system_api
                                    .invoke(invocation)
                                    .map(|a| ScryptoValue::from_typed(&a))
                            }
                        }
                        .map_err(InvokeError::Downstream)
                    })
                    .and_then(|result| {
                        // Auto move into auth_zone
                        for (proof_id, _) in &result.proof_ids {
                            system_api
                                .invoke(AuthZonePushInput {
                                    proof: scrypto::resource::Proof(*proof_id),
                                    auth_zone_id,
                                })
                                .map_err(InvokeError::Downstream)?;
                        }
                        // Auto move into worktop
                        for (bucket_id, _) in &result.bucket_ids {
                            system_api
                                .invoke(WorktopPutInput {
                                    bucket: scrypto::resource::Bucket(*bucket_id),
                                })
                                .map_err(InvokeError::Downstream)?;
                        }
                        Ok(result)
                    })
                }
                Instruction::CallNativeMethod { method_ident, args } => {
                    Self::replace_ids(
                        &mut proof_id_mapping,
                        &mut bucket_id_mapping,
                        ScryptoValue::from_slice(args)
                            .expect("Invalid CALL_NATIVE_METHOD arguments"),
                    )
                    .and_then(|args| Self::process_expressions(args, system_api))
                    .and_then(|args| {
                        let native_method =
                            resolve_native_method(
                                method_ident.receiver,
                                &method_ident.method_name,
                            )
                                .ok_or(InvokeError::Error(
                                    TransactionProcessorError::NativeMethodNotFound(
                                        method_ident.clone(),
                                    ),
                                ))?;
                        match native_method {
                            NativeMethod::ResourceManager(ResourceManagerMethod::Burn) => {
                                let invocation: ResourceManagerBurnInput = scrypto_decode(&args.raw)
                                    .map_err(|e| InvokeError::Error(
                                        TransactionProcessorError::InvalidRequestData(e),
                                    ))?;
                                system_api
                                    .invoke(invocation)
                                    .map(|a| ScryptoValue::from_typed(&a))
                            }
                            NativeMethod::ResourceManager(ResourceManagerMethod::Mint) => {
                                let invocation: ResourceManagerMintInput = scrypto_decode(&args.raw)
                                    .map_err(|e| InvokeError::Error(
                                        TransactionProcessorError::InvalidRequestData(e),
                                    ))?;
                                system_api
                                    .invoke(invocation)
                                    .map(|a| ScryptoValue::from_typed(&a))
                            }
                            _ => {
                                system_api
                                    .invoke(NativeMethodInvocation(native_method,
                                                Self::replace_node_id(
                                                    method_ident.receiver,
                                                    &mut proof_id_mapping,
                                                    &mut bucket_id_mapping,
                                                )?,
                                                args,
                                            ))
                            }
                        }.map_err(InvokeError::Downstream)
                    })
                    .and_then(|result| {
                        // Auto move into auth_zone
                        for (proof_id, _) in &result.proof_ids {
                            system_api
                                .invoke(AuthZonePushInput {
                                    proof: scrypto::resource::Proof(*proof_id),
                                    auth_zone_id,
                                })
                                .map_err(InvokeError::Downstream)?;
                        }
                        // Auto move into worktop
                        for (bucket_id, _) in &result.bucket_ids {
                            system_api
                                .invoke(WorktopPutInput {
                                    bucket: scrypto::resource::Bucket(*bucket_id),
                                })
                                .map_err(InvokeError::downstream)?;
                        }
                        Ok(result)
                    })
                }
            }?;
            outputs.push(result);
        }

        Ok(outputs
            .into_iter()
            .map(|sv| sv.raw)
            .collect::<Vec<Vec<u8>>>())
    }
}
