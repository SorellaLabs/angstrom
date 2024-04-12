use std::{collections::HashMap, path::Path, pin::pin, time::Duration};

use alloy_primitives::{hex, Address, U256};
use alloy_sol_types::SolStruct;
use angstrom_types::orders::{OrderLocation, OrderOrigin, OrderValidationOutcome};
use futures::future::{select, Either};
use testing_tools::{
    load_reth_db, mocks::eth_events::MockEthEventHandle,
    type_generator::orders::generate_rand_valid_limit_order, validation::TestOrderValidator
};
use validation::order::{state::upkeepers::ANGSTROM_CONTRACT, OrderValidator};

const WETH_ADDRESS: Address = Address::new(hex!("c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2"));
const USDT_ADDRESS: Address = Address::new(hex!("dAC17F958D2ee523a2206206994597C13D831ec7"));

// done to avoid having a return fn with box dyn (yaya im lazy lol)
macro_rules! init_tools {
    () => {{
        reth_tracing::init_test_tracing();
        let db_path = Path::new("/home/data/reth/db/");
        let db = load_reth_db(db_path);

        let (handle, eth_events) = MockEthEventHandle::new();
        TestOrderValidator::new(db, Box::pin(eth_events), handle)
    }};
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial]
async fn test_validation_pass() {
    let mut validator = init_tools!();

    // setup order to validate
    let mut order = generate_rand_valid_limit_order();
    order.order.currencyIn = WETH_ADDRESS;
    order.order.currencyOut = USDT_ADDRESS;
    let nonce = order.order.nonce;

    let address = order.recover_signer().unwrap();
    // overwrite the slots to ensure the balance needed exists
    let weth_approval = validator
        .config
        .approvals
        .iter()
        .find(|a| a.token == WETH_ADDRESS)
        .unwrap();

    let approval_slot = weth_approval
        .generate_slot(address, ANGSTROM_CONTRACT)
        .unwrap();

    let weth_balance = validator
        .config
        .balances
        .iter()
        .find(|a| a.token == WETH_ADDRESS)
        .unwrap();

    let balance_slot = weth_balance.generate_slot(address).unwrap();
    let mut state_overrides = HashMap::new();

    let mut weth = HashMap::new();
    weth.insert(balance_slot, U256::from(order.order.amountIn));
    weth.insert(approval_slot, U256::from(order.order.amountIn));

    let mut nonce_map = HashMap::new();
    let slot = validator.generate_nonce_slot(address, nonce.to());
    nonce_map.insert(slot, U256::ZERO);

    state_overrides.insert(WETH_ADDRESS, weth);
    state_overrides.insert(ANGSTROM_CONTRACT, nonce_map);
    validator.revm_lru.set_state_overrides(state_overrides);

    let client = validator.client.clone();
    let out = select(
        client.validate_order(OrderOrigin::External, order.try_into().unwrap()),
        Box::pin(validator.poll_for(Duration::from_millis(100)))
    )
    .await;

    match out {
        Either::Left((i, _)) => {
            tracing::info!(?i);
            assert!(i.is_valid(), "order wasn't valid");
        }
        Either::Right(..) => {
            panic!("timeout hit on validation");
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial]
async fn test_validation_nonce_failure() {
    let mut validator = init_tools!();

    // setup order to validate
    let mut order = generate_rand_valid_limit_order();
    order.order.currencyIn = WETH_ADDRESS;
    order.order.currencyOut = USDT_ADDRESS;
    let nonce = order.order.nonce;

    let address = order.recover_signer().unwrap();
    // overwrite the slots to ensure the balance needed exists
    let weth_approval = validator
        .config
        .approvals
        .iter()
        .find(|a| a.token == WETH_ADDRESS)
        .unwrap();

    let approval_slot = weth_approval
        .generate_slot(address, ANGSTROM_CONTRACT)
        .unwrap();

    let weth_balance = validator
        .config
        .balances
        .iter()
        .find(|a| a.token == WETH_ADDRESS)
        .unwrap();

    let balance_slot = weth_balance.generate_slot(address).unwrap();
    let mut state_overrides = HashMap::new();

    let mut weth = HashMap::new();
    weth.insert(balance_slot, U256::from(order.order.amountIn));
    weth.insert(approval_slot, U256::from(order.order.amountIn));

    let mut nonce_map = HashMap::new();
    let nonce: u64 = nonce.to();
    let triggered_word = U256::from(1) << nonce.to_be_bytes()[7];
    let slot = validator.generate_nonce_slot(address, nonce);

    nonce_map.insert(slot, triggered_word);

    state_overrides.insert(WETH_ADDRESS, weth);
    state_overrides.insert(ANGSTROM_CONTRACT, nonce_map);
    validator.revm_lru.set_state_overrides(state_overrides);

    let client = validator.client.clone();
    let out = select(
        client.validate_order(OrderOrigin::External, order.try_into().unwrap()),
        Box::pin(validator.poll_for(Duration::from_millis(100)))
    )
    .await;

    match out {
        Either::Left((i, _)) => {
            if let OrderValidationOutcome::Invalid(..) = i {
            } else {
                panic!("order should be invalid");
            }
        }
        Either::Right(..) => {
            panic!("timeout hit on validation");
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial]
async fn test_validation_approval_failure() {
    let mut validator = init_tools!();

    // setup order to validate
    let mut order = generate_rand_valid_limit_order();
    order.order.currencyIn = WETH_ADDRESS;
    order.order.currencyOut = USDT_ADDRESS;

    let address = order.recover_signer().unwrap();
    // overwrite the slots to ensure the balance needed exists
    let weth_approval = validator
        .config
        .approvals
        .iter()
        .find(|a| a.token == WETH_ADDRESS)
        .unwrap();

    let approval_slot = weth_approval
        .generate_slot(address, ANGSTROM_CONTRACT)
        .unwrap();

    let weth_balance = validator
        .config
        .balances
        .iter()
        .find(|a| a.token == WETH_ADDRESS)
        .unwrap();

    let balance_slot = weth_balance.generate_slot(address).unwrap();
    let mut state_overrides = HashMap::new();

    let mut weth = HashMap::new();
    weth.insert(balance_slot, U256::from(order.order.amountIn));
    weth.insert(approval_slot, U256::ZERO);

    state_overrides.insert(WETH_ADDRESS, weth);
    validator.revm_lru.set_state_overrides(state_overrides);

    let client = validator.client.clone();
    let out = select(
        client.validate_order(OrderOrigin::External, order.try_into().unwrap()),
        Box::pin(validator.poll_for(Duration::from_millis(100)))
    )
    .await;

    match out {
        Either::Left((i, _)) => {
            if let OrderValidationOutcome::Valid { order, .. } = i {
                assert!(order.location == OrderLocation::LimitParked, "missing approval");
            } else {
                panic!("order should be valid");
            }
        }
        Either::Right(..) => {
            panic!("timeout hit on validation");
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial]
async fn test_validation_balance_failure() {
    let mut validator = init_tools!();

    // setup order to validate
    let mut order = generate_rand_valid_limit_order();
    order.order.currencyIn = WETH_ADDRESS;
    order.order.currencyOut = USDT_ADDRESS;

    let address = order.recover_signer().unwrap();
    // overwrite the slots to ensure the balance needed exists
    let weth_approval = validator
        .config
        .approvals
        .iter()
        .find(|a| a.token == WETH_ADDRESS)
        .unwrap();

    let approval_slot = weth_approval
        .generate_slot(address, ANGSTROM_CONTRACT)
        .unwrap();

    let weth_balance = validator
        .config
        .balances
        .iter()
        .find(|a| a.token == WETH_ADDRESS)
        .unwrap();

    let balance_slot = weth_balance.generate_slot(address).unwrap();
    let mut state_overrides = HashMap::new();

    let mut weth = HashMap::new();
    weth.insert(balance_slot, U256::ZERO);
    weth.insert(approval_slot, U256::from(order.order.amountIn));

    state_overrides.insert(WETH_ADDRESS, weth);
    validator.revm_lru.set_state_overrides(state_overrides);

    let client = validator.client.clone();
    let out = select(
        client.validate_order(OrderOrigin::External, order.try_into().unwrap()),
        Box::pin(validator.poll_for(Duration::from_millis(100)))
    )
    .await;

    match out {
        Either::Left((i, _)) => {
            if let OrderValidationOutcome::Valid { order, .. } = i {
                assert!(order.location == OrderLocation::LimitParked, "missing balance");
            } else {
                panic!("order should be valid");
            }
        }
        Either::Right(..) => {
            panic!("timeout hit on validation");
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial]
async fn test_validation_balance_runout() {
    let mut validator = init_tools!();

    // setup order to validate
    let mut order = generate_rand_valid_limit_order();
    order.order.currencyIn = WETH_ADDRESS;
    order.order.currencyOut = USDT_ADDRESS;

    let address = order.recover_signer().unwrap();
    // overwrite the slots to ensure the balance needed exists
    let weth_approval = validator
        .config
        .approvals
        .iter()
        .find(|a| a.token == WETH_ADDRESS)
        .unwrap();

    let approval_slot = weth_approval
        .generate_slot(address, ANGSTROM_CONTRACT)
        .unwrap();

    let weth_balance = validator
        .config
        .balances
        .iter()
        .find(|a| a.token == WETH_ADDRESS)
        .unwrap();

    let balance_slot = weth_balance.generate_slot(address).unwrap();
    let mut state_overrides = HashMap::new();

    let mut weth = HashMap::new();
    weth.insert(balance_slot, U256::from(order.order.amountIn));
    weth.insert(approval_slot, U256::MAX);

    state_overrides.insert(WETH_ADDRESS, weth);
    validator.revm_lru.set_state_overrides(state_overrides);

    let client = validator.client.clone();
    let client2 = validator.client.clone();

    let mut validator = pin!(validator);

    {
        let out = select(
            client.validate_order(OrderOrigin::External, order.clone().try_into().unwrap()),
            Box::pin(validator.poll_for(Duration::from_millis(100)))
        )
        .await;

        match out {
            Either::Left((i, f)) => {
                if let OrderValidationOutcome::Valid { order, .. } = i {
                    assert!(order.location == OrderLocation::LimitPending, "wrong");
                } else {
                    panic!("order should be valid");
                }
                f.await;
            }
            Either::Right((..)) => {
                panic!("timeout hit on validation");
            }
        }
    }

    // send same order but different nonce
    order.order.nonce += U256::from(1);
    order.hash = order.order.eip712_hash_struct();

    let out = select(
        client2.validate_order(OrderOrigin::External, order.clone().try_into().unwrap()),
        Box::pin(validator.poll_for(Duration::from_millis(100)))
    )
    .await;

    match out {
        Either::Left((i, _)) => {
            if let OrderValidationOutcome::Valid { order, .. } = i {
                assert!(order.location == OrderLocation::LimitParked, "should be parked");
            } else {
                panic!("order should be valid");
            }
        }
        Either::Right(..) => {
            panic!("timeout hit on validation");
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial]
async fn test_validation_duplicated_nonce() {
    let mut validator = init_tools!();

    // setup order to validate
    let mut order = generate_rand_valid_limit_order();
    order.order.currencyIn = WETH_ADDRESS;
    order.order.currencyOut = USDT_ADDRESS;

    let address = order.recover_signer().unwrap();
    // overwrite the slots to ensure the balance needed exists
    let weth_approval = validator
        .config
        .approvals
        .iter()
        .find(|a| a.token == WETH_ADDRESS)
        .unwrap();

    let approval_slot = weth_approval
        .generate_slot(address, ANGSTROM_CONTRACT)
        .unwrap();

    let weth_balance = validator
        .config
        .balances
        .iter()
        .find(|a| a.token == WETH_ADDRESS)
        .unwrap();

    let balance_slot = weth_balance.generate_slot(address).unwrap();
    let mut state_overrides = HashMap::new();

    let mut weth = HashMap::new();
    weth.insert(balance_slot, U256::MAX);
    weth.insert(approval_slot, U256::MAX);

    state_overrides.insert(WETH_ADDRESS, weth);
    validator.revm_lru.set_state_overrides(state_overrides);

    let client = validator.client.clone();
    let client2 = validator.client.clone();

    let mut validator = pin!(validator);

    {
        let out = select(
            client.validate_order(OrderOrigin::External, order.clone().try_into().unwrap()),
            Box::pin(validator.poll_for(Duration::from_millis(100)))
        )
        .await;

        match out {
            Either::Left((i, f)) => {
                if let OrderValidationOutcome::Valid { order, .. } = i {
                    assert!(order.location == OrderLocation::LimitPending, "wrong");
                } else {
                    panic!("order should be valid");
                }
                f.await;
            }
            Either::Right((..)) => {
                panic!("timeout hit on validation");
            }
        }
    }

    // send same order but different nonce

    let out = select(
        client2.validate_order(OrderOrigin::External, order.clone().try_into().unwrap()),
        Box::pin(validator.poll_for(Duration::from_millis(100)))
    )
    .await;

    match out {
        Either::Left((i, _)) => {
            if let OrderValidationOutcome::Invalid(..) = i {
            } else {
                panic!("order should be valid");
            }
        }
        Either::Right(..) => {
            panic!("timeout hit on validation");
        }
    }
}
