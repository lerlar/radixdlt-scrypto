use crate::errors::*;
use crate::kernel::actor::Actor;
use crate::kernel::call_frame::CallFrameUpdate;
use crate::kernel::kernel_api::KernelApi;
use crate::kernel::kernel_api::KernelInvocation;
use crate::system::module::SystemModule;
use crate::system::system_callback::{SystemConfig, SystemInvocation};
use crate::system::system_callback_api::SystemCallbackObject;
use crate::system::system_modules::auth::AuthModule;
use crate::system::system_modules::costing::CostingModule;
use crate::system::system_modules::costing::FeeTable;
use crate::system::system_modules::costing::SystemLoanFeeReserve;
use crate::system::system_modules::events::EventsModule;
use crate::system::system_modules::execution_trace::ExecutionTraceModule;
use crate::system::system_modules::kernel_trace::KernelTraceModule;
use crate::system::system_modules::logger::LoggerModule;
use crate::system::system_modules::node_move::NodeMoveModule;
use crate::system::system_modules::transaction_limits::{
    TransactionLimitsConfig, TransactionLimitsModule,
};
use crate::system::system_modules::transaction_runtime::TransactionRuntimeModule;
use crate::system::system_modules::virtualization::VirtualizationModule;
use crate::track::interface::NodeSubstates;
use crate::transaction::ExecutionConfig;
use crate::types::*;
use bitflags::bitflags;
use radix_engine_interface::api::field_lock_api::LockFlags;
use radix_engine_interface::crypto::Hash;
use resources_tracker_macro::trace_resources;
use transaction::model::AuthZoneParams;

bitflags! {
    pub struct EnabledModules: u32 {
        const KERNEL_DEBUG = 0x1 << 0;
        const COSTING = 0x01 << 1;
        const NODE_MOVE = 0x01 << 2;
        const AUTH = 0x01 << 3;
        const LOGGER = 0x01 << 4;
        const TRANSACTION_RUNTIME = 0x01 << 5;
        const EXECUTION_TRACE = 0x01 << 6;
        const TRANSACTION_LIMITS = 0x01 << 7;
        const EVENTS = 0x01 << 8;
    }
}

pub struct SystemModuleMixer {
    /* flags */
    pub enabled_modules: EnabledModules,

    /* states */
    pub kernel_debug: KernelTraceModule,
    pub costing: CostingModule,
    pub node_move: NodeMoveModule,
    pub auth: AuthModule,
    pub logger: LoggerModule,
    pub transaction_runtime: TransactionRuntimeModule,
    pub execution_trace: ExecutionTraceModule,
    pub transaction_limits: TransactionLimitsModule,
    pub events: EventsModule,
    pub virtualization: VirtualizationModule,
}

impl SystemModuleMixer {
    pub fn standard(
        tx_hash: Hash,
        auth_zone_params: AuthZoneParams,
        fee_reserve: SystemLoanFeeReserve,
        fee_table: FeeTable,
        payload_len: usize,
        num_of_signatures: usize,
        execution_config: &ExecutionConfig,
    ) -> Self {
        let mut modules = EnabledModules::empty();

        if execution_config.kernel_trace {
            modules |= EnabledModules::KERNEL_DEBUG
        };

        if execution_config.execution_trace.is_some() {
            modules |= EnabledModules::EXECUTION_TRACE;
        }

        if !execution_config.genesis {
            modules |= EnabledModules::COSTING;
            modules |= EnabledModules::AUTH;
            modules |= EnabledModules::TRANSACTION_LIMITS;
        }

        modules |= EnabledModules::NODE_MOVE;
        modules |= EnabledModules::LOGGER;
        modules |= EnabledModules::TRANSACTION_RUNTIME;
        modules |= EnabledModules::EVENTS;

        Self {
            enabled_modules: modules,
            kernel_debug: KernelTraceModule {},
            costing: CostingModule {
                fee_reserve,
                fee_table,
                max_call_depth: execution_config.max_call_depth,
                payload_len,
                num_of_signatures,
            },
            node_move: NodeMoveModule {},
            auth: AuthModule {
                params: auth_zone_params.clone(),
                auth_zone_stack: Vec::new(),
            },
            logger: LoggerModule::default(),
            transaction_runtime: TransactionRuntimeModule {
                tx_hash,
                next_id: 0,
            },
            transaction_limits: TransactionLimitsModule::new(TransactionLimitsConfig {
                max_wasm_memory: execution_config.max_wasm_mem_per_transaction,
                max_wasm_memory_per_call_frame: execution_config.max_wasm_mem_per_call_frame,
                max_substate_read_count: execution_config.max_substate_reads_per_transaction,
                max_substate_write_count: execution_config.max_substate_writes_per_transaction,
                max_substate_size: execution_config.max_substate_size,
                max_invoke_payload_size: execution_config.max_invoke_input_size,
            }),
            execution_trace: ExecutionTraceModule::new(
                execution_config.execution_trace.unwrap_or(0),
            ),
            events: EventsModule::default(),
            virtualization: VirtualizationModule,
        }
    }
}

