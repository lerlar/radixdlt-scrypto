use super::node_modules::type_info::{TypeInfoBlueprint, TypeInfoSubstate};
use crate::blueprints::account::ACCOUNT_CREATE_VIRTUAL_ED25519_ID;
use crate::blueprints::account::ACCOUNT_CREATE_VIRTUAL_SECP256K1_ID;
use crate::blueprints::identity::IDENTITY_CREATE_VIRTUAL_ED25519_ID;
use crate::blueprints::identity::IDENTITY_CREATE_VIRTUAL_SECP256K1_ID;
use crate::blueprints::resource::AuthZone;
use crate::errors::RuntimeError;
use crate::errors::SystemUpstreamError;
use crate::kernel::actor::Actor;
use crate::kernel::actor::BlueprintHookActor;
use crate::kernel::actor::FunctionActor;
use crate::kernel::actor::MethodActor;
use crate::kernel::call_frame::Message;
use crate::kernel::kernel_api::KernelSubstateApi;
use crate::kernel::kernel_api::{KernelApi, KernelInvocation};
use crate::kernel::kernel_callback_api::KernelCallbackObject;
use crate::system::module::SystemModule;
use crate::system::system::FieldSubstate;
use crate::system::system::KeyValueEntrySubstate;
use crate::system::system::SystemService;
use crate::system::system_callback_api::SystemCallbackObject;
use crate::system::system_modules::SystemModuleMixer;
use crate::track::interface::StoreAccess;
use crate::types::*;
use radix_engine_interface::api::field_api::LockFlags;
use radix_engine_interface::api::ClientBlueprintApi;
use radix_engine_interface::api::ClientObjectApi;
use radix_engine_interface::blueprints::account::ACCOUNT_BLUEPRINT;
use radix_engine_interface::blueprints::identity::IDENTITY_BLUEPRINT;
use radix_engine_interface::blueprints::package::*;
use radix_engine_interface::hooks::OnDropInput;
use radix_engine_interface::hooks::OnDropOutput;
use radix_engine_interface::hooks::OnMoveOutput;
use radix_engine_interface::hooks::OnPersistOutput;
use radix_engine_interface::hooks::OnVirtualizeInput;
use radix_engine_interface::hooks::OnVirtualizeOutput;
use radix_engine_interface::schema::{InstanceSchema, RefTypes};

#[derive(Clone)]
pub enum SystemLockData {
    KeyValueEntry(KeyValueEntryLockData),
    Field(FieldLockData),
    Default,
}

impl Default for SystemLockData {
    fn default() -> Self {
        SystemLockData::Default
    }
}

#[derive(Clone)]
pub enum KeyValueEntryLockData {
    Read,
    Write {
        schema: ScryptoSchema,
        index: LocalTypeIndex,
        can_own: bool,
    },
    BlueprintWrite {
        blueprint_id: BlueprintId,
        instance_schema: Option<InstanceSchema>,
        type_pointer: TypePointer,
        can_own: bool,
    },
}

#[derive(Clone)]
pub enum FieldLockData {
    Read,
    Write {
        blueprint_id: BlueprintId,
        type_pointer: TypePointer,
    },
}

impl SystemLockData {
    pub fn is_kv_entry(&self) -> bool {
        matches!(self, SystemLockData::KeyValueEntry(..))
    }
}

pub struct SystemConfig<C: SystemCallbackObject> {
    pub callback_obj: C,
    pub blueprint_cache: NonIterMap<CanonicalBlueprintId, BlueprintDefinition>,
    pub schema_cache: NonIterMap<Hash, ScryptoSchema>,
    pub auth_cache: NonIterMap<CanonicalBlueprintId, AuthConfig>,
    pub modules: SystemModuleMixer,
}

impl<C: SystemCallbackObject> KernelCallbackObject for SystemConfig<C> {
    type LockData = SystemLockData;

    fn on_init<Y>(api: &mut Y) -> Result<(), RuntimeError>
    where
        Y: KernelApi<Self>,
    {
        SystemModuleMixer::on_init(api)
    }

    fn on_teardown<Y>(api: &mut Y) -> Result<(), RuntimeError>
    where
        Y: KernelApi<Self>,
    {
        SystemModuleMixer::on_teardown(api)
    }

    fn before_drop_node<Y>(node_id: &NodeId, api: &mut Y) -> Result<(), RuntimeError>
    where
        Y: KernelApi<Self>,
    {
        SystemModuleMixer::before_drop_node(api, node_id)
    }

