// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use move_core_types::language_storage::StructTag;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use sui_json_rpc_types::{EndOfEpochInfo, ObjectChange};
use sui_types::base_types::{ObjectDigest, SequenceNumber};
use sui_types::base_types::{ObjectID, SuiAddress};
use sui_types::digests::TransactionDigest;
use sui_types::dynamic_field::DynamicFieldInfo;
use sui_types::effects::TransactionEffects;
use sui_types::messages_checkpoint::{CheckpointDigest, EndOfEpochData};
use sui_types::move_package::MovePackage;
use sui_types::object::{Object, Owner};
use sui_types::sui_serde::SuiStructTag;
use sui_types::sui_system_state::sui_system_state_summary::SuiValidatorSummary;
use sui_types::transaction::SenderSignedData;

use crate::errors::IndexerError;

pub type IndexerResult<T> = Result<T, IndexerError>;

#[derive(Debug)]
pub struct IndexedCheckpoint {
    pub sequence_number: u64,
    pub checkpoint_digest: CheckpointDigest,
    pub epoch: u64,
    pub tx_digests: Vec<TransactionDigest>,
    pub network_total_transactions: u64,
    pub previous_checkpoint_digest: Option<CheckpointDigest>,
    pub end_of_epoch: bool,
    pub timestamp_ms: u64,
    pub total_gas_cost: i64, // total gas cost could be negative
    pub computation_cost: u64,
    pub storage_cost: u64,
    pub storage_rebate: u64,
    pub non_refundable_storage_fee: u64,
    pub checkpoint_commitments: Vec<u8>,
    pub validator_signature: Vec<u8>,
    pub successful_tx_num: usize,
}

impl IndexedCheckpoint {
    pub fn from_sui_checkpoint(
        checkpoint: &sui_types::messages_checkpoint::CertifiedCheckpointSummary,
        contents: &sui_types::messages_checkpoint::CheckpointContents,
        successful_tx_num: usize,
    ) -> Self {
        let total_gas_cost = checkpoint.epoch_rolling_gas_cost_summary.computation_cost as i64
            + checkpoint.epoch_rolling_gas_cost_summary.storage_cost as i64
            - checkpoint.epoch_rolling_gas_cost_summary.storage_rebate as i64;
        let tx_digests = contents.iter().map(|t| t.transaction).collect::<Vec<_>>();
        let auth_sig = &checkpoint.auth_sig().signature;
        Self {
            sequence_number: checkpoint.sequence_number,
            checkpoint_digest: *checkpoint.digest(),
            epoch: checkpoint.epoch,
            tx_digests,
            previous_checkpoint_digest: checkpoint.previous_digest,
            end_of_epoch: checkpoint.end_of_epoch_data.is_some(),
            total_gas_cost,
            computation_cost: checkpoint.epoch_rolling_gas_cost_summary.computation_cost,
            storage_cost: checkpoint.epoch_rolling_gas_cost_summary.storage_cost,
            storage_rebate: checkpoint.epoch_rolling_gas_cost_summary.storage_rebate,
            non_refundable_storage_fee: checkpoint
                .epoch_rolling_gas_cost_summary
                .non_refundable_storage_fee,
            successful_tx_num,
            network_total_transactions: checkpoint.network_total_transactions,
            timestamp_ms: checkpoint.timestamp_ms,
            validator_signature: bcs::to_bytes(auth_sig).unwrap(),
            checkpoint_commitments: bcs::to_bytes(&checkpoint.checkpoint_commitments).unwrap(),
        }
    }
}

#[derive(Debug)]
pub struct IndexedEndOfEpochInfo {
    pub epoch: u64,
    pub epoch_total_transactions: u64,
    pub end_of_epoch_info: EndOfEpochInfo,
    pub end_of_epoch_data: EndOfEpochData,
}

#[derive(Debug)]
pub struct IndexedEpochInfo {
    pub epoch: u64,
    pub validators: Vec<SuiValidatorSummary>,
    pub epoch_total_transactions: u64,
    pub first_checkpoint_id: u64,
    pub epoch_start_timestamp: u64,
    pub end_of_epoch_info: Option<EndOfEpochInfo>,
    pub end_of_epoch_data: Option<EndOfEpochData>,
    pub reference_gas_price: u64,
    pub protocol_version: u64,
}