//====================================================================
// NOTE: Modules are applied in the reverse order of initialization!
//====================================================================

impl<V: SystemCallbackObject> SystemModule<SystemConfig<V>> for SystemModuleMixer {
    #[trace_resources]
    fn on_init<Y: KernelApi<SystemConfig<V>>>(api: &mut Y) -> Result<(), RuntimeError> {
        let modules: EnabledModules = api.kernel_get_system().modules.enabled_modules;

        // Enable transaction limits
        if modules.contains(EnabledModules::TRANSACTION_LIMITS) {
            TransactionLimitsModule::on_init(api)?;
        }

        // Enable execution trace
        if modules.contains(EnabledModules::KERNEL_DEBUG) {
            ExecutionTraceModule::on_init(api)?;
        }

        // Enable transaction runtime
        if modules.contains(EnabledModules::TRANSACTION_RUNTIME) {
            TransactionRuntimeModule::on_init(api)?;
        }

        // Enable logger
        if modules.contains(EnabledModules::LOGGER) {
            LoggerModule::on_init(api)?;
        }

        // Enable auth
        if modules.contains(EnabledModules::AUTH) {
            AuthModule::on_init(api)?;
        }

        // Enable node move
        if modules.contains(EnabledModules::NODE_MOVE) {
            NodeMoveModule::on_init(api)?;
        }

        // Enable costing
        if modules.contains(EnabledModules::COSTING) {
            CostingModule::on_init(api)?;
        }

        // Enable debug
        if modules.contains(EnabledModules::KERNEL_DEBUG) {
            KernelTraceModule::on_init(api)?;
        }

        // Enable events
        if modules.contains(EnabledModules::EVENTS) {
            EventsModule::on_init(api)?;
        }

        Ok(())
    }

    #[trace_resources]
    fn on_teardown<Y: KernelApi<SystemConfig<V>>>(api: &mut Y) -> Result<(), RuntimeError> {
        let modules: EnabledModules = api.kernel_get_system().modules.enabled_modules;
        if modules.contains(EnabledModules::KERNEL_DEBUG) {
            KernelTraceModule::on_teardown(api)?;
        }
        if modules.contains(EnabledModules::COSTING) {
            CostingModule::on_teardown(api)?;
        }
        if modules.contains(EnabledModules::NODE_MOVE) {
            NodeMoveModule::on_teardown(api)?;
        }
        if modules.contains(EnabledModules::AUTH) {
            AuthModule::on_teardown(api)?;
        }
        if modules.contains(EnabledModules::LOGGER) {
            LoggerModule::on_teardown(api)?;
        }
        if modules.contains(EnabledModules::TRANSACTION_RUNTIME) {
            TransactionRuntimeModule::on_teardown(api)?;
        }
        if modules.contains(EnabledModules::EXECUTION_TRACE) {
            ExecutionTraceModule::on_teardown(api)?;
        }
        if modules.contains(EnabledModules::TRANSACTION_LIMITS) {
            TransactionLimitsModule::on_teardown(api)?;
        }
        if modules.contains(EnabledModules::EVENTS) {
            EventsModule::on_teardown(api)?;
        }
        Ok(())
    }

