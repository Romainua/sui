// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//# init --addresses Test=0x0 --accounts A

//# publish --upgradeable --sender A
module Test::M1 {
    use sui::tx_context::TxContext;
    fun init(_ctx: &mut TxContext) { }
}

//# upgrade --package Test --upgrade-capability 106
module Test::M1 {
    use sui::tx_context::TxContext;
    fun init(_ctx: &mut TxContext) { }
    fun upgraded() { }
}