#[derive(Debug)]
pub struct IndexedEvent {
    pub tx_sequence_number: u64,
    pub event_sequence_number: u64,
    pub transaction_digest: TransactionDigest,
    pub sender: SuiAddress,
    pub package: ObjectID,
    pub module: String,
    pub event_type: String,
    pub bcs: Vec<u8>,
    pub timestamp_ms: u64,
}

impl IndexedEvent {
    pub fn from_event(
        tx_sequence_number: u64,
        event_sequence_number: u64,
        transaction_digest: TransactionDigest,
        event: &sui_types::event::Event,
        timestamp_ms: u64,
    ) -> Self {
        Self {
            tx_sequence_number,
            event_sequence_number,
            transaction_digest,
            sender: event.sender,
            package: event.package_id,
            module: event.transaction_module.to_string(),
            event_type: event.type_.to_string(),
            bcs: event.contents.clone(),
            timestamp_ms,
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub enum OwnerType {
    Immutable = 0,
    Address = 1,
    Object = 2,
    Shared = 3,
}

// return owner_type, owner_address
pub fn owner_to_owner_info(owner: &Owner) -> (OwnerType, Option<SuiAddress>) {
    match owner {
        Owner::AddressOwner(address) => (OwnerType::Address, Some(*address)),
        Owner::ObjectOwner(address) => (OwnerType::Object, Some(*address)),
        Owner::Shared { .. } => (OwnerType::Shared, None),
        Owner::Immutable => (OwnerType::Immutable, None),
    }
}

pub fn filter_latest_objects(objects: Vec<Object>) -> Vec<Object> {
    // Transactions in checkpoint are ordered by causal depedencies.
    // But HashMap is not a lot more costly than HashSet, and it
    // may be good to still keep the relative order of objects in
    // the checkpoint.
    let mut latest_objects = HashMap::new();
    for object in objects {
        match latest_objects.entry(object.id()) {
            Entry::Vacant(e) => {
                e.insert(object);
            }
            Entry::Occupied(mut e) => {
                if object.version() > e.get().version() {
                    e.insert(object);
                }
            }
        }
    }
    latest_objects.into_values().collect()
}

#[derive(Debug)]
pub struct IndexedObject {
    pub object_id: ObjectID,
    pub object_version: u64,
    pub object_digest: ObjectDigest,
    pub checkpoint_sequence_number: u64,
    pub owner_type: OwnerType,
    pub owner_id: Option<SuiAddress>,
    pub object: Object,
    pub coin_type: Option<String>,
    pub coin_balance: Option<u64>,
    pub df_info: Option<DynamicFieldInfo>,
}

impl IndexedObject {
    pub fn from_object(
        checkpoint_sequence_number: u64,
        object: Object,
        df_info: Option<DynamicFieldInfo>,
    ) -> Self {
        let (owner_type, owner_id) = owner_to_owner_info(&object.owner);
        let coin_type = object.coin_type_maybe().map(|t| t.to_string());
        let coin_balance = if coin_type.is_some() {
            Some(object.get_coin_value_unsafe())
        } else {
            None
        };

        Self {
            checkpoint_sequence_number,
            object_id: object.id(),
            object_version: object.version().value(),
            object_digest: object.digest(),
            owner_type,
            owner_id,
            object,
            coin_type,
            coin_balance,
            df_info,
        }
    }
}

#[derive(Debug)]
pub struct IndexedPackage {
    pub package_id: ObjectID,
    pub move_package: MovePackage,
}

#[derive(Debug, Clone)]
pub enum TransactionKind {
    SystemTransaction = 0,
    ProgrammableTransaction = 1,
}

#[derive(Debug, Clone)]
pub struct IndexedTransaction {
    pub tx_sequence_number: u64,
    pub tx_digest: TransactionDigest,
    pub sender_signed_data: SenderSignedData,
    pub effects: TransactionEffects,
    pub checkpoint_sequence_number: u64,
    pub timestamp_ms: u64,
    pub object_changes: Vec<IndexedObjectChange>,
    pub balance_change: Vec<sui_json_rpc_types::BalanceChange>,
    pub events: Vec<sui_types::event::Event>,
    pub transaction_kind: TransactionKind,
    pub successful_tx_num: u64,
}

#[derive(Debug)]
pub struct TxIndex {
    pub tx_sequence_number: u64,
    pub transaction_digest: TransactionDigest,
    pub input_objects: Vec<ObjectID>,
    pub changed_objects: Vec<ObjectID>,
    pub senders: Vec<SuiAddress>,
    pub recipients: Vec<SuiAddress>,
    pub move_calls: Vec<(ObjectID, String, String)>,
}

/// ObjectChange is not bcs deserializable, IndexedObjectChange is.
#[serde_as]
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub enum IndexedObjectChange {
    Published {
        package_id: ObjectID,
        version: SequenceNumber,
        digest: ObjectDigest,
        modules: Vec<String>,
    },
    Transferred {
        sender: SuiAddress,
        recipient: Owner,
        #[serde_as(as = "SuiStructTag")]
        object_type: StructTag,
        object_id: ObjectID,
        version: SequenceNumber,
        digest: ObjectDigest,
    },
    /// Object mutated.
    Mutated {
        sender: SuiAddress,
        owner: Owner,
        #[serde_as(as = "SuiStructTag")]
        object_type: StructTag,
        object_id: ObjectID,
        version: SequenceNumber,
        previous_version: SequenceNumber,
        digest: ObjectDigest,
    },
    /// Delete object
    Deleted {
        sender: SuiAddress,
        #[serde_as(as = "SuiStructTag")]
        object_type: StructTag,
        object_id: ObjectID,
        version: SequenceNumber,
    },
    /// Wrapped object
    Wrapped {
        sender: SuiAddress,
        #[serde_as(as = "SuiStructTag")]
        object_type: StructTag,
        object_id: ObjectID,
        version: SequenceNumber,
    },
    /// New object creation
    Created {
        sender: SuiAddress,
        owner: Owner,
        #[serde_as(as = "SuiStructTag")]
        object_type: StructTag,
        object_id: ObjectID,
        version: SequenceNumber,
        digest: ObjectDigest,
    },
}

impl From<ObjectChange> for IndexedObjectChange {
    fn from(oc: ObjectChange) -> Self {
        match oc {
            ObjectChange::Published {
                package_id,
                version,
                digest,
                modules,
            } => Self::Published {
                package_id,
                version,
                digest,
                modules,
            },
            ObjectChange::Transferred {
                sender,
                recipient,
                object_type,
                object_id,
                version,
                digest,
            } => Self::Transferred {
                sender,
                recipient,
                object_type,
                object_id,
                version,
                digest,
            },
            ObjectChange::Mutated {
                sender,
                owner,
                object_type,
                object_id,
                version,
                previous_version,
                digest,
            } => Self::Mutated {
                sender,
                owner,
                object_type,
                object_id,
                version,
                previous_version,
                digest,
            },
            ObjectChange::Deleted {
                sender,
                object_type,
                object_id,
                version,
            } => Self::Deleted {
                sender,
                object_type,
                object_id,
                version,
            },
            ObjectChange::Wrapped {
                sender,
                object_type,
                object_id,
                version,
            } => Self::Wrapped {
                sender,
                object_type,
                object_id,
                version,
            },
            ObjectChange::Created {
                sender,
                owner,
                object_type,
                object_id,
                version,
                digest,
            } => Self::Created {
                sender,
                owner,
                object_type,
                object_id,
                version,
                digest,
            },
        }
    }
}

impl From<IndexedObjectChange> for ObjectChange {
    fn from(val: IndexedObjectChange) -> Self {
        match val {
            IndexedObjectChange::Published {
                package_id,
                version,
                digest,
                modules,
            } => ObjectChange::Published {
                package_id,
                version,
                digest,
                modules,
            },
            IndexedObjectChange::Transferred {
                sender,
                recipient,
                object_type,
                object_id,
                version,
                digest,
            } => ObjectChange::Transferred {
                sender,
                recipient,
                object_type,
                object_id,
                version,
                digest,
            },
            IndexedObjectChange::Mutated {
                sender,
                owner,
                object_type,
                object_id,
                version,
                previous_version,
                digest,
            } => ObjectChange::Mutated {
                sender,
                owner,
                object_type,
                object_id,
                version,
                previous_version,
                digest,
            },
            IndexedObjectChange::Deleted {
                sender,
                object_type,
                object_id,
                version,
            } => ObjectChange::Deleted {
                sender,
                object_type,
                object_id,
                version,
            },
            IndexedObjectChange::Wrapped {
                sender,
                object_type,
                object_id,
                version,
            } => ObjectChange::Wrapped {
                sender,
                object_type,
                object_id,
                version,
            },
            IndexedObjectChange::Created {
                sender,
                owner,
                object_type,
                object_id,
                version,
                digest,
            } => ObjectChange::Created {
                sender,
                owner,
                object_type,
                object_id,
                version,
                digest,
            },
        }
    }
}