    #[trace_resources(log=input_size, log={&*identifier.sys_invocation.blueprint.blueprint_name}, log=identifier.sys_invocation.ident.to_debug_string(), log={format!("{:?}", identifier.sys_invocation.receiver)})]
    fn before_invoke<Y: KernelApi<SystemConfig<V>>>(
        api: &mut Y,
        identifier: &KernelInvocation<SystemInvocation>,
        input_size: usize,
    ) -> Result<(), RuntimeError> {
        let modules: EnabledModules = api.kernel_get_system().modules.enabled_modules;
        if modules.contains(EnabledModules::KERNEL_DEBUG) {
            KernelTraceModule::before_invoke(api, identifier, input_size)?;
        }
        if modules.contains(EnabledModules::COSTING) {
            CostingModule::before_invoke(api, identifier, input_size)?;
        }
        if modules.contains(EnabledModules::NODE_MOVE) {
            NodeMoveModule::before_invoke(api, identifier, input_size)?;
        }
        if modules.contains(EnabledModules::AUTH) {
            AuthModule::before_invoke(api, identifier, input_size)?;
        }
        if modules.contains(EnabledModules::LOGGER) {
            LoggerModule::before_invoke(api, identifier, input_size)?;
        }
        if modules.contains(EnabledModules::TRANSACTION_RUNTIME) {
            TransactionRuntimeModule::before_invoke(api, identifier, input_size)?;
        }
        if modules.contains(EnabledModules::EXECUTION_TRACE) {
            ExecutionTraceModule::before_invoke(api, identifier, input_size)?;
        }
        if modules.contains(EnabledModules::TRANSACTION_LIMITS) {
            TransactionLimitsModule::before_invoke(api, identifier, input_size)?;
        }
        if modules.contains(EnabledModules::EVENTS) {
            EventsModule::before_invoke(api, identifier, input_size)?;
        }
        Ok(())
    }

    #[trace_resources]
    fn before_push_frame<Y: KernelApi<SystemConfig<V>>>(
        api: &mut Y,
        callee: &Actor,
        update: &mut CallFrameUpdate,
        args: &IndexedScryptoValue,
    ) -> Result<(), RuntimeError> {
        let modules: EnabledModules = api.kernel_get_system().modules.enabled_modules;
        if modules.contains(EnabledModules::KERNEL_DEBUG) {
            KernelTraceModule::before_push_frame(api, callee, update, args)?;
        }
        if modules.contains(EnabledModules::COSTING) {
            CostingModule::before_push_frame(api, callee, update, args)?;
        }
        if modules.contains(EnabledModules::NODE_MOVE) {
            NodeMoveModule::before_push_frame(api, callee, update, args)?;
        }
        if modules.contains(EnabledModules::AUTH) {
            AuthModule::before_push_frame(api, callee, update, args)?;
        }
        if modules.contains(EnabledModules::LOGGER) {
            LoggerModule::before_push_frame(api, callee, update, args)?;
        }
        if modules.contains(EnabledModules::TRANSACTION_RUNTIME) {
            TransactionRuntimeModule::before_push_frame(api, callee, update, args)?;
        }
        if modules.contains(EnabledModules::EXECUTION_TRACE) {
            ExecutionTraceModule::before_push_frame(api, callee, update, args)?;
        }
        if modules.contains(EnabledModules::TRANSACTION_LIMITS) {
            TransactionLimitsModule::before_push_frame(api, callee, update, args)?;
        }
        if modules.contains(EnabledModules::EVENTS) {
            EventsModule::before_push_frame(api, callee, update, args)?;
        }
        Ok(())
    }

    #[trace_resources]
    fn on_execution_start<Y: KernelApi<SystemConfig<V>>>(api: &mut Y) -> Result<(), RuntimeError> {
        let modules: EnabledModules = api.kernel_get_system().modules.enabled_modules;
        if modules.contains(EnabledModules::KERNEL_DEBUG) {
            KernelTraceModule::on_execution_start(api)?;
        }
        if modules.contains(EnabledModules::COSTING) {
            CostingModule::on_execution_start(api)?;
        }
        if modules.contains(EnabledModules::NODE_MOVE) {
            NodeMoveModule::on_execution_start(api)?;
        }
        if modules.contains(EnabledModules::AUTH) {
            AuthModule::on_execution_start(api)?;
        }
        if modules.contains(EnabledModules::LOGGER) {
            LoggerModule::on_execution_start(api)?;
        }
        if modules.contains(EnabledModules::TRANSACTION_RUNTIME) {
            TransactionRuntimeModule::on_execution_start(api)?;
        }
        if modules.contains(EnabledModules::EXECUTION_TRACE) {
            ExecutionTraceModule::on_execution_start(api)?;
        }
        if modules.contains(EnabledModules::TRANSACTION_LIMITS) {
            TransactionLimitsModule::on_execution_start(api)?;
        }
        if modules.contains(EnabledModules::EVENTS) {
            EventsModule::on_execution_start(api)?;
        }
        Ok(())
    }

