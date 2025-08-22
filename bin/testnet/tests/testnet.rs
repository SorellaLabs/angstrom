use std::time::Duration;

use alloy::{
    eips::{Encodable2718, eip1559::ETHEREUM_BLOCK_GAS_LIMIT_30M},
    network::TransactionBuilder,
    providers::Provider,
    sol_types::SolCall
};
use angstrom_types::{
    contract_bindings::angstrom::Angstrom::unlockWithEmptyAttestationCall,
    primitive::{ANGSTROM_ADDRESS, AngstromAddressConfig, CHAIN_ID},
    sol_bindings::rpc_orders::AttestAngstromBlockEmpty
};
use reth_provider::test_utils::NoopProvider;
use testing_tools::{controllers::enviroments::AngstromTestnet, utils::noop_agent};
use testnet::cli::{init_tracing, testnet::TestnetCli};

#[test]
#[serial_test::serial]
fn testnet_deploy() {
    init_tracing(4);
    AngstromAddressConfig::INTERNAL_TESTNET.try_init();

    let runner = reth::CliRunner::try_default_runtime().unwrap();
    runner.run_command_until_exit(|ctx| async move {
        let cli = TestnetCli {
            eth_fork_url: "wss://ethereum-rpc.publicnode.com".to_string(),
            ..Default::default()
        };

        let testnet = AngstromTestnet::spawn_testnet(
            NoopProvider::default(),
            cli.make_config().unwrap(),
            vec![noop_agent],
            ctx.task_executor
        )
        .await;
        assert!(testnet.is_ok());
        eyre::Ok(())
    });
}

#[test]
#[serial_test::serial]
fn testnet_bundle_unlock() {
    init_tracing(3);
    AngstromAddressConfig::INTERNAL_TESTNET.try_init();
    let runner = reth::CliRunner::try_default_runtime().unwrap();

    runner.run_command_until_exit(|ctx| async move {
        let config = TestnetCli {
            eth_fork_url: "wss://ethereum-rpc.publicnode.com".to_string(),
            ..Default::default()
        };

        let config = config.make_config().unwrap();
        let agents = vec![noop_agent];
        tracing::info!("spinning up testnet for unlock attestation test");

        // spawn testnet
        let testnet = AngstromTestnet::spawn_testnet(
            NoopProvider::default(),
            config,
            agents,
            ctx.task_executor.clone()
        )
        .await
        .expect("failed to start angstrom testnet");

        // Get validator provider (first node)
        let validator_provider = testnet.node_provider(Some(0));
        let provider = validator_provider.rpc_provider();

        // Get initial state for addresses
        let signer = testnet.get_random_peer(vec![]).get_sk();

        let executor = ctx.task_executor.clone();
        let testnet_task = ctx.task_executor.spawn_critical(
            "testnet",
            Box::pin(async move {
                testnet.run_to_completion(executor).await;
                tracing::info!("testnet run to completion");
            })
        );
        tokio::time::sleep(Duration::from_secs(5)).await;

        // Get current block number
        let current_block = provider.get_block_number().await.unwrap();
        let target_block = current_block + 1;

        tracing::info!("current block: {}, target block: {}", current_block, target_block);

        // Create and sign attestation
        let signature = AttestAngstromBlockEmpty::sign(target_block, &signer);

        // Create unlock call
        let unlock_call = unlockWithEmptyAttestationCall {
            node:      signer.address(),
            signature: signature.into()
        };
        // getting invalid signature
        let nonce = provider.get_transaction_count(signer.address()).await?;
        let fees = provider.estimate_eip1559_fees().await?;

        let tx = alloy::rpc::types::TransactionRequest::default()
            .to(*ANGSTROM_ADDRESS.get().unwrap())
            .with_from(signer.address())
            .with_input(unlock_call.abi_encode())
            .with_chain_id(*CHAIN_ID.get().unwrap())
            .with_nonce(nonce)
            .gas_limit(ETHEREUM_BLOCK_GAS_LIMIT_30M)
            .with_max_fee_per_gas(fees.max_fee_per_gas)
            .with_max_priority_fee_per_gas(fees.max_priority_fee_per_gas)
            .build(&signer)
            .await
            .unwrap();

        let hash = *tx.tx_hash();
        let encoded = tx.encoded_2718();

        let a = provider.send_raw_transaction(&encoded).await.unwrap();
        a.watch().await.unwrap();

        // Wait for transaction to be mined
        let receipt = provider
            .get_transaction_receipt(hash)
            .await
            .expect("failed to get transaction receipt")
            .expect("transaction receipt not found");

        tracing::info!("transaction receipt: {:?}", receipt);
        let bn = receipt.block_number;

        tracing::info!("attestation unlock included for block {bn:?}");

        // Verify transaction was successful
        assert!(receipt.status(), "unlock transaction should succeed");

        testnet_task.abort();
        eyre::Ok(())
    });
}
