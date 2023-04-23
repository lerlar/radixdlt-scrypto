use radix_engine_interface::api::key_value_store_api::ClientKeyValueStoreApi;
use radix_engine_interface::api::substate_lock_api::LockFlags;
use radix_engine_interface::api::*;
use radix_engine_interface::data::scrypto::model::*;
use radix_engine_interface::data::scrypto::well_known_scrypto_custom_types::OWN_KEY_VALUE_STORE_ID;
use radix_engine_interface::data::scrypto::*;
use sbor::rust::marker::PhantomData;
use sbor::*;
use scrypto_schema::KeyValueStoreSchema;

use crate::engine::scrypto_env::ScryptoEnv;
use crate::runtime::{DataRef, DataRefMut, OriginalData};

// TODO: optimize `rust_value -> bytes -> scrypto_value` conversion.

/// A scalable key-value map which loads entries on demand.
pub struct KeyValueStore<
    K: ScryptoEncode + ScryptoDecode + ScryptoDescribe,
    V: ScryptoEncode + ScryptoDecode + ScryptoDescribe,
> {
    pub id: Own,
    pub key: PhantomData<K>,
    pub value: PhantomData<V>,
}

impl<
        K: ScryptoEncode + ScryptoDecode + ScryptoDescribe,
        V: ScryptoEncode + ScryptoDecode + ScryptoDescribe,
    > KeyValueStore<K, V>
{
    /// Creates a new key value store.
    pub fn new() -> Self {
        let mut env = ScryptoEnv;

        let schema = KeyValueStoreSchema::new::<K, V>(true);

        let id = env.new_key_value_store(schema).unwrap();

        Self {
            id: Own(id),
            key: PhantomData,
            value: PhantomData,
        }
    }

    /// Returns the value that is associated with the given key.
    pub fn get(&self, key: &K) -> Option<DataRef<V>> {
        let mut env = ScryptoEnv;
        let key_payload = scrypto_encode(key).unwrap();
        let handle = env
            .lock_key_value_store_entry(self.id.as_node_id(), &key_payload, LockFlags::read_only())
            .unwrap();
        let raw_bytes = env.sys_read_substate(handle).unwrap();

        // Decode and create Ref
        let substate: Option<ScryptoValue> = scrypto_decode(&raw_bytes).unwrap();
        match substate {
            Option::Some(value) => Some(DataRef::new(
                handle,
                scrypto_decode(&scrypto_encode(&value).unwrap()).unwrap(),
            )),
            Option::None => {
                env.sys_drop_lock(handle).unwrap();
                None
            }
        }
    }

    pub fn get_mut(&mut self, key: &K) -> Option<DataRefMut<V>> {
        let mut env = ScryptoEnv;
        let key_payload = scrypto_encode(key).unwrap();
        let handle = env
            .lock_key_value_store_entry(self.id.as_node_id(), &key_payload, LockFlags::MUTABLE)
            .unwrap();
        let raw_bytes = env.sys_read_substate(handle).unwrap();

        // Decode and create RefMut
        let substate: Option<ScryptoValue> = scrypto_decode(&raw_bytes).unwrap();
        match substate {
            Option::Some(value) => {
                let rust_value = scrypto_decode(&scrypto_encode(&value).unwrap()).unwrap();
                Some(DataRefMut::new(
                    handle,
                    OriginalData::KeyValueStoreEntry(value),
                    rust_value,
                ))
            }
            Option::None => {
                env.sys_drop_lock(handle).unwrap();
                None
            }
        }
    }

    /// Inserts a new key-value pair into this map.
    pub fn insert(&self, key: K, value: V) {
        let mut env = ScryptoEnv;
        let key_payload = scrypto_encode(&key).unwrap();
        let handle = env
            .lock_key_value_store_entry(self.id.as_node_id(), &key_payload, LockFlags::MUTABLE)
            .unwrap();
        let value_payload = scrypto_encode(&value).unwrap();

        let value: ScryptoValue = scrypto_decode(&value_payload).unwrap();
        let buffer = scrypto_encode(&Option::Some(value)).unwrap();

        env.key_value_entry_set(handle, buffer).unwrap();
        env.sys_drop_lock(handle).unwrap();
    }

    /// Remove an entry from the map and return the original value if it exists
    pub fn remove(&self, key: &K) -> Option<V> {
        let mut env = ScryptoEnv;
        let key_payload = scrypto_encode(&key).unwrap();
        let handle = env
            .lock_key_value_store_entry(self.id.as_node_id(), &key_payload, LockFlags::MUTABLE)
            .unwrap();

        let raw_bytes = env.key_value_entry_get(handle).unwrap();
        let value: Option<ScryptoValue> = scrypto_decode(&raw_bytes).unwrap();
        let rtn = value.map(|v| {
            let rust_value = scrypto_decode(&scrypto_encode(&v).unwrap()).unwrap();
            rust_value
        });

        let substate: Option<ScryptoValue> = None;
        env.sys_write_substate(handle, scrypto_encode(&substate).unwrap())
            .unwrap();
        env.sys_drop_lock(handle).unwrap();

        rtn
    }
}

//========
// binary
//========
impl<
        K: ScryptoEncode + ScryptoDecode + ScryptoDescribe,
        V: ScryptoEncode + ScryptoDecode + ScryptoDescribe,
    > Categorize<ScryptoCustomValueKind> for KeyValueStore<K, V>
{
    #[inline]
    fn value_kind() -> ValueKind<ScryptoCustomValueKind> {
        ValueKind::Custom(ScryptoCustomValueKind::Own)
    }
}

impl<
        K: ScryptoEncode + ScryptoDecode + ScryptoDescribe,
        V: ScryptoEncode + ScryptoDecode + ScryptoDescribe,
        E: Encoder<ScryptoCustomValueKind>,
    > Encode<ScryptoCustomValueKind, E> for KeyValueStore<K, V>
{
    #[inline]
    fn encode_value_kind(&self, encoder: &mut E) -> Result<(), EncodeError> {
        encoder.write_value_kind(Self::value_kind())
    }

    #[inline]
    fn encode_body(&self, encoder: &mut E) -> Result<(), EncodeError> {
        self.id.encode_body(encoder)
    }
}

impl<
        K: ScryptoEncode + ScryptoDecode + ScryptoDescribe,
        V: ScryptoEncode + ScryptoDecode + ScryptoDescribe,
        D: Decoder<ScryptoCustomValueKind>,
    > Decode<ScryptoCustomValueKind, D> for KeyValueStore<K, V>
{
    fn decode_body_with_value_kind(
        decoder: &mut D,
        value_kind: ValueKind<ScryptoCustomValueKind>,
    ) -> Result<Self, DecodeError> {
        let own = Own::decode_body_with_value_kind(decoder, value_kind)?;
        Ok(Self {
            id: own,
            key: PhantomData,
            value: PhantomData,
        })
    }
}

impl<
        K: ScryptoEncode + ScryptoDecode + ScryptoDescribe,
        V: ScryptoEncode + ScryptoDecode + ScryptoDescribe,
    > Describe<ScryptoCustomTypeKind> for KeyValueStore<K, V>
{
    const TYPE_ID: GlobalTypeId = GlobalTypeId::WellKnown([OWN_KEY_VALUE_STORE_ID]);
}