    fn after_drop_node<Y>(api: &mut Y, total_substate_size: usize) -> Result<(), RuntimeError>
    where
        Y: KernelApi<Self>,
    {
        SystemModuleMixer::after_drop_node(api, total_substate_size)
    }

    fn before_create_node<Y>(
        node_id: &NodeId,
        node_module_init: &BTreeMap<PartitionNumber, BTreeMap<SubstateKey, IndexedScryptoValue>>,
        api: &mut Y,
    ) -> Result<(), RuntimeError>
    where
        Y: KernelApi<Self>,
    {
        SystemModuleMixer::before_create_node(api, node_id, node_module_init)
    }

    fn before_open_substate<Y>(
        node_id: &NodeId,
        partition_num: &PartitionNumber,
        substate_key: &SubstateKey,
        flags: &LockFlags,
        api: &mut Y,
    ) -> Result<(), RuntimeError>
    where
        Y: KernelApi<Self>,
    {
        SystemModuleMixer::before_open_substate(api, node_id, partition_num, substate_key, flags)
    }

    fn after_open_substate<Y>(
        handle: LockHandle,
        node_id: &NodeId,
        size: usize,
        api: &mut Y,
    ) -> Result<(), RuntimeError>
    where
        Y: KernelApi<Self>,
    {
        SystemModuleMixer::after_open_substate(api, handle, node_id, size)
    }

    fn on_close_substate<Y>(lock_handle: LockHandle, api: &mut Y) -> Result<(), RuntimeError>
    where
        Y: KernelApi<Self>,
    {
        SystemModuleMixer::on_close_substate(api, lock_handle)
    }

    fn on_read_substate<Y>(
        lock_handle: LockHandle,
        value_size: usize,
        api: &mut Y,
    ) -> Result<(), RuntimeError>
    where
        Y: KernelApi<Self>,
    {
        SystemModuleMixer::on_read_substate(api, lock_handle, value_size)
    }

    fn on_write_substate<Y>(
        lock_handle: LockHandle,
        value_size: usize,
        api: &mut Y,
    ) -> Result<(), RuntimeError>
    where
        Y: KernelApi<Self>,
    {
        SystemModuleMixer::on_write_substate(api, lock_handle, value_size)
    }

    fn on_store_access(&mut self, store_access: &StoreAccess) -> Result<(), RuntimeError> {
        SystemModuleMixer::on_store_access(store_access, self)
    }

    fn on_set_substate<Y>(value_size: usize, api: &mut Y) -> Result<(), RuntimeError>
    where
        Y: KernelApi<Self>,
    {
        SystemModuleMixer::on_set_substate(api, value_size)
    }

    fn on_remove_substate<Y>(api: &mut Y) -> Result<(), RuntimeError>
    where
        Y: KernelApi<Self>,
    {
        SystemModuleMixer::on_remove_substate(api)
    }

    fn on_scan_keys<Y>(api: &mut Y) -> Result<(), RuntimeError>
    where
        Y: KernelApi<Self>,
    {
        SystemModuleMixer::on_scan_keys(api)
    }

    fn on_scan_sorted_substates<Y>(api: &mut Y) -> Result<(), RuntimeError>
    where
        Y: KernelApi<Self>,
    {
        SystemModuleMixer::on_scan_sorted_substates(api)
    }

    fn on_drain_substates<Y>(
        api: &mut Y,
    ) -> Result<(), RuntimeError>
        where
            Y: KernelApi<Self>,
    {
        SystemModuleMixer::on_drain_substates(api)
    }

    fn after_create_node<Y>(node_id: &NodeId, api: &mut Y) -> Result<(), RuntimeError>
    where
        Y: KernelApi<Self>,
    {
        SystemModuleMixer::after_create_node(api, node_id)
    }

    fn before_invoke<Y>(invocation: &KernelInvocation, api: &mut Y) -> Result<(), RuntimeError>
    where
        Y: KernelApi<Self>,
    {
        SystemModuleMixer::before_invoke(api, invocation)
    }

    fn after_invoke<Y>(output_size: usize, api: &mut Y) -> Result<(), RuntimeError>
    where
        Y: KernelApi<Self>,
    {
        SystemModuleMixer::after_invoke(api, output_size)
    }

    fn before_push_frame<Y>(
        callee: &Actor,
        update: &mut Message,
        args: &IndexedScryptoValue,
        api: &mut Y,
    ) -> Result<(), RuntimeError>
    where
        Y: KernelApi<Self>,
    {
        SystemModuleMixer::before_push_frame(api, callee, update, args)
    }