    #[trace_resources]
    fn on_execution_finish<Y: KernelApi<SystemConfig<V>>>(
        api: &mut Y,
        update: &CallFrameUpdate,
    ) -> Result<(), RuntimeError> {
        let modules: EnabledModules = api.kernel_get_system().modules.enabled_modules;
        if modules.contains(EnabledModules::KERNEL_DEBUG) {
            KernelTraceModule::on_execution_finish(api, update)?;
        }
        if modules.contains(EnabledModules::COSTING) {
            CostingModule::on_execution_finish(api, update)?;
        }
        if modules.contains(EnabledModules::NODE_MOVE) {
            NodeMoveModule::on_execution_finish(api, update)?;
        }
        if modules.contains(EnabledModules::AUTH) {
            AuthModule::on_execution_finish(api, update)?;
        }
        if modules.contains(EnabledModules::LOGGER) {
            LoggerModule::on_execution_finish(api, update)?;
        }
        if modules.contains(EnabledModules::TRANSACTION_RUNTIME) {
            TransactionRuntimeModule::on_execution_finish(api, update)?;
        }
        if modules.contains(EnabledModules::EXECUTION_TRACE) {
            ExecutionTraceModule::on_execution_finish(api, update)?;
        }
        if modules.contains(EnabledModules::TRANSACTION_LIMITS) {
            TransactionLimitsModule::on_execution_finish(api, update)?;
        }
        if modules.contains(EnabledModules::EVENTS) {
            EventsModule::on_execution_finish(api, update)?;
        }
        Ok(())
    }

    #[trace_resources]
    fn after_pop_frame<Y: KernelApi<SystemConfig<V>>>(api: &mut Y) -> Result<(), RuntimeError> {
        let modules: EnabledModules = api.kernel_get_system().modules.enabled_modules;
        if modules.contains(EnabledModules::KERNEL_DEBUG) {
            KernelTraceModule::after_pop_frame(api)?;
        }
        if modules.contains(EnabledModules::COSTING) {
            CostingModule::after_pop_frame(api)?;
        }
        if modules.contains(EnabledModules::NODE_MOVE) {
            NodeMoveModule::after_pop_frame(api)?;
        }
        if modules.contains(EnabledModules::AUTH) {
            AuthModule::after_pop_frame(api)?;
        }
        if modules.contains(EnabledModules::LOGGER) {
            LoggerModule::after_pop_frame(api)?;
        }
        if modules.contains(EnabledModules::TRANSACTION_RUNTIME) {
            TransactionRuntimeModule::after_pop_frame(api)?;
        }
        if modules.contains(EnabledModules::EXECUTION_TRACE) {
            ExecutionTraceModule::after_pop_frame(api)?;
        }
        if modules.contains(EnabledModules::TRANSACTION_LIMITS) {
            TransactionLimitsModule::after_pop_frame(api)?;
        }
        if modules.contains(EnabledModules::EVENTS) {
            EventsModule::after_pop_frame(api)?;
        }
        Ok(())
    }

    #[trace_resources(log=output_size)]
    fn after_invoke<Y: KernelApi<SystemConfig<V>>>(
        api: &mut Y,
        output_size: usize,
    ) -> Result<(), RuntimeError> {
        let modules: EnabledModules = api.kernel_get_system().modules.enabled_modules;
        if modules.contains(EnabledModules::KERNEL_DEBUG) {
            KernelTraceModule::after_invoke(api, output_size)?;
        }
        if modules.contains(EnabledModules::COSTING) {
            CostingModule::after_invoke(api, output_size)?;
        }
        if modules.contains(EnabledModules::NODE_MOVE) {
            NodeMoveModule::after_invoke(api, output_size)?;
        }
        if modules.contains(EnabledModules::AUTH) {
            AuthModule::after_invoke(api, output_size)?;
        }
        if modules.contains(EnabledModules::LOGGER) {
            LoggerModule::after_invoke(api, output_size)?;
        }
        if modules.contains(EnabledModules::TRANSACTION_RUNTIME) {
            TransactionRuntimeModule::after_invoke(api, output_size)?;
        }
        if modules.contains(EnabledModules::EXECUTION_TRACE) {
            ExecutionTraceModule::after_invoke(api, output_size)?;
        }
        if modules.contains(EnabledModules::TRANSACTION_LIMITS) {
            TransactionLimitsModule::after_invoke(api, output_size)?;
        }
        if modules.contains(EnabledModules::EVENTS) {
            EventsModule::after_invoke(api, output_size)?;
        }
        Ok(())
    }

