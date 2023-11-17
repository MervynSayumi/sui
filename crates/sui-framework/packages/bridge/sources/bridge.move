// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

module sui_bridge::bridge {
    use std::ascii::String;
    use std::string;
    use std::type_name;
    use std::vector;

    use sui::address;
    use sui::balance;
    use sui::bcs;
    use sui::coin::{Self, Coin};
    use sui::dynamic_field as df;
    use sui::event::emit;
    use sui::object::{Self, UID};
    use sui::transfer;
    use sui::tx_context::{Self, TxContext};
    use sui::vec_set::VecSet;

    use sui_bridge::escrow::{Self, BridgeEscrow};

    struct Bridge has key {
        id: UID,
        // nonce for replay protection
        sequence_num: u128,
        // commitee pub keys
        commitee: VecSet<vector<u8>>,
        escrow: BridgeEscrow,
    }

    const SUI: u8 = 0;
    const ETH: u8 = 1;

    const TOKEN_BRIDGE_MSG: u8 = 0;
    const OBJ_BRIDGE_MSG: u8 = 1;

    struct BridgeMessage has copy, store, drop {
        // 0: token , 1: object ? TBD
        message_type: u8,
        // TODO: confirm message format
        payload: vector<u8>
    }

    struct ApprovedBridgeMessage has store {
        target_chain: u8,
        target_address: vector<u8>,
        message: BridgeMessage,
        approved_epoch: u64,
        signatures: vector<vector<u8>>,
    }

    struct BridgeTarget {
        chain_id: u8,
        target_address: vector<u8>
    }

    struct BridgeEvent has copy, drop {
        sui_address: address,
        sequence_num: u128,
        target_chain: u8,
        target_address: vector<u8>,
        message: BridgeMessage
    }

    const UnexpectedMessageType: u64 = 0;
    const UnauthorisedClaim: u64 = 1;
    const MalformedMessageError: u64 = 2;
    const UnexpectedTokenType: u64 = 3;
    const UnexpectedChainID: u64 = 4;

    // TODO: confirm message format, do we use BCS?
    fun create_token_bridge_message<T>(token: &Coin<T>): BridgeMessage {
        let coin_type = bcs::to_bytes(type_name::borrow_string(&type_name::get<T>()));
        let amount = bcs::to_bytes(&balance::value(coin::balance(token)));
        BridgeMessage {
            message_type: TOKEN_BRIDGE_MSG,
            payload: bcs::to_bytes(&vector[coin_type, amount])
        }
    }

    fun extract_token_info_from_message<T>(message: vector<u8>): (String, u64) {
        let bcs = bcs::new(message);
        let payload = bcs::peel_vec_vec_u8(&mut bcs);
        assert!(vector::length(&payload) == 2, MalformedMessageError);
        let amount_bytes = bcs::new(vector::pop_back(&mut payload));
        let amount = bcs::peel_u64(&mut amount_bytes);

        let coin_type_bytes = bcs::new(vector::pop_back(&mut payload));
        let coin_type = string::to_ascii(string::utf8(bcs::peel_vec_u8(&mut coin_type_bytes)));
        (coin_type, amount)
    }

    // TODO: confirm message format
    fun object_to_bridge_message<T: key>(obj: &T): BridgeMessage {
        let object_id = bcs::to_bytes(&object::id(obj));
        let object_type = bcs::to_bytes(type_name::borrow_string(&type_name::get<T>()));
        let object_data = bcs::to_bytes(obj);

        BridgeMessage {
            message_type: OBJ_BRIDGE_MSG,
            payload: bcs::to_bytes(&vector[object_id, object_type, object_data])
        }
    }

    // Send object to ethereum
    public fun send_object<T: key + store>(
        bridge: &mut Bridge,
        target_chain: u8,
        target_address: vector<u8>,
        obj: T,
        ctx: &mut TxContext
    ) {
        bridge.sequence_num = bridge.sequence_num + 1;
        // create bridge message
        let message = object_to_bridge_message(&obj);
        // escrow object
        escrow::escrow_object(&mut bridge.escrow, obj);
        // emit event
        emit(BridgeEvent {
            sequence_num: bridge.sequence_num,
            sui_address: tx_context::sender(ctx),
            target_chain,
            target_address,
            message
        })
    }

