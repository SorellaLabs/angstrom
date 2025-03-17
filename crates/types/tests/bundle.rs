use std::{collections::HashMap, io::Read, str::FromStr};

use alloy::{
    network::{Ethereum, EthereumWallet},
    providers::Provider,
    signers::local::PrivateKeySigner,
    sol_types::SolInterface
};
use alloy_primitives::{
    Address, Bytes, FixedBytes, I256, TxHash, U256,
    aliases::{I24, U24}
};
use angstrom_types::{
    block_sync::GlobalBlockSync,
    contract_bindings::{
        angstrom::Angstrom::AngstromInstance,
        controller_v_1::ControllerV1::ControllerV1Instance,
        mintable_mock_erc_20::MintableMockERC20,
        pool_gate::PoolGate::PoolGateInstance,
        pool_manager::{
            IPoolManager::ModifyLiquidityParams,
            PoolManager::{PoolKey, PoolManagerErrors, PoolManagerInstance}
        }
    },
    contract_payloads::{
        Asset, Pair, Signature,
        angstrom::{AngstromBundle, BundleGasDetails, OrderQuantities, TopOfBlockOrder, UserOrder},
        rewards::PoolUpdate
    },
    matching::{Ray, SqrtPriceX96, uniswap::LiqRange},
    primitive::AngstromSigner
};
use pade::PadeEncode;
use testing_tools::{
    contracts::{
        DebugTransaction,
        environment::{
            LocalAnvil, SpawnedAnvil, TestAnvilEnvironment,
            angstrom::AngstromEnv,
            uniswap::{TestUniswapEnv, UniswapEnv}
        }
    },
    providers::{AnvilInitializer, AnvilProvider, WalletProvider},
    type_generator::{
        amm::AMMSnapshotBuilder,
        consensus::{pool::Pool, proposal::ProposalBuilder}
    },
    types::{
        config::{TestingNodeConfig, TestnetConfig},
        initial_state::PartialConfigPoolKey
    }
};

