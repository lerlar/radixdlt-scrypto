use sbor::*;
use scrypto::engine::types::*;
use scrypto::rust::collections::BTreeSet;
use scrypto::rust::rc::Rc;

use crate::model::Bucket;
use crate::model::{ResourceAmount, ResourceContainer, ResourceError};

/// Represents an error when accessing a vault.
#[derive(Debug, Clone)]
pub enum VaultError {
    ResourceError(ResourceError),
    VaultLocked,
    OtherBucketLocked,
}

/// A persistent resource container.
#[derive(Debug, TypeId, Encode, Decode)]
pub struct Vault {
    container: Rc<ResourceContainer>,
}

impl Vault {
    pub fn new(container: ResourceContainer) -> Self {
        Self {
            container: Rc::new(container),
        }
    }

    pub fn put(&mut self, other: Bucket) -> Result<(), VaultError> {
        let this_container = self.borrow_container()?;
        let other_container = other
            .own_container()
            .map_err(|_| VaultError::OtherBucketLocked)?;

        this_container
            .put(other_container)
            .map_err(VaultError::ResourceError)
    }

    pub fn take(&mut self, amount: Decimal) -> Result<Bucket, VaultError> {
        let this_container = self.borrow_container()?;

        Ok(Bucket::new(
            this_container
                .take(amount)
                .map_err(VaultError::ResourceError)?,
        ))
    }

    pub fn take_non_fungible(&mut self, key: &NonFungibleId) -> Result<Bucket, VaultError> {
        self.take_non_fungibles(&BTreeSet::from([key.clone()]))
    }

    pub fn take_non_fungibles(
        &mut self,
        ids: &BTreeSet<NonFungibleId>,
    ) -> Result<Bucket, VaultError> {
        let this_container = self.borrow_container()?;

        Ok(Bucket::new(
            this_container
                .take_non_fungibles(ids)
                .map_err(VaultError::ResourceError)?,
        ))
    }

    pub fn liquid_amount(&self) -> ResourceAmount {
        self.container.liquid_amount()
    }

    pub fn resource_def_id(&self) -> ResourceDefId {
        self.container.resource_def_id()
    }

    pub fn resource_type(&self) -> ResourceType {
        self.container.resource_type()
    }

    /// Creates another `Rc<ResourceContainer>` to the container
    pub fn reference_container(&self) -> Rc<ResourceContainer> {
        self.container.clone()
    }

    /// Creates a mutable reference to the container
    pub fn borrow_container(&mut self) -> Result<&mut ResourceContainer, VaultError> {
        Ok(Rc::get_mut(&mut self.container).ok_or(VaultError::VaultLocked)?)
    }

    /// Takes the ownership of the container
    pub fn own_container(self) -> Result<ResourceContainer, VaultError> {
        Ok(Rc::try_unwrap(self.container).map_err(|_| VaultError::VaultLocked)?)
    }
}