    fn on_execution_start<Y>(api: &mut Y) -> Result<(), RuntimeError>
    where
        Y: KernelApi<Self>,
    {
        SystemModuleMixer::on_execution_start(api)
    }

    fn on_execution_finish<Y>(update: &Message, api: &mut Y) -> Result<(), RuntimeError>
    where
        Y: KernelApi<Self>,
    {
        SystemModuleMixer::on_execution_finish(api, update)
    }

    fn after_pop_frame<Y>(api: &mut Y, dropped_actor: &Actor) -> Result<(), RuntimeError>
    where
        Y: KernelApi<Self>,
    {
        SystemModuleMixer::after_pop_frame(api, dropped_actor)
    }

    fn on_allocate_node_id<Y>(entity_type: EntityType, api: &mut Y) -> Result<(), RuntimeError>
    where
        Y: KernelApi<Self>,
    {
        SystemModuleMixer::on_allocate_node_id(api, entity_type)
    }

    //--------------------------------------------------------------------------
    // Note that the following logic doesn't go through mixer and is not costed
    //--------------------------------------------------------------------------

    fn invoke_upstream<Y>(
        input: &IndexedScryptoValue,
        api: &mut Y,
    ) -> Result<IndexedScryptoValue, RuntimeError>
    where
        Y: KernelApi<SystemConfig<C>>,
    {
        let mut system = SystemService::new(api);
        let actor = system.current_actor();
        let node_id = actor.node_id();
        let is_direct_access = actor.is_direct_access();

        // Make dependent resources/components visible
        if let Some(blueprint_id) = actor.blueprint_id() {
            let key = BlueprintVersionKey {
                blueprint: blueprint_id.blueprint_name.clone(),
                version: BlueprintVersion::default(),
            };

            let handle = system.kernel_open_substate_with_default(
                blueprint_id.package_address.as_node_id(),
                MAIN_BASE_PARTITION
                    .at_offset(PACKAGE_BLUEPRINT_DEPENDENCIES_PARTITION_OFFSET)
                    .unwrap(),
                &SubstateKey::Map(scrypto_encode(&key).unwrap()),
                LockFlags::read_only(),
                Some(|| {
                    let kv_entry = KeyValueEntrySubstate::<()>::default();
                    IndexedScryptoValue::from_typed(&kv_entry)
                }),
                SystemLockData::default(),
            )?;
            system.kernel_read_substate(handle)?;
            system.kernel_close_substate(handle)?;
        }

        match &actor {
            Actor::Root => panic!("Root is invoked"),
            actor @ Actor::Method(MethodActor { ident, .. })
            | actor @ Actor::Function(FunctionActor { ident, .. }) => {
                let blueprint_id = actor.blueprint_id().unwrap();

                //  Validate input
                let definition = system.load_blueprint_definition(
                    blueprint_id.package_address,
                    &BlueprintVersionKey::new_default(blueprint_id.blueprint_name.as_str()),
                )?;
                let input_type_pointer = definition
                    .interface
                    .get_function_input_type_pointer(ident.as_str())
                    .ok_or_else(|| {
                        RuntimeError::SystemUpstreamError(SystemUpstreamError::FnNotFound(
                            ident.to_string(),
                        ))
                    })?;
                system.validate_payload_against_blueprint_schema(
                    &blueprint_id,
                    &None,
                    &[(input.as_vec_ref(), input_type_pointer)],
                )?;

                // Validate receiver type
                let function_schema = definition
                    .interface
                    .functions
                    .get(ident)
                    .expect("Should exist due to schema check");
                match (&function_schema.receiver, node_id) {
                    (Some(receiver_info), Some(_)) => {
                        if is_direct_access
                            != receiver_info.ref_types.contains(RefTypes::DIRECT_ACCESS)
                        {
                            return Err(RuntimeError::SystemUpstreamError(
                                SystemUpstreamError::ReceiverNotMatch(ident.to_string()),
                            ));
                        }
                    }
                    (None, None) => {}
                    _ => {
                        return Err(RuntimeError::SystemUpstreamError(
                            SystemUpstreamError::ReceiverNotMatch(ident.to_string()),
                        ));
                    }
                }

                // Execute
                let export = definition
                    .function_exports
                    .get(ident)
                    .expect("Schema should have validated this exists")
                    .clone();
                let output =
                    { C::invoke(&blueprint_id.package_address, export, input, &mut system)? };

                // Validate output
                let output_type_pointer = definition
                    .interface
                    .get_function_output_type_pointer(ident.as_str())
                    .expect("Schema verification should enforce that this exists.");
                system.validate_payload_against_blueprint_schema(
                    &blueprint_id,
                    &None,
                    &[(output.as_vec_ref(), output_type_pointer)],
                )?;
                Ok(output)
            }
            Actor::BlueprintHook(BlueprintHookActor {
                blueprint_id, hook, ..
            }) => {
                // Find the export
                let definition = system.load_blueprint_definition(
                    blueprint_id.package_address,
                    &BlueprintVersionKey::new_default(blueprint_id.blueprint_name.as_str()),
                )?;
                let export =
                    definition
                        .hook_exports
                        .get(&hook)
                        .ok_or(RuntimeError::SystemUpstreamError(
                            SystemUpstreamError::HookNotFound(hook.clone()),
                        ))?;

                // Input is not validated as they're created by system.

                // Invoke the export
                let output = C::invoke(
                    &blueprint_id.package_address,
                    export.clone(),
                    &input,
                    &mut system,
                )?;

                // Check output against well-known schema
                match hook {
                    BlueprintHook::OnVirtualize => {
                        scrypto_decode::<OnVirtualizeOutput>(output.as_slice()).map(|_| ())
                    }
                    BlueprintHook::OnDrop => {
                        scrypto_decode::<OnDropOutput>(output.as_slice()).map(|_| ())
                    }
                    BlueprintHook::OnMove => {
                        scrypto_decode::<OnMoveOutput>(output.as_slice()).map(|_| ())
                    }
                    BlueprintHook::OnPersist => {
                        scrypto_decode::<OnPersistOutput>(output.as_slice()).map(|_| ())
                    }
                }
                .map_err(|e| {
                    RuntimeError::SystemUpstreamError(SystemUpstreamError::OutputDecodeError(e))
                })?;

                Ok(output)
            }
        }
    }