fn raw_bundle(t0: Address, t1: Address) -> AngstromBundle {
    AngstromBundle {
        assets:              vec![
            Asset {
                addr:   t0,
                take:   1957294855932995510748379,
                save:   0,
                settle: 2768096223250057485352960
            },
            Asset { addr: t1, take: 13210706843549196798465171549, save: 0, settle: 0 },
        ],
        pairs:               vec![Pair {
            index0:       0,
            index1:       1,
            store_index:  0,
            price_1over0: U256::from(21800768998050910241799966556160_u128)
        }],
        pool_updates:        vec![PoolUpdate {
            zero_for_one:     false,
            pair_index:       0,
            swap_in_quantity: 810801367317061974604581,
            rewards_update:
                angstrom_types::contract_payloads::rewards::RewardsUpdate::CurrentOnly { amount: 0 }
        }],
        top_of_block_orders: vec![TopOfBlockOrder {
            use_internal:     false,
            quantity_in:      810801367317061974604581_u128,
            quantity_out:     13210706843549196798465171549_u128,
            max_gas_asset_0:  810801367317061974604581_u128,
            gas_used_asset_0: 810801367317061974604581_u128,
            pairs_index:      0,
            zero_for_1:       true,
            recipient:        Some(
                Address::from_str("0x4be689ce93b9446edad39269ca37bf1184747a2f").unwrap()
            ),
            signature:        Signature::Ecdsa {
                v: 27,
                r: FixedBytes::from_str(
                    "0xbe80fb0de9a1a9dbb267936eb419f65960062870fb46ede8616c4f19f2d43f08"
                )
                .unwrap(),
                s: FixedBytes::from_str(
                    "0x6d70b3f10a47b315a4f9a3d73101e66885cdc4c3807182f0a8762847813cd287"
                )
                .unwrap()
            }
        }],
        user_orders:         vec![
            UserOrder {
                ref_id:               0,
                use_internal:         false,
                pair_index:           0,
                min_price:            U256::from(49378067600787200583315_u128),
                recipient:            None,
                hook_data:            None,
                zero_for_one:         false,
                standing_validation:  None,
                order_quantities:     OrderQuantities::Exact { quantity: 610941543648688290660352 },
                max_extra_fee_asset0: 610941543648688290660352,
                extra_fee_asset0:     610941543648688290660352,
                exact_in:             false,
                signature:            Signature::Ecdsa {
                    v: 28,
                    r: FixedBytes::from_str(
                        "0x68d131e2918847bc1095def0cf2f24767d3a1667599557407941e8f93d506adc"
                    )
                    .unwrap(),
                    s: FixedBytes::from_str(
                        "0x2d107cad6914b3898c56347d3b30ec87c854948f760aaafc81a480fd8e2fd08c"
                    )
                    .unwrap()
                }
            },
            UserOrder {
                ref_id:               0,
                use_internal:         false,
                pair_index:           0,
                min_price:            U256::from(48351990994663474648764_u128),
                recipient:            None,
                hook_data:            None,
                zero_for_one:         false,
                standing_validation:  None,
                order_quantities:     OrderQuantities::Exact { quantity: 583425122153890188361728 },
                max_extra_fee_asset0: 583425122153890188361728,
                extra_fee_asset0:     583425122153890188361728,
                exact_in:             false,
                signature:            Signature::Ecdsa {
                    v: 27,
                    r: FixedBytes::from_str(
                        "0xf8300cb59dc08881af0fb2498d80b58e618682117474d0bbeca25ee4a8aaebd4"
                    )
                    .unwrap(),
                    s: FixedBytes::from_str(
                        "0x67a493079b4a902add9f45c688d3490c23f6a79802c1dd178a0842418543ab5a"
                    )
                    .unwrap()
                }
            },
            UserOrder {
                ref_id:               0,
                use_internal:         false,
                pair_index:           0,
                min_price:            U256::from(47933174108512206158880_u128),
                recipient:            None,
                hook_data:            None,
                zero_for_one:         false,
                standing_validation:  None,
                order_quantities:     OrderQuantities::Exact { quantity: 416848468343109802000384 },
                max_extra_fee_asset0: 416848468343109802000384,
                extra_fee_asset0:     416848468343109802000384,
                exact_in:             false,
                signature:            Signature::Ecdsa {
                    v: 28,
                    r: FixedBytes::from_str(
                        "0x6e429938ed24d469e63adf1982afd83dac134bc5053133b2398b7e09ebd85773"
                    )
                    .unwrap(),
                    s: FixedBytes::from_str(
                        "0x1458cbeba8332263e98b0c05221026af176cd1d7fd2d3aaa1436967a6963b9fa"
                    )
                    .unwrap()
                }
            },
            UserOrder {
                ref_id:               0,
                use_internal:         false,
                pair_index:           0,
                min_price:            U256::from(47694118328018537960077_u128),
                recipient:            None,
                hook_data:            None,
                zero_for_one:         false,
                standing_validation:  None,
                order_quantities:     OrderQuantities::Exact { quantity: 474485885312458955948032 },
                max_extra_fee_asset0: 474485885312458955948032,
                extra_fee_asset0:     474485885312458955948032,
                exact_in:             false,
                signature:            Signature::Ecdsa {
                    v: 28,
                    r: FixedBytes::from_str(
                        "0xd6325579bab701c92c7c83a728e1c4f6a185248479104fafe0e322481fe5c39e"
                    )
                    .unwrap(),
                    s: FixedBytes::from_str(
                        "0x47600b995584940f59d11642c0a365c9ab82aa1d36781983cf5e39ba265e9459"
                    )
                    .unwrap()
                }
            },
            UserOrder {
                ref_id:               0,
                use_internal:         false,
                pair_index:           0,
                min_price:            U256::from(47181818861036437561232_u128),
                recipient:            None,
                hook_data:            None,
                zero_for_one:         false,
                standing_validation:  None,
                order_quantities:     OrderQuantities::Exact { quantity: 682395203791910248382464 },
                max_extra_fee_asset0: 682395203791910248382464,
                extra_fee_asset0:     682395203791910248382464,
                exact_in:             false,
                signature:            Signature::Ecdsa {
                    v: 27,
                    r: FixedBytes::from_str(
                        "0x0c44925e4270992ff3a1a4a9fc194169f622915ae06f18eaf9d41ebd8ed49842"
                    )
                    .unwrap(),
                    s: FixedBytes::from_str(
                        "0x556e0cac578403c827323cdf68ac6f67cde12ab5bc4fe2749eb92b1837d9a6c5"
                    )
                    .unwrap()
                }
            },
            UserOrder {
                ref_id:               0,
                use_internal:         false,
                pair_index:           0,
                min_price:            U256::from(20282305467117333969432809046016_u128),
                recipient:            None,
                hook_data:            None,
                zero_for_one:         true,
                standing_validation:  None,
                order_quantities:     OrderQuantities::Exact {
                    quantity: 1193274575280400119103488
                },
                max_extra_fee_asset0: 1193274575280400119103488,
                extra_fee_asset0:     1193274575280400119103488,
                exact_in:             false,
                signature:            Signature::Ecdsa {
                    v: 28,
                    r: FixedBytes::from_str(
                        "0xb4b059d1c2a48728f658d13d88556476b187bc6a4f25696d202ba247856b5316"
                    )
                    .unwrap(),
                    s: FixedBytes::from_str(
                        "0x1fd0ac008cd63dcc1a670a518735bb041759e04323d92c7525cb159eaf918406"
                    )
                    .unwrap()
                }
            },
            UserOrder {
                ref_id:               0,
                use_internal:         false,
                pair_index:           0,
                min_price:            U256::from(20762483581245476488826738180096_u128),
                recipient:            None,
                hook_data:            None,
                zero_for_one:         true,
                standing_validation:  None,
                order_quantities:     OrderQuantities::Exact {
                    quantity: 1121857342254363640332288
                },
                max_extra_fee_asset0: 1121857342254363640332288,
                extra_fee_asset0:     1121857342254363640332288,
                exact_in:             false,
                signature:            Signature::Ecdsa {
                    v: 27,
                    r: FixedBytes::from_str(
                        "0x12c03dda47b3443c8d58882e3423af030881e715da9de304450adaa39a065fe7"
                    )
                    .unwrap(),
                    s: FixedBytes::from_str(
                        "0x02a002c8d50be754f00f00da0a758eb5bf301e3d29606fd81ca652b9275a3cae"
                    )
                    .unwrap()
                }
            },
            UserOrder {
                ref_id:               0,
                use_internal:         false,
                pair_index:           0,
                min_price:            U256::from(21497699819401486627642875576320_u128),
                recipient:            None,
                hook_data:            None,
                zero_for_one:         true,
                standing_validation:  None,
                order_quantities:     OrderQuantities::Exact { quantity: 852536190206042982842368 },
                max_extra_fee_asset0: 852536190206042982842368,
                extra_fee_asset0:     852536190206042982842368,
                exact_in:             false,
                signature:            Signature::Ecdsa {
                    v: 28,
                    r: FixedBytes::from_str(
                        "0xe79a89cfe197872724a49f6eb00b6acb5925dd95349fbaac454bc74681af6ef8"
                    )
                    .unwrap(),
                    s: FixedBytes::from_str(
                        "0x741aeb73c7f3157ee8534d655d787aadccdc804be408669d26844bd6d97198f1"
                    )
                    .unwrap()
                }
            },
            UserOrder {
                ref_id:               0,
                use_internal:         false,
                pair_index:           0,
                min_price:            U256::from(21741989249062707439517273423872_u128),
                recipient:            None,
                hook_data:            None,
                zero_for_one:         true,
                standing_validation:  None,
                order_quantities:     OrderQuantities::Exact { quantity: 505725768839177229565952 },
                max_extra_fee_asset0: 505725768839177229565952,
                extra_fee_asset0:     505725768839177229565952,
                exact_in:             false,
                signature:            Signature::Ecdsa {
                    v: 28,
                    r: FixedBytes::from_str(
                        "0x635042524341f3213c6aac2677a2e25d8c6df59592d41914930890c0265577f5"
                    )
                    .unwrap(),
                    s: FixedBytes::from_str(
                        "0x4ba5fd352a58a96f5d828ec128cdbeb9218ccd6940afa0a1b0547acb765b831a"
                    )
                    .unwrap()
                }
            },
            UserOrder {
                ref_id:               0,
                use_internal:         false,
                pair_index:           0,
                min_price:            U256::from(21800768998050910241799966556160_u128),
                recipient:            None,
                hook_data:            None,
                zero_for_one:         true,
                standing_validation:  None,
                order_quantities:     OrderQuantities::Partial {
                    min_quantity_in: 0,
                    max_quantity_in: 1026907610798494993874944,
                    filled_quantity: 561802272409004655247360
                },
                max_extra_fee_asset0: 1026907610798494993874944,
                extra_fee_asset0:     1026907610798494993874944,
                exact_in:             false,
                signature:            Signature::Ecdsa {
                    v: 27,
                    r: FixedBytes::from_str(
                        "0x396fcb7e87a52e90405e61d43395d240c9fc6b35da7c342150c6cea080c1f14e"
                    )
                    .unwrap(),
                    s: FixedBytes::from_str(
                        "0x0d53ba7315d0c2a3c5acd32f51bced4c4faf307b31768bef9ecba184416e2c3e"
                    )
                    .unwrap()
                }
            },
        ]
    }
}