    #[trace_resources(log=entity_type)]
    fn on_allocate_node_id<Y: KernelApi<SystemConfig<V>>>(
        api: &mut Y,
        entity_type: Option<EntityType>,
        virtual_node: bool,
    ) -> Result<(), RuntimeError> {
        let modules: EnabledModules = api.kernel_get_system().modules.enabled_modules;
        if modules.contains(EnabledModules::KERNEL_DEBUG) {
            KernelTraceModule::on_allocate_node_id(api, entity_type, virtual_node)?;
        }
        if modules.contains(EnabledModules::COSTING) {
            CostingModule::on_allocate_node_id(api, entity_type, virtual_node)?;
        }
        if modules.contains(EnabledModules::NODE_MOVE) {
            NodeMoveModule::on_allocate_node_id(api, entity_type, virtual_node)?;
        }
        if modules.contains(EnabledModules::AUTH) {
            AuthModule::on_allocate_node_id(api, entity_type, virtual_node)?;
        }
        if modules.contains(EnabledModules::LOGGER) {
            LoggerModule::on_allocate_node_id(api, entity_type, virtual_node)?;
        }
        if modules.contains(EnabledModules::TRANSACTION_RUNTIME) {
            TransactionRuntimeModule::on_allocate_node_id(api, entity_type, virtual_node)?;
        }
        if modules.contains(EnabledModules::EXECUTION_TRACE) {
            ExecutionTraceModule::on_allocate_node_id(api, entity_type, virtual_node)?;
        }
        if modules.contains(EnabledModules::TRANSACTION_LIMITS) {
            TransactionLimitsModule::on_allocate_node_id(api, entity_type, virtual_node)?;
        }
        if modules.contains(EnabledModules::EVENTS) {
            EventsModule::on_allocate_node_id(api, entity_type, virtual_node)?;
        }
        Ok(())
    }

    #[trace_resources]
    fn before_create_node<Y: KernelApi<SystemConfig<V>>>(
        api: &mut Y,
        node_id: &NodeId,
        node_substates: &NodeSubstates,
    ) -> Result<(), RuntimeError> {
        let modules: EnabledModules = api.kernel_get_system().modules.enabled_modules;
        if modules.contains(EnabledModules::KERNEL_DEBUG) {
            KernelTraceModule::before_create_node(api, node_id, node_substates)?;
        }
        if modules.contains(EnabledModules::COSTING) {
            CostingModule::before_create_node(api, node_id, node_substates)?;
        }
        if modules.contains(EnabledModules::NODE_MOVE) {
            NodeMoveModule::before_create_node(api, node_id, node_substates)?;
        }
        if modules.contains(EnabledModules::AUTH) {
            AuthModule::before_create_node(api, node_id, node_substates)?;
        }
        if modules.contains(EnabledModules::LOGGER) {
            LoggerModule::before_create_node(api, node_id, node_substates)?;
        }
        if modules.contains(EnabledModules::TRANSACTION_RUNTIME) {
            TransactionRuntimeModule::before_create_node(api, node_id, node_substates)?;
        }
        if modules.contains(EnabledModules::EXECUTION_TRACE) {
            ExecutionTraceModule::before_create_node(api, node_id, node_substates)?;
        }
        if modules.contains(EnabledModules::TRANSACTION_LIMITS) {
            TransactionLimitsModule::before_create_node(api, node_id, node_substates)?;
        }
        if modules.contains(EnabledModules::EVENTS) {
            EventsModule::before_create_node(api, node_id, node_substates)?;
        }
        Ok(())
    }

