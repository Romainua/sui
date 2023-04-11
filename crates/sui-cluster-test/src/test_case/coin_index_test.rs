// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::str::FromStr;

use crate::{TestCaseImpl, TestContext};
use async_trait::async_trait;
use jsonrpsee::rpc_params;
use sui_core::test_utils::compile_ft_package;
use sui_indexer::schema::packages::package_id;
use sui_json::SuiJsonValue;
use sui_json_rpc_types::{Balance, SuiTransactionBlockResponseOptions, OwnedObjectRef};
use sui_types::gas_coin::GAS;
use sui_types::messages::ExecuteTransactionRequestType;
use sui_types::object::Owner;
use test_utils::messages::make_staking_transaction_with_wallet_context;
use tracing::info;
use sui_types::base_types::ObjectID;
use sui_json_rpc_types::SuiTransactionBlockEffectsAPI;

pub struct CoinIndexTest;

#[async_trait]
impl TestCaseImpl for CoinIndexTest {
    fn name(&self) -> &'static str {
        "CoinIndex"
    }

    fn description(&self) -> &'static str {
        "Test executing coin index"
    }

    async fn run(&self, ctx: &mut TestContext) -> Result<(), anyhow::Error> {
        let account = ctx.get_wallet_address();
        let client = ctx.clone_fullnode_client();
        let rgp = ctx.get_reference_gas_price().await;

        ctx.get_sui_from_faucet(None).await;
        let Balance {
            coin_object_count: mut old_coin_object_count,
            total_balance: mut old_total_balance,
            ..
        } = client.coin_read_api().get_balance(account, None).await?;

        let txn = ctx.make_transactions(1).await.swap_remove(0);

        let response = client
            .quorum_driver()
            .execute_transaction_block(
                txn,
                SuiTransactionBlockResponseOptions::new()
                    .with_effects()
                    .with_balance_changes(),
                Some(ExecuteTransactionRequestType::WaitForLocalExecution),
            )
            .await?;

        let balance_change = response.balance_changes.unwrap();
        let owner_balance = balance_change
            .iter()
            .find(|b| b.owner == Owner::AddressOwner(account))
            .unwrap();
        let recipient_balance = balance_change
            .iter()
            .find(|b| b.owner != Owner::AddressOwner(account))
            .unwrap();
        let Balance {
            coin_object_count,
            total_balance,
            coin_type,
            ..
        } = client.coin_read_api().get_balance(account, None).await?;
        assert_eq!(coin_type, GAS::type_().to_string());

        assert_eq!(coin_object_count, old_coin_object_count);
        assert_eq!(
            total_balance,
            (old_total_balance as i128 + owner_balance.amount) as u128
        );
        old_coin_object_count = coin_object_count;
        old_total_balance = total_balance;

        let Balance {
            coin_object_count,
            total_balance,
            ..
        } = client
            .coin_read_api()
            .get_balance(recipient_balance.owner.get_owner_address().unwrap(), None)
            .await?;
        assert_eq!(coin_object_count, 1);
        assert!(recipient_balance.amount > 0);
        assert_eq!(total_balance, recipient_balance.amount as u128);

        // Staking
        let validator_addr = ctx
            .get_latest_sui_system_state()
            .await
            .active_validators
            .get(0)
            .unwrap()
            .sui_address;
        let txn =
            make_staking_transaction_with_wallet_context(ctx.get_wallet_mut(), validator_addr)
                .await;

        let response = client
            .quorum_driver()
            .execute_transaction_block(
                txn,
                SuiTransactionBlockResponseOptions::new()
                    .with_effects()
                    .with_balance_changes(),
                Some(ExecuteTransactionRequestType::WaitForLocalExecution),
            )
            .await?;

        info!("response: {:?}", response);
        let balance_change = &response.balance_changes.unwrap()[0];
        assert_eq!(balance_change.owner, Owner::AddressOwner(account));

        let Balance {
            coin_object_count,
            total_balance,
            ..
        } = client.coin_read_api().get_balance(account, None).await?;
        assert_eq!(coin_object_count, old_coin_object_count - 1); // an object is staked
        assert_eq!(
            total_balance,
            (old_total_balance as i128 + balance_change.amount) as u128
        );
        old_coin_object_count = coin_object_count;
        old_total_balance = total_balance;

        let (package, cap) = publish_ft_package(ctx).await?;
        let txn = client.transaction_builder().move_call(
            account,
            package.object_id(),
            "managed".into(),
            "mint".into(),
            vec![],
            vec![
                SuiJsonValue::from_str(&account.to_string())?,
                SuiJsonValue::from_str(&cap.object_id().to_string())?,
                SuiJsonValue::from_str("10000")?,
            ],
            None,
            rgp * 20_000,
        ).await?;
        let response = ctx.sign_and_execute(txn, "mint managed coin to self").await;

        // let response = client
        // .quorum_driver()
        // .execute_transaction_block(
        //     txn,
        //     SuiTransactionBlockResponseOptions::new()
        //         .with_effects()
        //         .with_balance_changes(),
        //     Some(ExecuteTransactionRequestType::WaitForLocalExecution),
        // )
        // .await?;

        println!("balance: {:?}", response.balance_changes);
        let balance_change = response.balance_changes.unwrap();
        // let owner_balance = balance_change
        //     .iter()
        //     .find(|b| b.owner == Owner::AddressOwner(account))
        //     .unwrap();
        let balances = client.coin_read_api().get_all_balances(account).await?;
        println!("balances: {:?}", balances);

        // // let obj = response.effects.unwrap().gas_object().reference.object_id;
        // let mut objs = client
        //     .coin_read_api()
        //     .get_coins(account, None, None, None)
        //     .await?
        //     .data;
        // let primary_coin = objs.swap_remove(0);
        // let coin_to_merge = objs.swap_remove(0);

        // .move_call(
        //     *address,
        //     SUI_FRAMEWORK_ADDRESS.into(),
        //     COIN_MODULE_NAME.to_string(),
        //     "mint_and_transfer".into(),
        //     type_args![coin_name]?,
        //     call_args![treasury_cap, 100000, address]?,
        //     Some(gas.object_id),
        //     10_000_000.into(),
        //     None,
        // )
        Ok(())
    }
}

async fn publish_ft_package(ctx: &mut TestContext) -> Result<(OwnedObjectRef, OwnedObjectRef), anyhow::Error> {
    let compiled_package = compile_ft_package();
    let all_module_bytes =
        compiled_package.get_package_base64(/* with_unpublished_deps */ false);
    let dependencies = compiled_package.get_dependency_original_package_ids();

    let params = rpc_params![
        ctx.get_wallet_address(),
        all_module_bytes,
        dependencies,
        None::<ObjectID>,
        // Doesn't need to be scaled by RGP since most of the cost is storage
        50_000_000.to_string()
    ];

    let data = ctx
        .build_transaction_remotely("unsafe_publish", params)
        .await?;
    let response = ctx.sign_and_execute(data, "publish ft package").await;
    let effects = response
        .effects
        .unwrap();
    let package = effects.created()
        .iter()
        .find(|obj_ref| obj_ref.owner == Owner::Immutable)
        .unwrap()
        .clone();
    let treasury_cap = effects.created()
        .iter()
        .find(|obj_ref| obj_ref.owner == Owner::AddressOwner(ctx.get_wallet_address()))
        .unwrap()
        .clone();
    Ok((package, treasury_cap))
}