    // Note: we check dangling nodes, in kernel, after auto-drop
    fn auto_drop<Y>(nodes: Vec<NodeId>, api: &mut Y) -> Result<(), RuntimeError>
    where
        Y: KernelApi<Self>,
    {
        // Round 1 - drop all proofs
        for node_id in nodes {
            let type_info = TypeInfoBlueprint::get_type(&node_id, api)?;

            match type_info {
                TypeInfoSubstate::Object(ObjectInfo {
                    blueprint_info: BlueprintInfo { blueprint_id, .. },
                    ..
                }) => {
                    match (
                        blueprint_id.package_address,
                        blueprint_id.blueprint_name.as_str(),
                    ) {
                        (RESOURCE_PACKAGE, FUNGIBLE_PROOF_BLUEPRINT) => {
                            let mut system = SystemService::new(api);
                            system.call_function(
                                RESOURCE_PACKAGE,
                                FUNGIBLE_PROOF_BLUEPRINT,
                                PROOF_DROP_IDENT,
                                scrypto_encode(&ProofDropInput {
                                    proof: Proof(Own(node_id)),
                                })
                                .unwrap(),
                            )?;
                        }
                        (RESOURCE_PACKAGE, NON_FUNGIBLE_PROOF_BLUEPRINT) => {
                            let mut system = SystemService::new(api);
                            system.call_function(
                                RESOURCE_PACKAGE,
                                NON_FUNGIBLE_PROOF_BLUEPRINT,
                                PROOF_DROP_IDENT,
                                scrypto_encode(&ProofDropInput {
                                    proof: Proof(Own(node_id)),
                                })
                                .unwrap(),
                            )?;
                        }
                        _ => {
                            // no-op
                        }
                    }
                }
                _ => {}
            }
        }

        // Round 2 - drop the auth zone
        //
        // Note that we destroy frame's auth zone at the very end of the `auto_drop` process
        // to make sure the auth zone stack is in good state for the proof dropping above.
        //
        if let Some(auth_zone_id) = api.kernel_get_system().modules.auth_zone_id() {
            // Detach proofs from the auth zone
            let handle = api.kernel_open_substate(
                &auth_zone_id,
                MAIN_BASE_PARTITION,
                &AuthZoneField::AuthZone.into(),
                LockFlags::MUTABLE,
                SystemLockData::Default,
            )?;
            let mut substate: FieldSubstate<AuthZone> =
                api.kernel_read_substate(handle)?.as_typed().unwrap();
            let proofs = core::mem::replace(&mut substate.value.0.proofs, Vec::new());
            api.kernel_write_substate(handle, IndexedScryptoValue::from_typed(&substate.value.0))?;
            api.kernel_close_substate(handle)?;

            // Drop all proofs (previously) owned by the auth zone
            for proof in proofs {
                let mut system = SystemService::new(api);
                let object_info = system.get_object_info(proof.0.as_node_id())?;
                system.call_function(
                    RESOURCE_PACKAGE,
                    &object_info.blueprint_info.blueprint_id.blueprint_name,
                    PROOF_DROP_IDENT,
                    scrypto_encode(&ProofDropInput { proof }).unwrap(),
                )?;
            }

            // Drop the auth zone
            api.kernel_drop_node(&auth_zone_id)?;
        }

        Ok(())
    }

