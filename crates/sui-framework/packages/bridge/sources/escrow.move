// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

module sui_bridge::escrow {
    use std::type_name::{Self, TypeName};

    use sui::balance::{Self, Balance};
    use sui::coin::{Self, Coin};
    use sui::dynamic_field as df;
    use sui::object::{Self, UID, ID};
    use sui::tx_context::TxContext;

    friend sui_bridge::bridge;

    struct BridgeEscrow has store {
        id: UID
    }

    public(friend) fun escrow_token<T>(self: &mut BridgeEscrow, coin: Coin<T>) {
        // TODO: allow list
        let coin_type = type_name::get<T>();
        if (!df::exists_(&self.id, coin_type)) {
            df::add(&mut self.id, coin_type, balance::zero<T>());
        };
        let balance = df::borrow_mut<TypeName, Balance<T>>(&mut self.id, coin_type);
        balance::join(balance, coin::into_balance(coin));
    }

    public(friend) fun escrow_object<T: key + store>(self: &mut BridgeEscrow, obj: T) {
        // TODO: allow list?
        df::add(&mut self.id, object::id(&obj), obj);
    }

    public(friend) fun claim_object<T: store>(self: &mut BridgeEscrow, obj_id: ID): T {
        df::remove<ID, T>(&mut self.id, obj_id)
    }

    public(friend) fun claim_token<T>(self: &mut BridgeEscrow, amount: u64, ctx: &mut TxContext): Coin<T> {
        let pool = df::borrow_mut<TypeName, Balance<T>>(&mut self.id, type_name::get<T>());
        coin::from_balance(balance::split(pool, amount), ctx)
    }
}