#[tokio::test]
async fn use_raw_bundle() {
    use secp256k1::{PublicKey, Secp256k1, SecretKey};
    // Setup our environment and get our contract interface objects

    let sk: Bytes = [
        102, 27, 190, 55, 135, 232, 40, 136, 200, 139, 236, 174, 205, 166, 147, 166, 128, 135, 124,
        214, 190, 241, 2, 235, 9, 139, 91, 116, 204, 130, 120, 159
    ]
    .into();
    let signer = PrivateKeySigner::from_bytes(&Into::<TxHash>::into(
        SecretKey::from_slice(&sk).unwrap().secret_bytes()
    ))
    .unwrap();

    let config = TestingNodeConfig::new(
        0,
        TestnetConfig::new(
            1,
            vec![PartialConfigPoolKey {
                fee:               0,
                tick_spacing:      60,
                initial_liquidity: 34028236692221234111,
                sqrt_price:        SqrtPriceX96::at_tick(100020).unwrap()
            }],
            "ws://localhost:8545",
            false,
            None,
            None
        ),
        100
    );

    let mut provider = AnvilProvider::new(
        AnvilInitializer::new(config.clone(), vec![signer.address()]),
        true,
        GlobalBlockSync::new(0)
    )
    .await
    .unwrap();

    let _anvil_instance = Some(provider._instance.take().unwrap());

    let initializer = provider.provider_mut().provider_mut();

    let initial_state = initializer.initialize_state_no_bytes().await.unwrap();

    // let wallet = EthereumWallet::new(signer.clone());
    // let rpc = alloy::providers::builder::<Ethereum>()
    //     .with_recommended_fillers()
    //     .wallet(wallet)
    //     .on_builtin("http://127.0.0.1:8541")
    //     .await
    //     .unwrap();

    // let controller = signer.address();
    // let provider = WalletProvider::new_with_provider(rpc, signer);

    // let uniswap = UniswapEnv::new(provider).await.unwrap();
    // let env = AngstromEnv::new(uniswap, vec![controller]).await.unwrap();
    // let ang_instance = AngstromInstance::new(env.angstrom(), env.provider());
    // let controller_v1 = ControllerV1Instance::new(env.controller_v1(),
    // env.provider().clone()); let pool_gate =
    // PoolGateInstance::new(env.pool_gate(), env.provider());

    // // setup the two tokens we'll be working with as T0 and T1
    // let raw_c0 = MintableMockERC20::deploy(env.provider()).await.unwrap();
    // let raw_c1 = MintableMockERC20::deploy(env.provider()).await.unwrap();
    // let (currency0, currency1) = match raw_c0.address().cmp(raw_c1.address()) {
    //     std::cmp::Ordering::Greater => (*raw_c1.address(), *raw_c0.address()),
    //     _ => (*raw_c0.address(), *raw_c1.address())
    // };

    // // Setup our Uniswap pool parameters
    // let pool_manager = PoolManagerInstance::new(env.pool_manager(),
    // env.provider()); let fee = U24::ZERO;
    // let tickSpacing = I24::unchecked_from(60);
    // let hooks = env.angstrom();
    // let key = PoolKey { currency0, currency1, fee, tickSpacing, hooks };
    // let start_price = SqrtPriceX96::at_tick(100040).unwrap();

    // // Configure the pool via the Controller
    // controller_v1
    //     .configurePool(
    //         key.currency0,
    //         key.currency1,
    //         key.tickSpacing.as_i32() as u16,
    //         key.fee,
    //         key.fee
    //     )
    //     .from(controller)
    //     .run_safe()
    //     .await
    //     .unwrap();
    // println!("Configured pool");

    // ang_instance
    //     .initializePool(key.currency0, key.currency1, U256::ZERO,
    // start_price.into())     .from(controller)
    //     .run_safe()
    //     .await
    //     .unwrap();
    // println!("Ansgstrom initialized pool");

    // pool_gate
    //     .tickSpacing(tickSpacing)
    //     .from(controller)
    //     .run_safe()
    //     .await
    //     .unwrap();
    // println!("PoolGate given tick spacing");

    // // Define our liquidity range
    // let tickLower = I24::unchecked_from(100020);
    // let tickUpper = I24::unchecked_from(100080);
    // let liquidityDelta = U256::from(1_000_000_000_000_000_000_u128);

    // pool_gate
    //     .addLiquidity(
    //         key.currency0,
    //         key.currency1,
    //         tickLower,
    //         tickUpper,
    //         liquidityDelta,
    //         FixedBytes::default()
    //     )
    //     .run_safe()
    //     .await
    //     .unwrap();

    // // let params =
    // //     ModifyLiquidityParams { tickLower, tickUpper, liquidityDelta, salt:
    // // FixedBytes::random() };

    // // pool_manager
    // //     .modifyLiquidity(key.clone(), params,
    // // alloy::primitives::Bytes::default())     .run_safe()
    // //     .await
    // //     .unwrap();

    // /*
    // pool_gate
    //     .mint_0(currency0, U256::from(1_000_000_000_000_000_000_u128))
    //     .run_safe()
    //     .await
    //     .unwrap();
    // pool_gate
    //     .mint_0(currency1, U256::from(1_000_000_000_000_000_000_u128))
    //     .run_safe()
    //     .await
    //     .unwrap();
    //     */
    // let err = initializer
    //     .pool_gate
    //     .addLiquidity(
    //         currency0,
    //         currency1,
    //         tickLower,
    //         tickUpper,
    //         liquidityDelta,
    //         FixedBytes::random()
    //     )
    //     .call()
    //     .await
    //     .unwrap_err();
    // let revert_data = if let alloy::contract::Error::TransportError(e) = err {
    //     e.as_error_resp().and_then(|e| e.as_revert_data())
    // } else {
    //     None
    // }
    // .unwrap();
    // println!("Decoding revert data: {:?}", revert_data);

    // let decoded_err = PoolManagerErrors::abi_decode(&revert_data, false).ok();
    // println!("Decoded error: {decoded_err:?}");

    // pool_gate
    //     .addLiquidity(
    //         currency0,
    //         currency1,
    //         tickLower,
    //         tickUpper,
    //         liquidityDelta,
    //         FixedBytes::random()
    //     )
    //     .run_safe()
    //     .await
    //     .unwrap();

    let pool_key0 = initial_state.pool_keys[0].clone();
    let encoded = alloy_primitives::Bytes::from(
        raw_bundle(pool_key0.currency0, pool_key0.currency1).pade_encode()
    );
    initializer
        .angstrom
        .execute(encoded)
        .run_safe()
        .await
        .unwrap();
}