    fn on_substate_lock_fault<Y>(
        node_id: NodeId,
        _partition_num: PartitionNumber,
        _offset: &SubstateKey,
        api: &mut Y,
    ) -> Result<bool, RuntimeError>
    where
        Y: KernelApi<Self>,
    {
        let (blueprint_id, variant_id) = match node_id.entity_type() {
            Some(EntityType::GlobalVirtualSecp256k1Account) => (
                BlueprintId::new(&ACCOUNT_PACKAGE, ACCOUNT_BLUEPRINT),
                ACCOUNT_CREATE_VIRTUAL_SECP256K1_ID,
            ),
            Some(EntityType::GlobalVirtualEd25519Account) => (
                BlueprintId::new(&ACCOUNT_PACKAGE, ACCOUNT_BLUEPRINT),
                ACCOUNT_CREATE_VIRTUAL_ED25519_ID,
            ),
            Some(EntityType::GlobalVirtualSecp256k1Identity) => (
                BlueprintId::new(&IDENTITY_PACKAGE, IDENTITY_BLUEPRINT),
                IDENTITY_CREATE_VIRTUAL_SECP256K1_ID,
            ),
            Some(EntityType::GlobalVirtualEd25519Identity) => (
                BlueprintId::new(&IDENTITY_PACKAGE, IDENTITY_BLUEPRINT),
                IDENTITY_CREATE_VIRTUAL_ED25519_ID,
            ),
            _ => return Ok(false),
        };

        let mut service = SystemService::new(api);
        let definition = service.load_blueprint_definition(
            blueprint_id.package_address,
            &BlueprintVersionKey {
                blueprint: blueprint_id.blueprint_name.clone(),
                version: BlueprintVersion::default(),
            },
        )?;
        if definition
            .hook_exports
            .contains_key(&BlueprintHook::OnVirtualize)
        {
            let mut system = SystemService::new(api);
            let address = GlobalAddress::new_or_panic(node_id.into());
            let address_reservation =
                system.allocate_virtual_global_address(blueprint_id.clone(), address)?;

            api.kernel_invoke(Box::new(KernelInvocation {
                actor: Actor::BlueprintHook(BlueprintHookActor {
                    blueprint_id: blueprint_id.clone(),
                    hook: BlueprintHook::OnVirtualize,
                    receiver: None,
                }),
                args: IndexedScryptoValue::from_typed(&OnVirtualizeInput {
                    variant_id,
                    rid: copy_u8_array(&node_id.as_bytes()[1..]),
                    address_reservation,
                }),
            }))?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn on_drop_node<Y>(node_id: &NodeId, api: &mut Y) -> Result<(), RuntimeError>
    where
        Y: KernelApi<Self>,
    {
        let type_info = TypeInfoBlueprint::get_type(&node_id, api)?;

        match type_info {
            TypeInfoSubstate::Object(node_object_info) => {
                let mut service = SystemService::new(api);
                let definition = service.load_blueprint_definition(
                    node_object_info.blueprint_info.blueprint_id.package_address,
                    &BlueprintVersionKey {
                        blueprint: node_object_info
                            .blueprint_info
                            .blueprint_id
                            .blueprint_name
                            .clone(),
                        version: BlueprintVersion::default(),
                    },
                )?;
                if definition.hook_exports.contains_key(&BlueprintHook::OnDrop) {
                    api.kernel_invoke(Box::new(KernelInvocation {
                        actor: Actor::BlueprintHook(BlueprintHookActor {
                            blueprint_id: node_object_info.blueprint_info.blueprint_id.clone(),
                            hook: BlueprintHook::OnDrop,
                            receiver: Some(node_id.clone()),
                        }),
                        args: IndexedScryptoValue::from_typed(&OnDropInput {}),
                    }))
                    .map(|_| ())
                } else {
                    Ok(())
                }
            }
            TypeInfoSubstate::KeyValueStore(_)
            | TypeInfoSubstate::GlobalAddressReservation(_)
            | TypeInfoSubstate::GlobalAddressPhantom(_) => {
                // There is no way to drop a non-object through system API, triggering `NotAnObject` error.
                Ok(())
            }
        }
    }
}