    #[trace_resources]
    fn after_create_node<Y: KernelApi<SystemConfig<V>>>(
        api: &mut Y,
        node_id: &NodeId,
    ) -> Result<(), RuntimeError> {
        let modules: EnabledModules = api.kernel_get_system().modules.enabled_modules;
        if modules.contains(EnabledModules::KERNEL_DEBUG) {
            KernelTraceModule::after_create_node(api, node_id)?;
        }
        if modules.contains(EnabledModules::COSTING) {
            CostingModule::after_create_node(api, node_id)?;
        }
        if modules.contains(EnabledModules::NODE_MOVE) {
            NodeMoveModule::after_create_node(api, node_id)?;
        }
        if modules.contains(EnabledModules::AUTH) {
            AuthModule::after_create_node(api, node_id)?;
        }
        if modules.contains(EnabledModules::LOGGER) {
            LoggerModule::after_create_node(api, node_id)?;
        }
        if modules.contains(EnabledModules::TRANSACTION_RUNTIME) {
            TransactionRuntimeModule::after_create_node(api, node_id)?;
        }
        if modules.contains(EnabledModules::EXECUTION_TRACE) {
            ExecutionTraceModule::after_create_node(api, node_id)?;
        }
        if modules.contains(EnabledModules::TRANSACTION_LIMITS) {
            TransactionLimitsModule::after_create_node(api, node_id)?;
        }
        if modules.contains(EnabledModules::EVENTS) {
            EventsModule::after_create_node(api, node_id)?;
        }
        Ok(())
    }

    #[trace_resources]
    fn before_drop_node<Y: KernelApi<SystemConfig<V>>>(
        api: &mut Y,
        node_id: &NodeId,
    ) -> Result<(), RuntimeError> {
        let modules: EnabledModules = api.kernel_get_system().modules.enabled_modules;
        if modules.contains(EnabledModules::KERNEL_DEBUG) {
            KernelTraceModule::before_drop_node(api, node_id)?;
        }
        if modules.contains(EnabledModules::COSTING) {
            CostingModule::before_drop_node(api, node_id)?;
        }
        if modules.contains(EnabledModules::NODE_MOVE) {
            NodeMoveModule::before_drop_node(api, node_id)?;
        }
        if modules.contains(EnabledModules::AUTH) {
            AuthModule::before_drop_node(api, node_id)?;
        }
        if modules.contains(EnabledModules::LOGGER) {
            LoggerModule::before_drop_node(api, node_id)?;
        }
        if modules.contains(EnabledModules::TRANSACTION_RUNTIME) {
            TransactionRuntimeModule::before_drop_node(api, node_id)?;
        }
        if modules.contains(EnabledModules::EXECUTION_TRACE) {
            ExecutionTraceModule::before_drop_node(api, node_id)?;
        }
        if modules.contains(EnabledModules::TRANSACTION_LIMITS) {
            TransactionLimitsModule::before_drop_node(api, node_id)?;
        }
        if modules.contains(EnabledModules::EVENTS) {
            EventsModule::before_drop_node(api, node_id)?;
        }
        Ok(())
    }

    #[trace_resources]
    fn after_drop_node<Y: KernelApi<SystemConfig<V>>>(api: &mut Y) -> Result<(), RuntimeError> {
        let modules: EnabledModules = api.kernel_get_system().modules.enabled_modules;
        if modules.contains(EnabledModules::KERNEL_DEBUG) {
            KernelTraceModule::after_drop_node(api)?;
        }
        if modules.contains(EnabledModules::COSTING) {
            CostingModule::after_drop_node(api)?;
        }
        if modules.contains(EnabledModules::NODE_MOVE) {
            NodeMoveModule::after_drop_node(api)?;
        }
        if modules.contains(EnabledModules::AUTH) {
            AuthModule::after_drop_node(api)?;
        }
        if modules.contains(EnabledModules::LOGGER) {
            LoggerModule::after_drop_node(api)?;
        }
        if modules.contains(EnabledModules::TRANSACTION_RUNTIME) {
            TransactionRuntimeModule::after_drop_node(api)?;
        }
        if modules.contains(EnabledModules::EXECUTION_TRACE) {
            ExecutionTraceModule::after_drop_node(api)?;
        }
        if modules.contains(EnabledModules::TRANSACTION_LIMITS) {
            TransactionLimitsModule::after_drop_node(api)?;
        }
        if modules.contains(EnabledModules::EVENTS) {
            EventsModule::after_drop_node(api)?;
        }
        Ok(())
    }

