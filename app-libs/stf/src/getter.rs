/*
	Copyright 2021 Integritee AG and Supercomputing Systems AG
	Licensed under the Apache License, Version 2.0 (the "License");
	you may not use this file except in compliance with the License.
	You may obtain a copy of the License at
		http://www.apache.org/licenses/LICENSE-2.0
	Unless required by applicable law or agreed to in writing, software
	distributed under the License is distributed on an "AS IS" BASIS,
	WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
	See the License for the specific language governing permissions and
	limitations under the License.
*/

use binary_merkle_tree::{merkle_proof, MerkleProof};
use codec::{Decode, Encode};
use ita_sgx_runtime::System;
#[cfg(feature = "evm")]
use ita_sgx_runtime::{AddressMapping, HashedAddressMapping};
use itp_stf_interface::ExecuteGetter;
use itp_stf_primitives::types::{AccountId, KeyPair, LeafIndex, OrdersString, Signature};
use itp_utils::stringify::account_id_to_string;
use log::*;
use serde::{Deserialize, Serialize};
use simplyr_lib::Order;
use sp_runtime::traits::{Keccak256, Verify};
use std::{prelude::v1::*, time::Instant};

#[cfg(feature = "evm")]
use crate::evm_helpers::{get_evm_account, get_evm_account_codes, get_evm_account_storages};

#[cfg(feature = "evm")]
use sp_core::{H160, H256};

/// Custom Merkle proof that implements codec
/// The difference to the original one is that implements the scale-codec and that the fields contain u32 instead of usize.
#[derive(Debug, PartialEq, Eq, Decode, Encode, Deserialize, Serialize)]
pub struct MerkleProofWithCodec<H, L> {
	/// Root hash of generated merkle tree.
	pub root: H,
	/// Proof items (does not contain the leaf hash, nor the root obviously).
	///
	/// This vec contains all inner node hashes necessary to reconstruct the root hash given the
	/// leaf hash.
	pub proof: Vec<H>,
	/// Number of leaves in the original tree.
	///
	/// This is needed to detect a case where we have an odd number of leaves that "get promoted"
	/// to upper layers.
	pub number_of_leaves: u32,
	/// Index of the leaf the proof is for (0-based).
	pub leaf_index: u32,
	/// Leaf content.
	pub leaf: L,
}

/// Then we can also implement conversion of the two types to make the handling more ergonomic:
impl<H, L> From<MerkleProof<H, L>> for MerkleProofWithCodec<H, L> {
	fn from(source: MerkleProof<H, L>) -> Self {
		Self {
			root: source.root,
			proof: source.proof,
			number_of_leaves: source
				.number_of_leaves
				.try_into()
				.expect("We don't have more than u32::MAX leaves; qed"),
			leaf_index: source
				.leaf_index
				.try_into()
				.expect("Leave index is never bigger than U32::Max; qed"),
			leaf: source.leaf,
		}
	}
}

#[derive(Encode, Decode, Clone, Debug, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum Getter {
	public(PublicGetter),
	trusted(TrustedGetterSigned),
}

impl From<PublicGetter> for Getter {
	fn from(item: PublicGetter) -> Self {
		Getter::public(item)
	}
}

impl From<TrustedGetterSigned> for Getter {
	fn from(item: TrustedGetterSigned) -> Self {
		Getter::trusted(item)
	}
}

#[derive(Encode, Decode, Clone, Debug, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum PublicGetter {
	some_value,
}

#[derive(Encode, Decode, Clone, Debug, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum TrustedGetter {
	free_balance(AccountId),
	reserved_balance(AccountId),
	nonce(AccountId),
	#[cfg(feature = "evm")]
	evm_nonce(AccountId),
	#[cfg(feature = "evm")]
	evm_account_codes(AccountId, H160),
	#[cfg(feature = "evm")]
	evm_account_storages(AccountId, H160, H256),
	pay_as_bid_proof(AccountId, OrdersString, LeafIndex),
}

impl TrustedGetter {
	pub fn sender_account(&self) -> &AccountId {
		match self {
			TrustedGetter::free_balance(sender_account) => sender_account,
			TrustedGetter::reserved_balance(sender_account) => sender_account,
			TrustedGetter::nonce(sender_account) => sender_account,
			#[cfg(feature = "evm")]
			TrustedGetter::evm_nonce(sender_account) => sender_account,
			#[cfg(feature = "evm")]
			TrustedGetter::evm_account_codes(sender_account, _) => sender_account,
			#[cfg(feature = "evm")]
			TrustedGetter::evm_account_storages(sender_account, ..) => sender_account,
			TrustedGetter::pay_as_bid_proof(sender_account, _orders_string, _leaf_index) =>
				sender_account,
		}
	}

