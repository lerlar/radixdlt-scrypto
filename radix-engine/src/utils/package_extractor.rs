use radix_engine_interface::blueprints::package::{PackageDefinition};
use radix_engine_interface::schema::BlueprintSchema;
use radix_engine_interface::schema::PackageSchema;

use crate::errors::InvokeError;
use crate::system::system_modules::costing::SystemLoanFeeReserve;
use crate::types::*;
use crate::vm::wasm::*;
use crate::vm::NopWasmRuntime;
use sbor::rust::sync::Arc;

#[derive(Debug)]
pub enum ExtractSchemaError {
    InvalidWasm(PrepareError),
    RunSchemaGenError(InvokeError<WasmRuntimeError>),
    SchemaDecodeError(DecodeError),
}

impl From<PrepareError> for ExtractSchemaError {
    fn from(value: PrepareError) -> Self {
        ExtractSchemaError::InvalidWasm(value)
    }
}

pub fn extract_definition(code: &[u8]) -> Result<PackageDefinition, ExtractSchemaError> {
    let function_exports = WasmModule::init(code)
        .and_then(WasmModule::to_bytes)?
        .1
        .into_iter()
        .filter(|s| s.ends_with("_schema"));

    // Validate WASM
    let validator = WasmValidator::default();
    let instrumented_code = InstrumentedCode {
        metered_code_key: (
            PackageAddress::new_or_panic([EntityType::GlobalPackage as u8; NodeId::LENGTH]),
            validator.metering_config,
        ),
        code: Arc::new(
            validator
                .validate(&code, &PackageSchema::default())
                .map_err(|e| ExtractSchemaError::InvalidWasm(e))?
                .0,
        ),
    };

    // Execute with empty state (with default cost unit limit)
    let wasm_engine = DefaultWasmEngine::default();
    let fee_reserve = SystemLoanFeeReserve::default().with_free_credit(DEFAULT_FREE_CREDIT_IN_XRD);
    let mut runtime: Box<dyn WasmRuntime> = Box::new(NopWasmRuntime::new(fee_reserve));
    let mut instance = wasm_engine.instantiate(&instrumented_code);
    let mut blueprints = BTreeMap::new();
    let mut blueprints_function_auth = BTreeMap::new();
    for function_export in function_exports {
        let rtn = instance
            .invoke_export(&function_export, vec![], &mut runtime)
            .map_err(ExtractSchemaError::RunSchemaGenError)?;

        let name = function_export.replace("_schema", "").to_string();
        let (schema, function_auth): (BlueprintSchema, BTreeMap<String, AccessRule>) =
            scrypto_decode(rtn.as_slice()).map_err(ExtractSchemaError::SchemaDecodeError)?;

        blueprints.insert(name.clone(), schema);
        blueprints_function_auth.insert(name.clone(), function_auth);
    }

    Ok(PackageDefinition {
        schema: PackageSchema {
            blueprints
        },
        function_access_rules: blueprints_function_auth,
    })
}
