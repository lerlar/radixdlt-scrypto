use radix_engine_interface::api::types::VaultId;
use radix_engine_interface::api::{
    ClientNodeApi, ClientStaticInvokeApi, ClientSubstateApi, Invokable,
};
use radix_engine_interface::blueprints::resource::*;
use radix_engine_interface::data::ScryptoDecode;
use radix_engine_interface::math::Decimal;
use sbor::rust::collections::BTreeSet;
use sbor::rust::fmt::Debug;

pub struct Vault(pub VaultId); // native stub

impl Vault {
    pub fn sys_new<Y, E: Debug + ScryptoDecode>(
        resource_address: ResourceAddress,
        api: &mut Y,
    ) -> Result<Self, E>
    where
        Y: ClientNodeApi<E> + ClientSubstateApi<E> + ClientStaticInvokeApi<E>,
    {
        let vault_id = api
            .invoke(ResourceManagerCreateVaultInvocation {
                receiver: resource_address,
            })?
            .vault_id();

        Ok(Self(vault_id))
    }

    pub fn sys_put<Y, E: Debug + ScryptoDecode>(
        &mut self,
        bucket: Bucket,
        api: &mut Y,
    ) -> Result<(), E>
    where
        Y: ClientNodeApi<E> + ClientSubstateApi<E> + ClientStaticInvokeApi<E>,
    {
        api.invoke(VaultPutInvocation {
            receiver: self.0,
            bucket,
        })
    }

    pub fn sys_take<Y, E: Debug + ScryptoDecode>(
        &mut self,
        amount: Decimal,
        api: &mut Y,
    ) -> Result<Bucket, E>
    where
        Y: ClientNodeApi<E> + ClientSubstateApi<E> + ClientStaticInvokeApi<E>,
    {
        api.invoke(VaultTakeInvocation {
            receiver: self.0,
            amount,
        })
    }

    pub fn sys_take_all<Y, E: Debug + ScryptoDecode>(&mut self, api: &mut Y) -> Result<Bucket, E>
    where
        Y: ClientNodeApi<E> + ClientSubstateApi<E> + ClientStaticInvokeApi<E>,
    {
        let amount = self.sys_amount(api)?;
        api.invoke(VaultTakeInvocation {
            receiver: self.0,
            amount,
        })
    }

    pub fn sys_take_ids<Y, E: Debug + ScryptoDecode>(
        &mut self,
        ids: BTreeSet<NonFungibleLocalId>,
        api: &mut Y,
    ) -> Result<Bucket, E>
    where
        Y: ClientNodeApi<E> + ClientSubstateApi<E> + ClientStaticInvokeApi<E>,
    {
        api.invoke(VaultTakeNonFungiblesInvocation {
            receiver: self.0,
            non_fungible_local_ids: ids,
        })
    }

    pub fn sys_amount<Y, E: Debug + ScryptoDecode>(&self, sys_calls: &mut Y) -> Result<Decimal, E>
    where
        Y: ClientNodeApi<E> + ClientSubstateApi<E> + Invokable<VaultGetAmountInvocation, E>,
    {
        sys_calls.invoke(VaultGetAmountInvocation { receiver: self.0 })
    }

    pub fn sys_create_proof<Y, E: Debug + ScryptoDecode>(
        &self,
        sys_calls: &mut Y,
    ) -> Result<Proof, E>
    where
        E: Debug + ScryptoDecode,
        Y: ClientNodeApi<E> + ClientSubstateApi<E> + ClientStaticInvokeApi<E>,
    {
        let amount = self.sys_amount(sys_calls)?;
        sys_calls.invoke(VaultCreateProofByAmountInvocation {
            receiver: self.0,
            amount,
        })
    }

    pub fn sys_create_proof_by_amount<Y, E: Debug + ScryptoDecode>(
        &self,
        sys_calls: &mut Y,
        amount: Decimal,
    ) -> Result<Proof, E>
    where
        Y: ClientNodeApi<E> + ClientSubstateApi<E> + ClientStaticInvokeApi<E>,
    {
        sys_calls.invoke(VaultCreateProofByAmountInvocation {
            receiver: self.0,
            amount,
        })
    }

    pub fn sys_create_proof_by_ids<Y, E: Debug + ScryptoDecode>(
        &self,
        sys_calls: &mut Y,
        ids: BTreeSet<NonFungibleLocalId>,
    ) -> Result<Proof, E>
    where
        Y: ClientNodeApi<E> + ClientSubstateApi<E> + ClientStaticInvokeApi<E>,
    {
        sys_calls.invoke(VaultCreateProofByIdsInvocation {
            receiver: self.0,
            ids,
        })
    }

    pub fn sys_lock_fee<Y, E: Debug + ScryptoDecode>(
        &mut self,
        sys_calls: &mut Y,
        amount: Decimal,
    ) -> Result<(), E>
    where
        Y: ClientNodeApi<E> + ClientSubstateApi<E> + ClientStaticInvokeApi<E>,
    {
        sys_calls.invoke(VaultLockFeeInvocation {
            receiver: self.0,
            amount,
            contingent: false,
        })
    }

    pub fn sys_lock_contingent_fee<Y, E: Debug + ScryptoDecode>(
        &mut self,
        sys_calls: &mut Y,
        amount: Decimal,
    ) -> Result<(), E>
    where
        Y: ClientNodeApi<E> + ClientSubstateApi<E> + ClientStaticInvokeApi<E>,
    {
        sys_calls.invoke(VaultLockFeeInvocation {
            receiver: self.0,
            amount,
            contingent: true,
        })
    }

    pub fn sys_resource_address<Y, E: Debug + ScryptoDecode>(
        &self,
        sys_calls: &mut Y,
    ) -> Result<ResourceAddress, E>
    where
        Y: ClientNodeApi<E> + ClientSubstateApi<E> + ClientStaticInvokeApi<E>,
    {
        sys_calls.invoke(VaultGetResourceAddressInvocation { receiver: self.0 })
    }
}