	pub fn sign(&self, pair: &KeyPair) -> TrustedGetterSigned {
		let signature = pair.sign(self.encode().as_slice());
		TrustedGetterSigned { getter: self.clone(), signature }
	}
}

#[derive(Encode, Decode, Clone, Debug, PartialEq, Eq)]
pub struct TrustedGetterSigned {
	pub getter: TrustedGetter,
	pub signature: Signature,
}

impl TrustedGetterSigned {
	pub fn new(getter: TrustedGetter, signature: Signature) -> Self {
		TrustedGetterSigned { getter, signature }
	}

	pub fn verify_signature(&self) -> bool {
		self.signature
			.verify(self.getter.encode().as_slice(), self.getter.sender_account())
	}
}

impl ExecuteGetter for Getter {
	fn execute(self) -> Option<Vec<u8>> {
		match self {
			Getter::trusted(g) => match &g.getter {
				TrustedGetter::free_balance(who) => {
					let info = System::account(&who);
					debug!("TrustedGetter free_balance");
					debug!("AccountInfo for {} is {:?}", account_id_to_string(&who), info);
					debug!("Account free balance is {}", info.data.free);
					Some(info.data.free.encode())
				},

				TrustedGetter::reserved_balance(who) => {
					let info = System::account(&who);
					debug!("TrustedGetter reserved_balance");
					debug!("AccountInfo for {} is {:?}", account_id_to_string(&who), info);
					debug!("Account reserved balance is {}", info.data.reserved);
					Some(info.data.reserved.encode())
				},
				TrustedGetter::nonce(who) => {
					let nonce = System::account_nonce(&who);
					debug!("TrustedGetter nonce");
					debug!("Account nonce is {}", nonce);
					Some(nonce.encode())
				},
				#[cfg(feature = "evm")]
				TrustedGetter::evm_nonce(who) => {
					let evm_account = get_evm_account(who);
					let evm_account = HashedAddressMapping::into_account_id(evm_account);
					let nonce = System::account_nonce(&evm_account);
					debug!("TrustedGetter evm_nonce");
					debug!("Account nonce is {}", nonce);
					Some(nonce.encode())
				},
				#[cfg(feature = "evm")]
				TrustedGetter::evm_account_codes(_who, evm_account) =>
				// TODO: This probably needs some security check if who == evm_account (or assosciated)
					if let Some(info) = get_evm_account_codes(evm_account) {
						debug!("TrustedGetter Evm Account Codes");
						debug!("AccountCodes for {} is {:?}", evm_account, info);
						Some(info) // TOOD: encoded?
					} else {
						None
					},
				#[cfg(feature = "evm")]
				TrustedGetter::evm_account_storages(_who, evm_account, index) =>
				// TODO: This probably needs some security check if who == evm_account (or assosciated)
					if let Some(value) = get_evm_account_storages(evm_account, index) {
						debug!("TrustedGetter Evm Account Storages");
						debug!("AccountStorages for {} is {:?}", evm_account, value);
						Some(value.encode())
					} else {
						None
					},

				TrustedGetter::pay_as_bid_proof(_who, orders_string, leaf_index) => {
					let now = Instant::now();

					let orders: Vec<Order> =
						serde_json::from_str(orders_string).expect("error serializing to JSON");
					let orders_encoded: Vec<Vec<u8>> = orders.iter().map(|o| o.encode()).collect();

					let leaf_index: usize = match (*leaf_index).try_into() {
						Ok(index) => index,
						Err(_) => {
							info!("Error converting Leaf Index to usize: {})", leaf_index);
							return None
						},
					};

					let leaf_index_u32 = leaf_index as u32;
					if leaf_index_u32 >= orders.len() as u32 {
						info!(
							"leaf_index out of range: {} (orders length: {})",
							leaf_index,
							orders.len()
						);

						return None
					}

					let proof: MerkleProofWithCodec<_, _> =
						merkle_proof::<Keccak256, _, _>(orders_encoded, leaf_index).into();

					let elapsed = now.elapsed();
					info!("Time Elapsed for PayAsBid Proof is: {:.2?}", elapsed);

					Some(proof.encode())
				},
			},
			Getter::public(g) => match g {
				PublicGetter::some_value => Some(42u32.encode()),
			},
		}
	}

	fn get_storage_hashes_to_update(self) -> Vec<Vec<u8>> {
		Vec::new()
	}
}