    #[trace_resources]
    fn before_lock_substate<Y: KernelApi<SystemConfig<V>>>(
        api: &mut Y,
        node_id: &NodeId,
        module_id: &ModuleNumber,
        substate_key: &SubstateKey,
        flags: &LockFlags,
    ) -> Result<(), RuntimeError> {
        let modules: EnabledModules = api.kernel_get_system().modules.enabled_modules;
        if modules.contains(EnabledModules::KERNEL_DEBUG) {
            KernelTraceModule::before_lock_substate(api, node_id, module_id, substate_key, flags)?;
        }
        if modules.contains(EnabledModules::COSTING) {
            CostingModule::before_lock_substate(api, node_id, module_id, substate_key, flags)?;
        }
        if modules.contains(EnabledModules::NODE_MOVE) {
            NodeMoveModule::before_lock_substate(api, node_id, module_id, substate_key, flags)?;
        }
        if modules.contains(EnabledModules::AUTH) {
            AuthModule::before_lock_substate(api, node_id, module_id, substate_key, flags)?;
        }
        if modules.contains(EnabledModules::LOGGER) {
            LoggerModule::before_lock_substate(api, node_id, module_id, substate_key, flags)?;
        }
        if modules.contains(EnabledModules::TRANSACTION_RUNTIME) {
            TransactionRuntimeModule::before_lock_substate(
                api,
                node_id,
                module_id,
                substate_key,
                flags,
            )?;
        }
        if modules.contains(EnabledModules::EXECUTION_TRACE) {
            ExecutionTraceModule::before_lock_substate(
                api,
                node_id,
                module_id,
                substate_key,
                flags,
            )?;
        }
        if modules.contains(EnabledModules::TRANSACTION_LIMITS) {
            TransactionLimitsModule::before_lock_substate(
                api,
                node_id,
                module_id,
                substate_key,
                flags,
            )?;
        }
        if modules.contains(EnabledModules::EVENTS) {
            EventsModule::before_lock_substate(api, node_id, module_id, substate_key, flags)?;
        }
        Ok(())
    }

    #[trace_resources(log=size)]
    fn after_lock_substate<Y: KernelApi<SystemConfig<V>>>(
        api: &mut Y,
        handle: LockHandle,
        first_lock_from_db: bool,
        size: usize,
    ) -> Result<(), RuntimeError> {
        let modules: EnabledModules = api.kernel_get_system().modules.enabled_modules;
        if modules.contains(EnabledModules::KERNEL_DEBUG) {
            KernelTraceModule::after_lock_substate(api, handle, first_lock_from_db, size)?;
        }
        if modules.contains(EnabledModules::COSTING) {
            CostingModule::after_lock_substate(api, handle, first_lock_from_db, size)?;
        }
        if modules.contains(EnabledModules::NODE_MOVE) {
            NodeMoveModule::after_lock_substate(api, handle, first_lock_from_db, size)?;
        }
        if modules.contains(EnabledModules::AUTH) {
            AuthModule::after_lock_substate(api, handle, first_lock_from_db, size)?;
        }
        if modules.contains(EnabledModules::LOGGER) {
            LoggerModule::after_lock_substate(api, handle, first_lock_from_db, size)?;
        }
        if modules.contains(EnabledModules::TRANSACTION_RUNTIME) {
            TransactionRuntimeModule::after_lock_substate(api, handle, first_lock_from_db, size)?;
        }
        if modules.contains(EnabledModules::EXECUTION_TRACE) {
            ExecutionTraceModule::after_lock_substate(api, handle, first_lock_from_db, size)?;
        }
        if modules.contains(EnabledModules::TRANSACTION_LIMITS) {
            TransactionLimitsModule::after_lock_substate(api, handle, first_lock_from_db, size)?;
        }
        if modules.contains(EnabledModules::EVENTS) {
            EventsModule::after_lock_substate(api, handle, first_lock_from_db, size)?;
        }
        Ok(())
    }