#[tokio::test]
#[cfg(feature = "anvil")]
async fn similar_to_prev() {
    // Create our anvil environment and grab the nodes and provider
    let anvil = LocalAnvil::new("http://localhost:8545".to_owned())
        .await
        .unwrap();
    // Some tricks since they're the same
    let spawned_anvil = SpawnedAnvil::new().await.unwrap();

    let nodes: Vec<Address> = spawned_anvil.anvil.addresses().to_vec();
    let controller = nodes[7];

    let pk_slice = spawned_anvil.anvil.keys()[7].to_bytes();
    let controller_signing_key =
        AngstromSigner::new(PrivateKeySigner::from_slice(pk_slice.as_slice()).unwrap());

    let uniswap = UniswapEnv::new(anvil).await.unwrap();
    let env = AngstromEnv::new(uniswap, vec![]).await.unwrap();
    let angstrom = AngstromInstance::new(env.angstrom(), env.provider());
    println!("Angstrom: {}", angstrom.address());
    println!("Controller: {}", controller);
    println!("Uniswap: {}", env.pool_manager());
    println!("PoolGate: {}", env.pool_gate());

    let pool_gate = PoolGateInstance::new(env.pool_gate(), env.provider());
    let raw_c0 = MintableMockERC20::deploy(env.provider()).await.unwrap();

    let raw_c1 = MintableMockERC20::deploy(env.provider()).await.unwrap();
    let (currency0, currency1) = match raw_c0.address().cmp(raw_c1.address()) {
        std::cmp::Ordering::Greater => (*raw_c1.address(), *raw_c0.address()),
        _ => (*raw_c0.address(), *raw_c1.address())
    };
    // Setup our pool
    let pool = PoolKey {
        currency0,
        currency1,
        fee: U24::ZERO,
        tickSpacing: I24::unchecked_from(10),
        hooks: Address::default()
    };
    let liquidity = 1_000_000_000_000_000_u128;
    let price = SqrtPriceX96::at_tick(100000).unwrap();
    let amm = AMMSnapshotBuilder::new(price)
        .with_positions(vec![LiqRange::new(99000, 101000, liquidity).unwrap()])
        .build();
    // Configure our pool that we just made
    angstrom
        .configurePool(pool.currency0, pool.currency1, 10, U24::ZERO, U24::ZERO)
        .from(controller)
        .run_safe()
        .await
        .unwrap();
    angstrom
        .initializePool(pool.currency0, pool.currency1, U256::ZERO, *price)
        .run_safe()
        .await
        .unwrap();
    let salt: FixedBytes<32> = FixedBytes::default();
    pool_gate
        .tickSpacing(I24::unchecked_from(10))
        .from(controller)
        .run_safe()
        .await
        .unwrap();
    pool_gate
        .addLiquidity(
            pool.currency0,
            pool.currency1,
            I24::unchecked_from(99000),
            I24::unchecked_from(101000),
            U256::from(liquidity),
            salt
        )
        .from(controller)
        .run_safe()
        .await
        .unwrap();

    // Get our ToB address and money it up
    // let tob_address = Address::random();
    // println!("TOB address: {:?}", tob_address);
    println!("--- Building Pools");
    raw_c0
        .mint(env.angstrom(), U256::from(1_000_000_000_000_000_000_u128))
        .run_safe()
        .await
        .unwrap();
    raw_c1
        .mint(controller, U256::from(1_000_000_000_000_000_000_u128))
        .run_safe()
        .await
        .unwrap();
    raw_c1
        .approve(env.angstrom(), U256::from(2201872310000_u128))
        .from(controller)
        .run_safe()
        .await
        .unwrap();
    let pool = Pool::new(pool, amm.clone(), controller);
    let pools = vec![pool.clone()];
    let current_block = env.provider().get_block_number().await.unwrap();
    println!("--- Building Proposal");
    let proposal = ProposalBuilder::new()
        .for_pools(pools)
        .order_count(10)
        .preproposal_count(1)
        .with_secret_key(controller_signing_key)
        .for_block(current_block + 2)
        .build();
    // println!("Proposal solutions:\n{:?}", proposal.solutions);
    let pools = HashMap::from([(pool.id(), (pool.token0(), pool.token1(), amm, 0))]);
    let bundle = AngstromBundle::from_proposal(
        &proposal,
        BundleGasDetails::new(
            HashMap::from([(
                (pool.token0(), pool.token1()),
                Ray::from(SqrtPriceX96::at_tick(-100000).unwrap())
            )]),
            16415544926496907170
        ),
        &pools
    )
    .unwrap();
    println!("Bundle: {:?}", bundle);
    let encoded = bundle.pade_encode();

    angstrom.toggleNodes(nodes).run_safe().await.unwrap();
    println!("--- Nodes toggled");
    angstrom
        .execute(Bytes::from(encoded))
        .from(controller)
        .run_safe()
        .await
        .unwrap();
    // angstrom.execute(encoded)
}