    // Send token to ethereum
    public fun send_token<T>(
        self: &mut Bridge,
        target_chain: u8,
        target_address: vector<u8>,
        token: Coin<T>,
        ctx: &mut TxContext
    ) {
        self.sequence_num = self.sequence_num + 1;
        // create bridge message
        let message = create_token_bridge_message(&token);
        // escrow token
        escrow::escrow_token(&mut self.escrow, token);
        // emit event
        emit(BridgeEvent {
            sequence_num: self.sequence_num,
            sui_address: tx_context::sender(ctx),
            target_chain,
            target_address,
            message
        })
    }

    // Record bridged message approvels in Sui, call by the bridge client
    public fun approve(
        bridge: &mut Bridge,
        tx_hash: vector<u8>,
        message: vector<u8>,
        pub_keys: vector<u8>,
        signatures: vector<vector<u8>>
    ) {
        // validate signatures
        /*        vec_set::contains(bridge.commitee,)

                ecdsa_r1::secp256r1_ecrecover()*/
    }

    // Claim message from ethereum
    fun claim_token_internal<T>(
        self: &mut Bridge,
        bridge_tx_hash: vector<u8>,
        ctx: &mut TxContext
    ): (Coin<T>, address) {
        // retrieve approved bridge message
        let ApprovedBridgeMessage {
            target_chain,
            target_address,
            message,
            approved_epoch: _,
            signatures: _,
        } = df::remove<vector<u8>, ApprovedBridgeMessage>(&mut self.id, bridge_tx_hash);
        // ensure target chain is Sui
        assert!(target_chain == SUI, UnexpectedChainID);
        // get owner address
        let owner = address::from_bytes(target_address);
        // ensure this is a token bridge message
        assert!(message.message_type == TOKEN_BRIDGE_MSG, UnexpectedMessageType);
        // extract token message
        let (token_type, amount) = extract_token_info_from_message<T>(message.payload);
        // check token type
        assert!(type_name::into_string(type_name::get<T>()) == token_type, UnexpectedTokenType);
        // claim from escrow
        (escrow::claim_token(&mut self.escrow, amount, ctx), owner)
    }

    /*    fun claim_object_internal<T>(self: &mut Bridge, bridge_tx_hash: vector<u8>, ): T {
            // retrieve approved bridge message
            let ApprovedBridgeMessage {
                target_chain,
                target_address,
                message,
                approved_epoch: _,
                signatures: _,
            } = df::remove<vector<u8>, ApprovedBridgeMessage>(&mut self.id, bridge_tx_hash);
            // ensure target chain is Sui
            assert!(target_chain == SUI, UnexpectedChainID);
            // get owner address
            let owner = address::from_bytes(target_address);
            // ensure this is a token bridge message
            assert!(message.message_type == OBJ_BRIDGE_MSG, UnexpectedMessageType);
            // get object id
            // Todo?
        }*/

    public fun claim_token<T>(self: &mut Bridge, bridge_tx_hash: vector<u8>, ctx: &mut TxContext): Coin<T> {
        let (token, owner) = claim_token_internal<T>(self, bridge_tx_hash, ctx);
        // Only token owner can claim the token
        assert!(tx_context::sender(ctx) == owner, UnauthorisedClaim);
        token
    }

    public fun claim_and_transfer_token<T>(self: &mut Bridge, bridge_tx_hash: vector<u8>, ctx: &mut TxContext) {
        let (token, owner) = claim_token_internal<T>(self, bridge_tx_hash, ctx);
        transfer::public_transfer(token, owner)
    }

    /*    public fun claim_object<T>(): T {}

        public fun claim_and_transfer_object<T>() {}*/
}