    #[trace_resources(log=size)]
    fn on_read_substate<Y: KernelApi<SystemConfig<V>>>(
        api: &mut Y,
        lock_handle: LockHandle,
        size: usize,
    ) -> Result<(), RuntimeError> {
        let modules: EnabledModules = api.kernel_get_system().modules.enabled_modules;
        if modules.contains(EnabledModules::KERNEL_DEBUG) {
            KernelTraceModule::on_read_substate(api, lock_handle, size)?;
        }
        if modules.contains(EnabledModules::COSTING) {
            CostingModule::on_read_substate(api, lock_handle, size)?;
        }
        if modules.contains(EnabledModules::NODE_MOVE) {
            NodeMoveModule::on_read_substate(api, lock_handle, size)?;
        }
        if modules.contains(EnabledModules::AUTH) {
            AuthModule::on_read_substate(api, lock_handle, size)?;
        }
        if modules.contains(EnabledModules::LOGGER) {
            LoggerModule::on_read_substate(api, lock_handle, size)?;
        }
        if modules.contains(EnabledModules::TRANSACTION_RUNTIME) {
            TransactionRuntimeModule::on_read_substate(api, lock_handle, size)?;
        }
        if modules.contains(EnabledModules::EXECUTION_TRACE) {
            ExecutionTraceModule::on_read_substate(api, lock_handle, size)?;
        }
        if modules.contains(EnabledModules::TRANSACTION_LIMITS) {
            TransactionLimitsModule::on_read_substate(api, lock_handle, size)?;
        }
        if modules.contains(EnabledModules::EVENTS) {
            EventsModule::on_read_substate(api, lock_handle, size)?;
        }
        Ok(())
    }

    #[trace_resources(log=size)]
    fn on_write_substate<Y: KernelApi<SystemConfig<V>>>(
        api: &mut Y,
        lock_handle: LockHandle,
        size: usize,
    ) -> Result<(), RuntimeError> {
        let modules: EnabledModules = api.kernel_get_system().modules.enabled_modules;
        if modules.contains(EnabledModules::KERNEL_DEBUG) {
            KernelTraceModule::on_write_substate(api, lock_handle, size)?;
        }
        if modules.contains(EnabledModules::COSTING) {
            CostingModule::on_write_substate(api, lock_handle, size)?;
        }
        if modules.contains(EnabledModules::NODE_MOVE) {
            NodeMoveModule::on_write_substate(api, lock_handle, size)?;
        }
        if modules.contains(EnabledModules::AUTH) {
            AuthModule::on_write_substate(api, lock_handle, size)?;
        }
        if modules.contains(EnabledModules::LOGGER) {
            LoggerModule::on_write_substate(api, lock_handle, size)?;
        }
        if modules.contains(EnabledModules::TRANSACTION_RUNTIME) {
            TransactionRuntimeModule::on_write_substate(api, lock_handle, size)?;
        }
        if modules.contains(EnabledModules::EXECUTION_TRACE) {
            ExecutionTraceModule::on_write_substate(api, lock_handle, size)?;
        }
        if modules.contains(EnabledModules::TRANSACTION_LIMITS) {
            TransactionLimitsModule::on_write_substate(api, lock_handle, size)?;
        }
        if modules.contains(EnabledModules::EVENTS) {
            EventsModule::on_write_substate(api, lock_handle, size)?;
        }
        Ok(())
    }

    #[trace_resources]
    fn on_drop_lock<Y: KernelApi<SystemConfig<V>>>(
        api: &mut Y,
        lock_handle: LockHandle,
    ) -> Result<(), RuntimeError> {
        let modules: EnabledModules = api.kernel_get_system().modules.enabled_modules;
        if modules.contains(EnabledModules::KERNEL_DEBUG) {
            KernelTraceModule::on_drop_lock(api, lock_handle)?;
        }
        if modules.contains(EnabledModules::COSTING) {
            CostingModule::on_drop_lock(api, lock_handle)?;
        }
        if modules.contains(EnabledModules::NODE_MOVE) {
            NodeMoveModule::on_drop_lock(api, lock_handle)?;
        }
        if modules.contains(EnabledModules::AUTH) {
            AuthModule::on_drop_lock(api, lock_handle)?;
        }
        if modules.contains(EnabledModules::LOGGER) {
            LoggerModule::on_drop_lock(api, lock_handle)?;
        }
        if modules.contains(EnabledModules::TRANSACTION_RUNTIME) {
            TransactionRuntimeModule::on_drop_lock(api, lock_handle)?;
        }
        if modules.contains(EnabledModules::EXECUTION_TRACE) {
            ExecutionTraceModule::on_drop_lock(api, lock_handle)?;
        }
        if modules.contains(EnabledModules::TRANSACTION_LIMITS) {
            TransactionLimitsModule::on_drop_lock(api, lock_handle)?;
        }
        if modules.contains(EnabledModules::EVENTS) {
            EventsModule::on_drop_lock(api, lock_handle)?;
        }
        Ok(())
    }
}
