//! LiteSVM integration coverage for market creation, canonical vaults, market
//! lifecycle transitions, and limit-order validation.

// LiteSVM intentionally returns rich transaction-failure metadata. Boxing it in
// every test helper would add indirection without reducing production account or
// instruction size, so this integration-test boundary permits the large error.
#![allow(clippy::result_large_err)]

use {
    anchor_lang::{
        prelude::Pubkey,
        solana_program::{instruction::Instruction, program_pack::Pack, system_program},
        AccountDeserialize, AccountSerialize, InstructionData, ToAccountMetas,
    },
    anchor_spl::token::spl_token::state::{
        Account as SplTokenAccount, AccountState, Mint as SplMint,
    },
    litesvm::LiteSVM,
    solana_account::Account,
    solana_keypair::Keypair,
    solana_message::{Message, VersionedMessage},
    solana_signer::Signer,
    solana_transaction::versioned::VersionedTransaction,
};

const PROGRAM_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../target/deploy/tidebook.so"
));

const TEST_PRICE_TICK_SIZE: u64 = 10_000;
const TEST_QUANTITY_LOT_SIZE: u64 = 1_000_000;
const TEST_ORDER_PRICE: u64 = 100_000_000;
const TEST_ORDER_QUANTITY: u64 = 5_000_000;

/// Mirrors the program's canonical market custody graph for instruction setup
/// and post-transaction assertions.
fn derive_market_vault_addresses(
    program_id: &Pubkey,
    market: &Pubkey,
    base_mint: &Pubkey,
    quote_mint: &Pubkey,
) -> (Pubkey, Pubkey, Pubkey) {
    let (vault_authority, _) = Pubkey::find_program_address(
        &[tidebook::constants::VAULT_AUTHORITY_SEED, market.as_ref()],
        program_id,
    );
    let (base_vault, _) = Pubkey::find_program_address(
        &[
            tidebook::constants::VAULT_SEED,
            market.as_ref(),
            base_mint.as_ref(),
        ],
        program_id,
    );
    let (quote_vault, _) = Pubkey::find_program_address(
        &[
            tidebook::constants::VAULT_SEED,
            market.as_ref(),
            quote_mint.as_ref(),
        ],
        program_id,
    );

    (vault_authority, base_vault, quote_vault)
}

fn send_initialize_market(
    svm: &mut LiteSVM,
    payer: &Keypair,
    base_mint: Pubkey,
    quote_mint: Pubkey,
) -> (Pubkey, litesvm::types::TransactionResult) {
    send_initialize_market_with_config(
        svm,
        payer,
        base_mint,
        quote_mint,
        TEST_PRICE_TICK_SIZE,
        TEST_QUANTITY_LOT_SIZE,
    )
}

fn send_initialize_market_with_config(
    svm: &mut LiteSVM,
    payer: &Keypair,
    base_mint: Pubkey,
    quote_mint: Pubkey,
    price_tick_size: u64,
    quantity_lot_size: u64,
) -> (Pubkey, litesvm::types::TransactionResult) {
    let program_id = tidebook::id();

    let (market, _) = Pubkey::find_program_address(
        &[
            tidebook::constants::MARKET_SEED,
            base_mint.as_ref(),
            quote_mint.as_ref(),
        ],
        &program_id,
    );
    let (admin_record, _) = Pubkey::find_program_address(
        &[tidebook::constants::ADMIN_SEED, payer.pubkey().as_ref()],
        &program_id,
    );
    let (vault_authority, base_vault, quote_vault) =
        derive_market_vault_addresses(&program_id, &market, &base_mint, &quote_mint);

    let instruction = Instruction::new_with_bytes(
        program_id,
        &tidebook::instruction::InitializeMarket {
            price_tick_size,
            quantity_lot_size,
        }
        .data(),
        tidebook::accounts::InitializeMarket {
            authority: payer.pubkey(),
            admin_record,
            market,
            base_mint,
            quote_mint,
            vault_authority,
            base_vault,
            quote_vault,
            token_program: anchor_spl::token::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    );

    let message = Message::new_with_blockhash(
        &[instruction],
        Some(&payer.pubkey()),
        &svm.latest_blockhash(),
    );

    let transaction =
        VersionedTransaction::try_new(VersionedMessage::Legacy(message), &[payer]).unwrap();

    let result = svm.send_transaction(transaction);
    (market, result)
}

fn send_initial_limit_order(
    svm: &mut LiteSVM,
    payer: &Keypair,
    market: Pubkey,
    price: u64,
    quantity: u64,
) -> (Pubkey, litesvm::types::TransactionResult) {
    let market_state = load_market(svm, market);
    let collateral_mint = market_state.quote_mint;
    let trader_collateral =
        create_test_token_account(svm, collateral_mint, payer.pubkey(), u64::MAX);
    let (vault_authority, _, market_vault) = derive_market_vault_addresses(
        &tidebook::id(),
        &market,
        &market_state.base_mint,
        &market_state.quote_mint,
    );

    send_initial_limit_order_with_collateral(
        svm,
        payer,
        market,
        tidebook::state::OrderSide::Bid,
        price,
        quantity,
        collateral_mint,
        trader_collateral,
        vault_authority,
        market_vault,
    )
}

#[allow(clippy::too_many_arguments)]
fn send_initial_limit_order_with_collateral(
    svm: &mut LiteSVM,
    payer: &Keypair,
    market: Pubkey,
    side: tidebook::state::OrderSide,
    price: u64,
    quantity: u64,
    collateral_mint: Pubkey,
    trader_collateral: Pubkey,
    vault_authority: Pubkey,
    market_vault: Pubkey,
) -> (Pubkey, litesvm::types::TransactionResult) {
    let order_id = load_market(svm, market).next_order_id;
    let (order, _) = Pubkey::find_program_address(
        &[
            tidebook::constants::ORDER_SEED,
            market.as_ref(),
            order_id.to_le_bytes().as_ref(),
        ],
        &tidebook::id(),
    );
    let (price_level, _) = tidebook::derive_price_level_pda(&tidebook::id(), &market, side, price);
    let instruction = Instruction::new_with_bytes(
        tidebook::id(),
        &tidebook::instruction::InsertLimitOrder {
            side,
            price,
            quantity,
        }
        .data(),
        tidebook::accounts::InsertLimitOrder {
            trader: payer.pubkey(),
            market,
            order,
            price_level,
            collateral_mint,
            trader_collateral,
            vault_authority,
            market_vault,
            token_program: anchor_spl::token::ID,
            system_program: system_program::ID,
            better_level: None,
            worse_level: None,
        }
        .to_account_metas(None),
    );
    let message = Message::new_with_blockhash(
        &[instruction],
        Some(&payer.pubkey()),
        &svm.latest_blockhash(),
    );
    let transaction =
        VersionedTransaction::try_new(VersionedMessage::Legacy(message), &[payer]).unwrap();

    (order, svm.send_transaction(transaction))
}

fn send_append_limit_order(
    svm: &mut LiteSVM,
    payer: &Keypair,
    market: Pubkey,
    side: tidebook::state::OrderSide,
    price: u64,
    quantity: u64,
    previous_order: Pubkey,
) -> (Pubkey, litesvm::types::TransactionResult) {
    let market_state = load_market(svm, market);
    let (price_level, _) = tidebook::derive_price_level_pda(&tidebook::id(), &market, side, price);
    let (vault_authority, base_vault, quote_vault) = derive_market_vault_addresses(
        &tidebook::id(),
        &market,
        &market_state.base_mint,
        &market_state.quote_mint,
    );
    let (collateral_mint, market_vault) = match side {
        tidebook::state::OrderSide::Bid => (market_state.quote_mint, quote_vault),
        tidebook::state::OrderSide::Ask => (market_state.base_mint, base_vault),
    };
    let trader_collateral =
        create_test_token_account(svm, collateral_mint, payer.pubkey(), u64::MAX);

    send_append_limit_order_with_accounts(
        svm,
        payer,
        market,
        side,
        price,
        quantity,
        previous_order,
        price_level,
        collateral_mint,
        trader_collateral,
        vault_authority,
        market_vault,
    )
}

#[allow(clippy::too_many_arguments)]
fn send_append_limit_order_with_accounts(
    svm: &mut LiteSVM,
    payer: &Keypair,
    market: Pubkey,
    side: tidebook::state::OrderSide,
    price: u64,
    quantity: u64,
    previous_order: Pubkey,
    price_level: Pubkey,
    collateral_mint: Pubkey,
    trader_collateral: Pubkey,
    vault_authority: Pubkey,
    market_vault: Pubkey,
) -> (Pubkey, litesvm::types::TransactionResult) {
    let order_id = load_market(svm, market).next_order_id;
    let (order, _) = Pubkey::find_program_address(
        &[
            tidebook::constants::ORDER_SEED,
            market.as_ref(),
            order_id.to_le_bytes().as_ref(),
        ],
        &tidebook::id(),
    );
    let instruction = Instruction::new_with_bytes(
        tidebook::id(),
        &tidebook::instruction::AppendLimitOrder {
            side,
            price,
            quantity,
        }
        .data(),
        tidebook::accounts::AppendLimitOrder {
            trader: payer.pubkey(),
            market,
            order,
            price_level,
            previous_order,
            collateral_mint,
            trader_collateral,
            vault_authority,
            market_vault,
            token_program: anchor_spl::token::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    );
    let message = Message::new_with_blockhash(
        &[instruction],
        Some(&payer.pubkey()),
        &svm.latest_blockhash(),
    );
    let transaction =
        VersionedTransaction::try_new(VersionedMessage::Legacy(message), &[payer]).unwrap();

    (order, svm.send_transaction(transaction))
}

#[allow(clippy::too_many_arguments)]
fn send_insert_limit_order(
    svm: &mut LiteSVM,
    payer: &Keypair,
    market: Pubkey,
    side: tidebook::state::OrderSide,
    price: u64,
    quantity: u64,
    better_level: Option<Pubkey>,
    worse_level: Option<Pubkey>,
) -> (Pubkey, litesvm::types::TransactionResult) {
    let market_state = load_market(svm, market);
    let order_id = market_state.next_order_id;
    let (order, _) = Pubkey::find_program_address(
        &[
            tidebook::constants::ORDER_SEED,
            market.as_ref(),
            order_id.to_le_bytes().as_ref(),
        ],
        &tidebook::id(),
    );
    let (price_level, _) = tidebook::derive_price_level_pda(&tidebook::id(), &market, side, price);
    let (vault_authority, base_vault, quote_vault) = derive_market_vault_addresses(
        &tidebook::id(),
        &market,
        &market_state.base_mint,
        &market_state.quote_mint,
    );
    let (collateral_mint, market_vault) = match side {
        tidebook::state::OrderSide::Bid => (market_state.quote_mint, quote_vault),
        tidebook::state::OrderSide::Ask => (market_state.base_mint, base_vault),
    };
    let trader_collateral =
        create_test_token_account(svm, collateral_mint, payer.pubkey(), u64::MAX);
    let instruction = Instruction::new_with_bytes(
        tidebook::id(),
        &tidebook::instruction::InsertLimitOrder {
            side,
            price,
            quantity,
        }
        .data(),
        tidebook::accounts::InsertLimitOrder {
            trader: payer.pubkey(),
            market,
            order,
            price_level,
            collateral_mint,
            trader_collateral,
            vault_authority,
            market_vault,
            token_program: anchor_spl::token::ID,
            system_program: system_program::ID,
            better_level,
            worse_level,
        }
        .to_account_metas(None),
    );
    let message = Message::new_with_blockhash(
        &[instruction],
        Some(&payer.pubkey()),
        &svm.latest_blockhash(),
    );
    let transaction =
        VersionedTransaction::try_new(VersionedMessage::Legacy(message), &[payer]).unwrap();

    (order, svm.send_transaction(transaction))
}

/// Inserts a level and fails the test immediately if the setup transaction is
/// rejected. Returning both PDAs keeps multi-level topology tests readable.
#[allow(clippy::too_many_arguments)]
fn insert_level_successfully(
    svm: &mut LiteSVM,
    payer: &Keypair,
    market: Pubkey,
    side: tidebook::state::OrderSide,
    price: u64,
    better_level: Option<Pubkey>,
    worse_level: Option<Pubkey>,
) -> (Pubkey, Pubkey) {
    let (order, result) = send_insert_limit_order(
        svm,
        payer,
        market,
        side,
        price,
        TEST_ORDER_QUANTITY,
        better_level,
        worse_level,
    );
    assert!(result.is_ok(), "price-level insertion failed: {result:?}");

    let (level, _) = tidebook::derive_price_level_pda(&tidebook::id(), &market, side, price);
    (order, level)
}

fn send_pause_market(
    svm: &mut LiteSVM,
    authority: &Keypair,
    market: Pubkey,
) -> litesvm::types::TransactionResult {
    let instruction = Instruction::new_with_bytes(
        tidebook::id(),
        &tidebook::instruction::PauseMarket {}.data(),
        tidebook::accounts::PauseMarket {
            authority: authority.pubkey(),
            market,
        }
        .to_account_metas(None),
    );
    let message = Message::new_with_blockhash(
        &[instruction],
        Some(&authority.pubkey()),
        &svm.latest_blockhash(),
    );
    let transaction =
        VersionedTransaction::try_new(VersionedMessage::Legacy(message), &[authority]).unwrap();

    svm.send_transaction(transaction)
}

struct ActiveMarketFixture {
    svm: LiteSVM,
    payer: Keypair,
    market: Pubkey,
    base_mint: Pubkey,
    quote_mint: Pubkey,
    vault_authority: Pubkey,
    base_vault: Pubkey,
    quote_vault: Pubkey,
}

fn setup_active_market(
    base_decimals: u8,
    price_tick_size: u64,
    quantity_lot_size: u64,
) -> ActiveMarketFixture {
    let mut svm = LiteSVM::new();
    let payer = Keypair::new();
    svm.add_program(tidebook::id(), PROGRAM_BYTES).unwrap();
    svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
    store_admin_record(
        &mut svm,
        payer.pubkey(),
        tidebook::state::AdminStatus::Active,
    );
    let base_mint = create_test_mint(&mut svm, base_decimals);
    let quote_mint = create_test_mint(&mut svm, 6);
    let (market, result) = send_initialize_market_with_config(
        &mut svm,
        &payer,
        base_mint,
        quote_mint,
        price_tick_size,
        quantity_lot_size,
    );
    assert!(result.is_ok(), "market initialization failed: {result:?}");
    let (vault_authority, base_vault, quote_vault) =
        derive_market_vault_addresses(&tidebook::id(), &market, &base_mint, &quote_mint);

    ActiveMarketFixture {
        svm,
        payer,
        market,
        base_mint,
        quote_mint,
        vault_authority,
        base_vault,
        quote_vault,
    }
}

fn store_admin_record(svm: &mut LiteSVM, authority: Pubkey, status: tidebook::state::AdminStatus) {
    let (address, bump) = Pubkey::find_program_address(
        &[tidebook::constants::ADMIN_SEED, authority.as_ref()],
        &tidebook::id(),
    );
    let state = tidebook::state::AdminRecord {
        authority,
        added_by: authority,
        status,
        bump,
    };
    let mut data = Vec::new();
    state.try_serialize(&mut data).unwrap();

    svm.set_account(
        address,
        Account {
            lamports: svm.minimum_balance_for_rent_exemption(data.len()),
            data,
            owner: tidebook::id(),
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();
}

/// Stores a program-owned price-level account at an arbitrary address. This is
/// used only to prove that handler-side PDA validation rejects structurally
/// valid account data placed at a noncanonical key.
fn store_price_level_at(svm: &mut LiteSVM, address: Pubkey, state: tidebook::state::PriceLevel) {
    let mut data = Vec::new();
    state.try_serialize(&mut data).unwrap();

    svm.set_account(
        address,
        Account {
            lamports: svm.minimum_balance_for_rent_exemption(data.len()),
            data,
            owner: tidebook::id(),
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();
}

fn create_test_mint(svm: &mut LiteSVM, decimals: u8) -> Pubkey {
    let mint = Pubkey::new_unique();

    let mint_state = SplMint {
        decimals,
        is_initialized: true,
        ..SplMint::default()
    };

    let mut data = vec![0_u8; SplMint::LEN];
    SplMint::pack(mint_state, &mut data).unwrap();

    svm.set_account(
        mint,
        Account {
            lamports: svm.minimum_balance_for_rent_exemption(SplMint::LEN),
            data,
            owner: anchor_spl::token::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    mint
}

fn create_test_token_account(
    svm: &mut LiteSVM,
    mint: Pubkey,
    owner: Pubkey,
    amount: u64,
) -> Pubkey {
    let address = Pubkey::new_unique();
    let token_state = SplTokenAccount {
        mint,
        owner,
        amount,
        state: AccountState::Initialized,
        ..SplTokenAccount::default()
    };
    let mut data = vec![0_u8; SplTokenAccount::LEN];
    SplTokenAccount::pack(token_state, &mut data).unwrap();

    svm.set_account(
        address,
        Account {
            lamports: svm.minimum_balance_for_rent_exemption(SplTokenAccount::LEN),
            data,
            owner: anchor_spl::token::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    address
}

fn token_balance(svm: &LiteSVM, address: Pubkey) -> u64 {
    let account = svm.get_account(&address).unwrap();
    SplTokenAccount::unpack(&account.data).unwrap().amount
}

fn load_market(svm: &LiteSVM, address: Pubkey) -> tidebook::state::Market {
    let account = svm.get_account(&address).unwrap();
    let mut data: &[u8] = &account.data;
    tidebook::state::Market::try_deserialize(&mut data).unwrap()
}

fn load_order(svm: &LiteSVM, address: Pubkey) -> tidebook::state::Order {
    let account = svm.get_account(&address).unwrap();
    let mut data: &[u8] = &account.data;
    tidebook::state::Order::try_deserialize(&mut data).unwrap()
}

fn load_price_level(svm: &LiteSVM, address: Pubkey) -> tidebook::state::PriceLevel {
    let account = svm.get_account(&address).unwrap();
    let mut data: &[u8] = &account.data;

    tidebook::state::PriceLevel::try_deserialize(&mut data).unwrap()
}

#[test]
fn market_and_order_flow() {
    let program_id = tidebook::id();
    let payer = Keypair::new();
    let mut svm = LiteSVM::new();
    let base_mint = create_test_mint(&mut svm, 9);
    let quote_mint = create_test_mint(&mut svm, 6);

    svm.add_program(program_id, PROGRAM_BYTES).unwrap();
    svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
    store_admin_record(
        &mut svm,
        payer.pubkey(),
        tidebook::state::AdminStatus::Active,
    );

    let (market, result) = send_initialize_market(&mut svm, &payer, base_mint, quote_mint);

    assert!(result.is_ok(), "initialization failed: {result:?}");

    let market_account = svm.get_account(&market).unwrap();
    let mut market_data: &[u8] = &market_account.data;
    let market_state = tidebook::state::Market::try_deserialize(&mut market_data).unwrap();
    assert_eq!(market_state.authority, payer.pubkey());
    assert_eq!(market_state.status, tidebook::state::MarketStatus::Active);
    assert_eq!(market_state.next_order_id, 1);

    let (order, result) = send_initial_limit_order(
        &mut svm,
        &payer,
        market,
        TEST_ORDER_PRICE,
        TEST_ORDER_QUANTITY,
    );
    assert!(result.is_ok(), "order placement failed: {result:?}");

    let order_account = svm.get_account(&order).unwrap();
    let mut order_data: &[u8] = &order_account.data;
    let order_state = tidebook::state::Order::try_deserialize(&mut order_data).unwrap();

    assert_eq!(order_state.owner, payer.pubkey());
    assert_eq!(order_state.market, market);
    assert_eq!(order_state.order_id, 1);
    assert_eq!(order_state.side, tidebook::state::OrderSide::Bid);
    assert_eq!(order_state.price, TEST_ORDER_PRICE);
    assert_eq!(order_state.quantity, TEST_ORDER_QUANTITY);
    assert_eq!(order_state.remaining_quantity, TEST_ORDER_QUANTITY);
    assert_eq!(order_state.locked_collateral, 500_000);
    assert_eq!(order_state.status, tidebook::state::OrderStatus::Open);

    let market_account = svm.get_account(&market).unwrap();
    let mut market_data: &[u8] = &market_account.data;
    let market_state = tidebook::state::Market::try_deserialize(&mut market_data).unwrap();
    assert_eq!(market_state.next_order_id, 2);
}

#[test]
fn ask_order_locks_base_collateral() {
    let ActiveMarketFixture {
        mut svm,
        payer,
        market,
        base_mint,
        vault_authority,
        base_vault,
        ..
    } = setup_active_market(9, TEST_PRICE_TICK_SIZE, TEST_QUANTITY_LOT_SIZE);
    let starting_balance = TEST_ORDER_QUANTITY + TEST_QUANTITY_LOT_SIZE;
    let trader_base =
        create_test_token_account(&mut svm, base_mint, payer.pubkey(), starting_balance);

    let (order, result) = send_initial_limit_order_with_collateral(
        &mut svm,
        &payer,
        market,
        tidebook::state::OrderSide::Ask,
        TEST_ORDER_PRICE,
        TEST_ORDER_QUANTITY,
        base_mint,
        trader_base,
        vault_authority,
        base_vault,
    );

    assert!(result.is_ok(), "ask placement failed: {result:?}");
    assert_eq!(
        token_balance(&svm, trader_base),
        starting_balance - TEST_ORDER_QUANTITY
    );
    assert_eq!(token_balance(&svm, base_vault), TEST_ORDER_QUANTITY);
    assert_eq!(
        load_order(&svm, order).locked_collateral,
        TEST_ORDER_QUANTITY
    );
}

#[test]
fn bid_order_locks_quote_collateral() {
    let ActiveMarketFixture {
        mut svm,
        payer,
        market,
        quote_mint,
        vault_authority,
        quote_vault,
        ..
    } = setup_active_market(9, TEST_PRICE_TICK_SIZE, TEST_QUANTITY_LOT_SIZE);
    let expected_quote_collateral = 500_000;
    let starting_balance = expected_quote_collateral + 100_000;
    let trader_quote =
        create_test_token_account(&mut svm, quote_mint, payer.pubkey(), starting_balance);

    let (order, result) = send_initial_limit_order_with_collateral(
        &mut svm,
        &payer,
        market,
        tidebook::state::OrderSide::Bid,
        TEST_ORDER_PRICE,
        TEST_ORDER_QUANTITY,
        quote_mint,
        trader_quote,
        vault_authority,
        quote_vault,
    );

    assert!(result.is_ok(), "bid placement failed: {result:?}");
    assert_eq!(
        token_balance(&svm, trader_quote),
        starting_balance - expected_quote_collateral
    );
    assert_eq!(token_balance(&svm, quote_vault), expected_quote_collateral);
    assert_eq!(
        load_order(&svm, order).locked_collateral,
        expected_quote_collateral
    );
}

#[test]
fn wrong_collateral_mint_is_rejected_atomically() {
    let ActiveMarketFixture {
        mut svm,
        payer,
        market,
        base_mint,
        vault_authority,
        base_vault,
        ..
    } = setup_active_market(9, TEST_PRICE_TICK_SIZE, TEST_QUANTITY_LOT_SIZE);
    let trader_base =
        create_test_token_account(&mut svm, base_mint, payer.pubkey(), TEST_ORDER_QUANTITY);

    let (order, result) = send_initial_limit_order_with_collateral(
        &mut svm,
        &payer,
        market,
        tidebook::state::OrderSide::Bid,
        TEST_ORDER_PRICE,
        TEST_ORDER_QUANTITY,
        base_mint,
        trader_base,
        vault_authority,
        base_vault,
    );

    assert!(result.is_err(), "bid accepted base collateral");
    assert!(svm.get_account(&order).is_none());
    assert_eq!(load_market(&svm, market).next_order_id, 1);
    assert_eq!(token_balance(&svm, trader_base), TEST_ORDER_QUANTITY);
    assert_eq!(token_balance(&svm, base_vault), 0);
}

#[test]
fn token_account_owned_by_another_wallet_is_rejected() {
    let ActiveMarketFixture {
        mut svm,
        payer,
        market,
        quote_mint,
        vault_authority,
        quote_vault,
        ..
    } = setup_active_market(9, TEST_PRICE_TICK_SIZE, TEST_QUANTITY_LOT_SIZE);
    let other_owner = Pubkey::new_unique();
    let required_collateral = 500_000;
    let trader_quote =
        create_test_token_account(&mut svm, quote_mint, other_owner, required_collateral);

    let (order, result) = send_initial_limit_order_with_collateral(
        &mut svm,
        &payer,
        market,
        tidebook::state::OrderSide::Bid,
        TEST_ORDER_PRICE,
        TEST_ORDER_QUANTITY,
        quote_mint,
        trader_quote,
        vault_authority,
        quote_vault,
    );

    assert!(
        result.is_err(),
        "another wallet's token account was accepted"
    );
    assert!(svm.get_account(&order).is_none());
    assert_eq!(load_market(&svm, market).next_order_id, 1);
    assert_eq!(token_balance(&svm, trader_quote), required_collateral);
    assert_eq!(token_balance(&svm, quote_vault), 0);
}

#[test]
fn insufficient_collateral_is_rejected_atomically() {
    let ActiveMarketFixture {
        mut svm,
        payer,
        market,
        quote_mint,
        vault_authority,
        quote_vault,
        ..
    } = setup_active_market(9, TEST_PRICE_TICK_SIZE, TEST_QUANTITY_LOT_SIZE);
    let available_collateral = 499_999;
    let trader_quote =
        create_test_token_account(&mut svm, quote_mint, payer.pubkey(), available_collateral);

    let (order, result) = send_initial_limit_order_with_collateral(
        &mut svm,
        &payer,
        market,
        tidebook::state::OrderSide::Bid,
        TEST_ORDER_PRICE,
        TEST_ORDER_QUANTITY,
        quote_mint,
        trader_quote,
        vault_authority,
        quote_vault,
    );

    assert!(result.is_err(), "order accepted insufficient collateral");
    assert!(svm.get_account(&order).is_none());
    assert_eq!(load_market(&svm, market).next_order_id, 1);
    assert_eq!(token_balance(&svm, trader_quote), available_collateral);
    assert_eq!(token_balance(&svm, quote_vault), 0);
}

#[test]
fn noncanonical_order_vault_is_rejected() {
    let ActiveMarketFixture {
        mut svm,
        payer,
        market,
        quote_mint,
        vault_authority,
        quote_vault,
        ..
    } = setup_active_market(9, TEST_PRICE_TICK_SIZE, TEST_QUANTITY_LOT_SIZE);
    let required_collateral = 500_000;
    let trader_quote =
        create_test_token_account(&mut svm, quote_mint, payer.pubkey(), required_collateral);
    let noncanonical_vault = create_test_token_account(&mut svm, quote_mint, vault_authority, 0);

    let (order, result) = send_initial_limit_order_with_collateral(
        &mut svm,
        &payer,
        market,
        tidebook::state::OrderSide::Bid,
        TEST_ORDER_PRICE,
        TEST_ORDER_QUANTITY,
        quote_mint,
        trader_quote,
        vault_authority,
        noncanonical_vault,
    );

    assert!(result.is_err(), "noncanonical market vault was accepted");
    assert!(svm.get_account(&order).is_none());
    assert_eq!(load_market(&svm, market).next_order_id, 1);
    assert_eq!(token_balance(&svm, trader_quote), required_collateral);
    assert_eq!(token_balance(&svm, noncanonical_vault), 0);
    assert_eq!(token_balance(&svm, quote_vault), 0);
}

#[test]
fn pause_and_unpause_market() {
    let program_id = tidebook::id();
    let payer = Keypair::new();
    let mut svm = LiteSVM::new();
    let base_mint = create_test_mint(&mut svm, 9);
    let quote_mint = create_test_mint(&mut svm, 6);

    svm.add_program(program_id, PROGRAM_BYTES).unwrap();
    svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
    store_admin_record(
        &mut svm,
        payer.pubkey(),
        tidebook::state::AdminStatus::Active,
    );

    let (market, result) = send_initialize_market(&mut svm, &payer, base_mint, quote_mint);

    assert!(result.is_ok(), "initialization failed: {result:?}");

    let pause_ix = Instruction::new_with_bytes(
        program_id,
        &tidebook::instruction::PauseMarket {}.data(),
        tidebook::accounts::PauseMarket {
            authority: payer.pubkey(),
            market,
        }
        .to_account_metas(None),
    );

    let blockhash = svm.latest_blockhash();
    let msg = Message::new_with_blockhash(&[pause_ix], Some(&payer.pubkey()), &blockhash);
    let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &[&payer]).unwrap();
    assert!(svm.send_transaction(tx).is_ok());

    let market_account = svm.get_account(&market).unwrap();
    let mut market_data: &[u8] = &market_account.data;
    let market_state = tidebook::state::Market::try_deserialize(&mut market_data).unwrap();
    assert_eq!(market_state.status, tidebook::state::MarketStatus::Paused);

    let unpause_ix = Instruction::new_with_bytes(
        program_id,
        &tidebook::instruction::UnpauseMarket {}.data(),
        tidebook::accounts::UnpauseMarket {
            authority: payer.pubkey(),
            market,
        }
        .to_account_metas(None),
    );

    let blockhash = svm.latest_blockhash();
    let msg = Message::new_with_blockhash(&[unpause_ix], Some(&payer.pubkey()), &blockhash);
    let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &[&payer]).unwrap();
    assert!(svm.send_transaction(tx).is_ok());

    let market_account = svm.get_account(&market).unwrap();
    let mut market_data: &[u8] = &market_account.data;
    let market_state = tidebook::state::Market::try_deserialize(&mut market_data).unwrap();
    assert_eq!(market_state.status, tidebook::state::MarketStatus::Active);
}

#[test]
fn place_order_fails_when_market_is_paused() {
    let program_id = tidebook::id();
    let payer = Keypair::new();
    let mut svm = LiteSVM::new();
    let base_mint = create_test_mint(&mut svm, 9);
    let quote_mint = create_test_mint(&mut svm, 6);

    svm.add_program(program_id, PROGRAM_BYTES).unwrap();
    svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
    store_admin_record(
        &mut svm,
        payer.pubkey(),
        tidebook::state::AdminStatus::Active,
    );

    let (market, result) = send_initialize_market(&mut svm, &payer, base_mint, quote_mint);

    assert!(result.is_ok(), "initialization failed: {result:?}");

    let pause_ix = Instruction::new_with_bytes(
        program_id,
        &tidebook::instruction::PauseMarket {}.data(),
        tidebook::accounts::PauseMarket {
            authority: payer.pubkey(),
            market,
        }
        .to_account_metas(None),
    );

    let blockhash = svm.latest_blockhash();
    let msg = Message::new_with_blockhash(&[pause_ix], Some(&payer.pubkey()), &blockhash);
    let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &[&payer]).unwrap();
    assert!(svm.send_transaction(tx).is_ok());

    let (vault_authority, _, quote_vault) =
        derive_market_vault_addresses(&program_id, &market, &base_mint, &quote_mint);
    let trader_quote = create_test_token_account(&mut svm, quote_mint, payer.pubkey(), 500_000);
    let trader_balance_before = token_balance(&svm, trader_quote);
    let vault_balance_before = token_balance(&svm, quote_vault);

    let (order, result) = send_initial_limit_order_with_collateral(
        &mut svm,
        &payer,
        market,
        tidebook::state::OrderSide::Bid,
        TEST_ORDER_PRICE,
        TEST_ORDER_QUANTITY,
        quote_mint,
        trader_quote,
        vault_authority,
        quote_vault,
    );

    assert!(result.is_err(), "paused market accepted an order");
    assert!(svm.get_account(&order).is_none());
    assert_eq!(load_market(&svm, market).next_order_id, 1);
    assert_eq!(token_balance(&svm, trader_quote), trader_balance_before);
    assert_eq!(token_balance(&svm, quote_vault), vault_balance_before);
}

#[test]
fn valid_mints_initialize_market() {
    let payer = Keypair::new();
    let mut svm = LiteSVM::new();

    svm.add_program(tidebook::id(), PROGRAM_BYTES).unwrap();
    svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
    store_admin_record(
        &mut svm,
        payer.pubkey(),
        tidebook::state::AdminStatus::Active,
    );

    let base_mint = create_test_mint(&mut svm, 9);
    let quote_mint = create_test_mint(&mut svm, 6);

    let (_, result) = send_initialize_market(&mut svm, &payer, base_mint, quote_mint);

    assert!(result.is_ok(), "initialization failed: {result:?}");
}

#[test]
fn non_mint_account_is_rejected() {
    let payer = Keypair::new();
    let mut svm = LiteSVM::new();

    svm.add_program(tidebook::id(), PROGRAM_BYTES).unwrap();
    svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
    store_admin_record(
        &mut svm,
        payer.pubkey(),
        tidebook::state::AdminStatus::Active,
    );

    let fake_base_mint = Pubkey::new_unique();
    svm.airdrop(&fake_base_mint, 1_000_000).unwrap();

    let quote_mint = create_test_mint(&mut svm, 6);

    let (_, result) = send_initialize_market(&mut svm, &payer, fake_base_mint, quote_mint);

    assert!(result.is_err(), "non-mint account was accepted");
}

#[test]
fn identical_base_and_quote_mints_are_rejected() {
    let payer = Keypair::new();
    let mut svm = LiteSVM::new();

    svm.add_program(tidebook::id(), PROGRAM_BYTES).unwrap();
    svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
    store_admin_record(
        &mut svm,
        payer.pubkey(),
        tidebook::state::AdminStatus::Active,
    );

    let mint = create_test_mint(&mut svm, 6);

    let (_, result) = send_initialize_market(&mut svm, &payer, mint, mint);

    assert!(result.is_err(), "identical mints were accepted");
}

#[test]
fn market_stores_base_and_quote_mints() {
    let payer = Keypair::new();
    let mut svm = LiteSVM::new();

    svm.add_program(tidebook::id(), PROGRAM_BYTES).unwrap();
    svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
    store_admin_record(
        &mut svm,
        payer.pubkey(),
        tidebook::state::AdminStatus::Active,
    );

    let base_mint = create_test_mint(&mut svm, 9);
    let quote_mint = create_test_mint(&mut svm, 6);

    let result = send_initialize_market(&mut svm, &payer, base_mint, quote_mint);

    assert!(result.1.is_ok());

    let (market, _) = Pubkey::find_program_address(
        &[
            tidebook::constants::MARKET_SEED,
            base_mint.as_ref(),
            quote_mint.as_ref(),
        ],
        &tidebook::id(),
    );

    let account = svm.get_account(&market).unwrap();
    let mut data: &[u8] = &account.data;

    let state = tidebook::state::Market::try_deserialize(&mut data).unwrap();

    assert_eq!(state.base_mint, base_mint);
    assert_eq!(state.quote_mint, quote_mint);
}

#[test]
fn non_admin_cannot_initialize_market() {
    let payer = Keypair::new();
    let mut svm = LiteSVM::new();

    svm.add_program(tidebook::id(), PROGRAM_BYTES).unwrap();
    svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();

    let base_mint = create_test_mint(&mut svm, 9);
    let quote_mint = create_test_mint(&mut svm, 6);

    let (_, result) = send_initialize_market(&mut svm, &payer, base_mint, quote_mint);

    assert!(result.is_err(), "non-admin initialized a market");
}

#[test]
fn disabled_admin_cannot_initialize_market() {
    let payer = Keypair::new();
    let mut svm = LiteSVM::new();

    svm.add_program(tidebook::id(), PROGRAM_BYTES).unwrap();
    svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
    store_admin_record(
        &mut svm,
        payer.pubkey(),
        tidebook::state::AdminStatus::Disabled,
    );

    let base_mint = create_test_mint(&mut svm, 9);
    let quote_mint = create_test_mint(&mut svm, 6);

    let (_, result) = send_initialize_market(&mut svm, &payer, base_mint, quote_mint);

    assert!(result.is_err(), "disabled admin initialized a market");
}

#[test]
fn zero_price_tick_size_is_rejected() {
    let payer = Keypair::new();
    let mut svm = LiteSVM::new();
    svm.add_program(tidebook::id(), PROGRAM_BYTES).unwrap();
    svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
    store_admin_record(
        &mut svm,
        payer.pubkey(),
        tidebook::state::AdminStatus::Active,
    );
    let base_mint = create_test_mint(&mut svm, 9);
    let quote_mint = create_test_mint(&mut svm, 6);

    let (market, result) = send_initialize_market_with_config(
        &mut svm,
        &payer,
        base_mint,
        quote_mint,
        0,
        TEST_QUANTITY_LOT_SIZE,
    );

    assert!(result.is_err(), "zero price tick size was accepted");
    assert!(svm.get_account(&market).is_none());
    let (_, base_vault, quote_vault) =
        derive_market_vault_addresses(&tidebook::id(), &market, &base_mint, &quote_mint);
    assert!(svm.get_account(&base_vault).is_none());
    assert!(svm.get_account(&quote_vault).is_none());
}

#[test]
fn zero_quantity_lot_size_is_rejected() {
    let payer = Keypair::new();
    let mut svm = LiteSVM::new();
    svm.add_program(tidebook::id(), PROGRAM_BYTES).unwrap();
    svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
    store_admin_record(
        &mut svm,
        payer.pubkey(),
        tidebook::state::AdminStatus::Active,
    );
    let base_mint = create_test_mint(&mut svm, 9);
    let quote_mint = create_test_mint(&mut svm, 6);

    let (market, result) = send_initialize_market_with_config(
        &mut svm,
        &payer,
        base_mint,
        quote_mint,
        TEST_PRICE_TICK_SIZE,
        0,
    );

    assert!(result.is_err(), "zero quantity lot size was accepted");
    assert!(svm.get_account(&market).is_none());
    let (_, base_vault, quote_vault) =
        derive_market_vault_addresses(&tidebook::id(), &market, &base_mint, &quote_mint);
    assert!(svm.get_account(&base_vault).is_none());
    assert!(svm.get_account(&quote_vault).is_none());
}

#[test]
fn off_tick_price_is_rejected() {
    let ActiveMarketFixture {
        mut svm,
        payer,
        market,
        ..
    } = setup_active_market(9, TEST_PRICE_TICK_SIZE, TEST_QUANTITY_LOT_SIZE);

    let (order, result) = send_initial_limit_order(
        &mut svm,
        &payer,
        market,
        TEST_ORDER_PRICE + 1,
        TEST_ORDER_QUANTITY,
    );

    assert!(result.is_err(), "off-tick price was accepted");
    assert!(svm.get_account(&order).is_none());
}

#[test]
fn off_lot_quantity_is_rejected() {
    let ActiveMarketFixture {
        mut svm,
        payer,
        market,
        ..
    } = setup_active_market(9, TEST_PRICE_TICK_SIZE, TEST_QUANTITY_LOT_SIZE);

    let (order, result) = send_initial_limit_order(
        &mut svm,
        &payer,
        market,
        TEST_ORDER_PRICE,
        TEST_ORDER_QUANTITY + 1,
    );

    assert!(result.is_err(), "off-lot quantity was accepted");
    assert!(svm.get_account(&order).is_none());
}

#[test]
fn zero_quote_notional_is_rejected() {
    let ActiveMarketFixture {
        mut svm,
        payer,
        market,
        ..
    } = setup_active_market(9, 1, 1);

    let (order, result) = send_initial_limit_order(&mut svm, &payer, market, 1, 1);

    assert!(result.is_err(), "zero quote notional was accepted");
    assert!(svm.get_account(&order).is_none());
}

#[test]
fn market_stores_price_configuration() {
    let ActiveMarketFixture { svm, market, .. } =
        setup_active_market(9, TEST_PRICE_TICK_SIZE, TEST_QUANTITY_LOT_SIZE);
    let account = svm.get_account(&market).unwrap();
    let mut data: &[u8] = &account.data;
    let state = tidebook::state::Market::try_deserialize(&mut data).unwrap();

    assert_eq!(state.base_decimals, 9);
    assert_eq!(state.quote_decimals, 6);
    assert_eq!(state.price_tick_size, TEST_PRICE_TICK_SIZE);
    assert_eq!(state.quantity_lot_size, TEST_QUANTITY_LOT_SIZE);
}

#[test]
fn market_initialization_creates_empty_canonical_vaults() {
    let payer = Keypair::new();
    let mut svm = LiteSVM::new();
    svm.add_program(tidebook::id(), PROGRAM_BYTES).unwrap();
    svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
    store_admin_record(
        &mut svm,
        payer.pubkey(),
        tidebook::state::AdminStatus::Active,
    );
    let base_mint = create_test_mint(&mut svm, 9);
    let quote_mint = create_test_mint(&mut svm, 6);

    let (market, result) = send_initialize_market(&mut svm, &payer, base_mint, quote_mint);
    assert!(result.is_ok(), "initialization failed: {result:?}");

    let (vault_authority, base_vault, quote_vault) =
        derive_market_vault_addresses(&tidebook::id(), &market, &base_mint, &quote_mint);
    let base_account = svm.get_account(&base_vault).unwrap();
    let quote_account = svm.get_account(&quote_vault).unwrap();
    assert_eq!(base_account.owner, anchor_spl::token::ID);
    assert_eq!(quote_account.owner, anchor_spl::token::ID);

    let base_state = SplTokenAccount::unpack(&base_account.data).unwrap();
    let quote_state = SplTokenAccount::unpack(&quote_account.data).unwrap();
    assert_eq!(base_state.mint, base_mint);
    assert_eq!(quote_state.mint, quote_mint);
    assert_eq!(base_state.owner, vault_authority);
    assert_eq!(quote_state.owner, vault_authority);
    assert_eq!(base_state.amount, 0);
    assert_eq!(quote_state.amount, 0);
}

#[test]
fn market_and_vaults_cannot_be_initialized_twice() {
    let payer = Keypair::new();
    let mut svm = LiteSVM::new();
    svm.add_program(tidebook::id(), PROGRAM_BYTES).unwrap();
    svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
    store_admin_record(
        &mut svm,
        payer.pubkey(),
        tidebook::state::AdminStatus::Active,
    );
    let base_mint = create_test_mint(&mut svm, 9);
    let quote_mint = create_test_mint(&mut svm, 6);

    let (_, first) = send_initialize_market(&mut svm, &payer, base_mint, quote_mint);
    assert!(first.is_ok(), "first initialization failed: {first:?}");

    svm.expire_blockhash();
    let (_, second) = send_initialize_market(&mut svm, &payer, base_mint, quote_mint);
    assert!(second.is_err(), "market and vaults initialized twice");
}

#[test]
fn noncanonical_vault_address_is_rejected() {
    let payer = Keypair::new();
    let mut svm = LiteSVM::new();
    let program_id = tidebook::id();
    svm.add_program(program_id, PROGRAM_BYTES).unwrap();
    svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
    store_admin_record(
        &mut svm,
        payer.pubkey(),
        tidebook::state::AdminStatus::Active,
    );
    let base_mint = create_test_mint(&mut svm, 9);
    let quote_mint = create_test_mint(&mut svm, 6);
    let (market, _) = Pubkey::find_program_address(
        &[
            tidebook::constants::MARKET_SEED,
            base_mint.as_ref(),
            quote_mint.as_ref(),
        ],
        &program_id,
    );
    let (admin_record, _) = Pubkey::find_program_address(
        &[tidebook::constants::ADMIN_SEED, payer.pubkey().as_ref()],
        &program_id,
    );
    let (vault_authority, _base_vault, quote_vault) =
        derive_market_vault_addresses(&program_id, &market, &base_mint, &quote_mint);
    let noncanonical_base_vault = Pubkey::new_unique();
    let instruction = Instruction::new_with_bytes(
        program_id,
        &tidebook::instruction::InitializeMarket {
            price_tick_size: TEST_PRICE_TICK_SIZE,
            quantity_lot_size: TEST_QUANTITY_LOT_SIZE,
        }
        .data(),
        tidebook::accounts::InitializeMarket {
            authority: payer.pubkey(),
            admin_record,
            market,
            base_mint,
            quote_mint,
            vault_authority,
            base_vault: noncanonical_base_vault,
            quote_vault,
            token_program: anchor_spl::token::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    );
    let message = Message::new_with_blockhash(
        &[instruction],
        Some(&payer.pubkey()),
        &svm.latest_blockhash(),
    );
    let transaction =
        VersionedTransaction::try_new(VersionedMessage::Legacy(message), &[&payer]).unwrap();

    let result = svm.send_transaction(transaction);

    assert!(result.is_err(), "noncanonical base vault was accepted");
    assert!(svm.get_account(&market).is_none());
    assert!(svm.get_account(&noncanonical_base_vault).is_none());
    assert!(svm.get_account(&quote_vault).is_none());
}

#[test]
fn first_bid_creates_best_price_level_and_fifo_queue() {
    let ActiveMarketFixture {
        mut svm,
        payer,
        market,
        ..
    } = setup_active_market(9, TEST_PRICE_TICK_SIZE, TEST_QUANTITY_LOT_SIZE);

    let (order, result) = send_initial_limit_order(
        &mut svm,
        &payer,
        market,
        TEST_ORDER_PRICE,
        TEST_ORDER_QUANTITY,
    );

    assert!(result.is_ok(), "bid placement failed: {result:?}");

    let (price_level, expected_bump) = tidebook::derive_price_level_pda(
        &tidebook::id(),
        &market,
        tidebook::state::OrderSide::Bid,
        TEST_ORDER_PRICE,
    );

    let market_state = load_market(&svm, market);
    let level_state = load_price_level(&svm, price_level);
    let order_state = load_order(&svm, order);

    assert_eq!(market_state.best_bid, Some(TEST_ORDER_PRICE));
    assert_eq!(market_state.best_ask, None);

    assert_eq!(level_state.market, market);
    assert_eq!(level_state.side, tidebook::state::OrderSide::Bid);
    assert_eq!(level_state.price, TEST_ORDER_PRICE);
    assert_eq!(level_state.better_price, None);
    assert_eq!(level_state.worse_price, None);
    assert_eq!(level_state.first_order, Some(order));
    assert_eq!(level_state.last_order, Some(order));
    assert_eq!(level_state.total_remaining_quantity, TEST_ORDER_QUANTITY);
    assert_eq!(level_state.order_count, 1);
    assert_eq!(level_state.rent_payer, payer.pubkey());
    assert_eq!(level_state.bump, expected_bump);

    assert_eq!(order_state.price_level, price_level);
    assert_eq!(order_state.previous_order, None);
    assert_eq!(order_state.next_order, None);
}

#[test]
fn first_ask_creates_best_price_level_and_fifo_queue() {
    let ActiveMarketFixture {
        mut svm,
        payer,
        market,
        base_mint,
        vault_authority,
        base_vault,
        ..
    } = setup_active_market(9, TEST_PRICE_TICK_SIZE, TEST_QUANTITY_LOT_SIZE);
    let trader_collateral =
        create_test_token_account(&mut svm, base_mint, payer.pubkey(), TEST_ORDER_QUANTITY);

    let (order, result) = send_initial_limit_order_with_collateral(
        &mut svm,
        &payer,
        market,
        tidebook::state::OrderSide::Ask,
        TEST_ORDER_PRICE,
        TEST_ORDER_QUANTITY,
        base_mint,
        trader_collateral,
        vault_authority,
        base_vault,
    );

    assert!(result.is_ok(), "ask placement failed: {result:?}");

    let (price_level, expected_bump) = tidebook::derive_price_level_pda(
        &tidebook::id(),
        &market,
        tidebook::state::OrderSide::Ask,
        TEST_ORDER_PRICE,
    );

    let market_state = load_market(&svm, market);
    let level_state = load_price_level(&svm, price_level);
    let order_state = load_order(&svm, order);

    assert_eq!(market_state.best_bid, None);
    assert_eq!(market_state.best_ask, Some(TEST_ORDER_PRICE));

    assert_eq!(level_state.market, market);
    assert_eq!(level_state.side, tidebook::state::OrderSide::Ask);
    assert_eq!(level_state.price, TEST_ORDER_PRICE);
    assert_eq!(level_state.better_price, None);
    assert_eq!(level_state.worse_price, None);
    assert_eq!(level_state.first_order, Some(order));
    assert_eq!(level_state.last_order, Some(order));
    assert_eq!(level_state.total_remaining_quantity, TEST_ORDER_QUANTITY);
    assert_eq!(level_state.order_count, 1);
    assert_eq!(level_state.rent_payer, payer.pubkey());
    assert_eq!(level_state.bump, expected_bump);

    assert_eq!(order_state.price_level, price_level);
    assert_eq!(order_state.previous_order, None);
    assert_eq!(order_state.next_order, None);
}

#[test]
fn second_bid_level_is_rejected_without_changing_best_bid() {
    let ActiveMarketFixture {
        mut svm,
        payer,
        market,
        ..
    } = setup_active_market(9, TEST_PRICE_TICK_SIZE, TEST_QUANTITY_LOT_SIZE);
    let first_price = TEST_ORDER_PRICE;
    let second_price = TEST_ORDER_PRICE + TEST_PRICE_TICK_SIZE;

    let (_, first_result) =
        send_initial_limit_order(&mut svm, &payer, market, first_price, TEST_ORDER_QUANTITY);
    assert!(
        first_result.is_ok(),
        "first bid placement failed: {first_result:?}"
    );

    let (second_order, second_result) =
        send_initial_limit_order(&mut svm, &payer, market, second_price, TEST_ORDER_QUANTITY);

    assert!(
        second_result.is_err(),
        "second price level was accepted before sorted insertion was implemented"
    );

    let market_state = load_market(&svm, market);
    assert_eq!(market_state.best_bid, Some(first_price));
    assert_eq!(market_state.next_order_id, 2);
    assert_eq!(market_state.open_order_count, 1);
    assert!(svm.get_account(&second_order).is_none());

    let (second_level, _) = tidebook::derive_price_level_pda(
        &tidebook::id(),
        &market,
        tidebook::state::OrderSide::Bid,
        second_price,
    );
    assert!(svm.get_account(&second_level).is_none());
}

#[test]
fn second_bid_at_same_price_appends_to_fifo_queue() {
    let ActiveMarketFixture {
        mut svm,
        payer,
        market,
        ..
    } = setup_active_market(9, TEST_PRICE_TICK_SIZE, TEST_QUANTITY_LOT_SIZE);

    let (first_order, first_result) = send_initial_limit_order(
        &mut svm,
        &payer,
        market,
        TEST_ORDER_PRICE,
        TEST_ORDER_QUANTITY,
    );
    assert!(
        first_result.is_ok(),
        "first order placement failed: {first_result:?}"
    );

    let (second_order, second_result) = send_append_limit_order(
        &mut svm,
        &payer,
        market,
        tidebook::state::OrderSide::Bid,
        TEST_ORDER_PRICE,
        TEST_ORDER_QUANTITY,
        first_order,
    );
    assert!(
        second_result.is_ok(),
        "FIFO append failed: {second_result:?}"
    );

    let (price_level, _) = tidebook::derive_price_level_pda(
        &tidebook::id(),
        &market,
        tidebook::state::OrderSide::Bid,
        TEST_ORDER_PRICE,
    );
    let market_state = load_market(&svm, market);
    let level_state = load_price_level(&svm, price_level);
    let first_state = load_order(&svm, first_order);
    let second_state = load_order(&svm, second_order);

    assert_eq!(market_state.best_bid, Some(TEST_ORDER_PRICE));
    assert_eq!(market_state.next_order_id, 3);
    assert_eq!(market_state.open_order_count, 2);

    assert_eq!(level_state.first_order, Some(first_order));
    assert_eq!(level_state.last_order, Some(second_order));
    assert_eq!(level_state.order_count, 2);
    assert_eq!(
        level_state.total_remaining_quantity,
        TEST_ORDER_QUANTITY * 2
    );

    assert_eq!(first_state.previous_order, None);
    assert_eq!(first_state.next_order, Some(second_order));

    assert_eq!(second_state.previous_order, Some(first_order));
    assert_eq!(second_state.next_order, None);
    assert_eq!(second_state.price_level, price_level);
}

#[test]
fn three_bids_at_same_price_preserve_fifo_order() {
    let ActiveMarketFixture {
        mut svm,
        payer,
        market,
        ..
    } = setup_active_market(9, TEST_PRICE_TICK_SIZE, TEST_QUANTITY_LOT_SIZE);

    let (first_order, first_result) = send_initial_limit_order(
        &mut svm,
        &payer,
        market,
        TEST_ORDER_PRICE,
        TEST_ORDER_QUANTITY,
    );
    assert!(
        first_result.is_ok(),
        "first order placement failed: {first_result:?}"
    );

    let (second_order, second_result) = send_append_limit_order(
        &mut svm,
        &payer,
        market,
        tidebook::state::OrderSide::Bid,
        TEST_ORDER_PRICE,
        TEST_ORDER_QUANTITY,
        first_order,
    );
    assert!(
        second_result.is_ok(),
        "second order append failed: {second_result:?}"
    );

    let (third_order, third_result) = send_append_limit_order(
        &mut svm,
        &payer,
        market,
        tidebook::state::OrderSide::Bid,
        TEST_ORDER_PRICE,
        TEST_ORDER_QUANTITY,
        second_order,
    );
    assert!(
        third_result.is_ok(),
        "third order append failed: {third_result:?}"
    );

    let (price_level, _) = tidebook::derive_price_level_pda(
        &tidebook::id(),
        &market,
        tidebook::state::OrderSide::Bid,
        TEST_ORDER_PRICE,
    );
    let market_state = load_market(&svm, market);
    let level_state = load_price_level(&svm, price_level);
    let first_state = load_order(&svm, first_order);
    let second_state = load_order(&svm, second_order);
    let third_state = load_order(&svm, third_order);

    assert_eq!(market_state.best_bid, Some(TEST_ORDER_PRICE));
    assert_eq!(market_state.best_ask, None);
    assert_eq!(market_state.next_order_id, 4);
    assert_eq!(market_state.open_order_count, 3);

    assert_eq!(level_state.first_order, Some(first_order));
    assert_eq!(level_state.last_order, Some(third_order));
    assert_eq!(level_state.order_count, 3);
    assert_eq!(
        level_state.total_remaining_quantity,
        TEST_ORDER_QUANTITY * 3
    );

    assert_eq!(first_state.previous_order, None);
    assert_eq!(first_state.next_order, Some(second_order));
    assert_eq!(second_state.previous_order, Some(first_order));
    assert_eq!(second_state.next_order, Some(third_order));
    assert_eq!(third_state.previous_order, Some(second_order));
    assert_eq!(third_state.next_order, None);

    assert_eq!(first_state.price_level, price_level);
    assert_eq!(second_state.price_level, price_level);
    assert_eq!(third_state.price_level, price_level);
}

#[test]
fn stale_tail_is_rejected_without_mutating_fifo_or_collateral() {
    let ActiveMarketFixture {
        mut svm,
        payer,
        market,
        quote_vault,
        ..
    } = setup_active_market(9, TEST_PRICE_TICK_SIZE, TEST_QUANTITY_LOT_SIZE);

    let (first_order, first_result) = send_initial_limit_order(
        &mut svm,
        &payer,
        market,
        TEST_ORDER_PRICE,
        TEST_ORDER_QUANTITY,
    );
    assert!(
        first_result.is_ok(),
        "first order placement failed: {first_result:?}"
    );

    let (second_order, second_result) = send_append_limit_order(
        &mut svm,
        &payer,
        market,
        tidebook::state::OrderSide::Bid,
        TEST_ORDER_PRICE,
        TEST_ORDER_QUANTITY,
        first_order,
    );
    assert!(
        second_result.is_ok(),
        "second order append failed: {second_result:?}"
    );

    let (price_level, _) = tidebook::derive_price_level_pda(
        &tidebook::id(),
        &market,
        tidebook::state::OrderSide::Bid,
        TEST_ORDER_PRICE,
    );
    let market_before = load_market(&svm, market);
    let level_before = load_price_level(&svm, price_level);
    let vault_balance_before = token_balance(&svm, quote_vault);

    // The current tail is `second_order`; supplying the older first order must
    // fail before any collateral or queue mutation can commit.
    let (rejected_order, rejected_result) = send_append_limit_order(
        &mut svm,
        &payer,
        market,
        tidebook::state::OrderSide::Bid,
        TEST_ORDER_PRICE,
        TEST_ORDER_QUANTITY,
        first_order,
    );
    assert!(
        rejected_result.is_err(),
        "append with a stale FIFO tail was accepted"
    );

    let market_after = load_market(&svm, market);
    let level_after = load_price_level(&svm, price_level);
    let first_after = load_order(&svm, first_order);
    let second_after = load_order(&svm, second_order);

    assert_eq!(market_after.next_order_id, market_before.next_order_id);
    assert_eq!(
        market_after.open_order_count,
        market_before.open_order_count
    );
    assert_eq!(market_after.best_bid, market_before.best_bid);
    assert_eq!(market_after.best_ask, market_before.best_ask);

    assert_eq!(level_after.first_order, level_before.first_order);
    assert_eq!(level_after.last_order, level_before.last_order);
    assert_eq!(level_after.order_count, level_before.order_count);
    assert_eq!(
        level_after.total_remaining_quantity,
        level_before.total_remaining_quantity
    );

    assert_eq!(first_after.previous_order, None);
    assert_eq!(first_after.next_order, Some(second_order));
    assert_eq!(second_after.previous_order, Some(first_order));
    assert_eq!(second_after.next_order, None);

    assert!(svm.get_account(&rejected_order).is_none());
    assert_eq!(token_balance(&svm, quote_vault), vault_balance_before);
}

#[test]
fn second_ask_at_same_price_appends_to_fifo_queue() {
    let ActiveMarketFixture {
        mut svm,
        payer,
        market,
        base_mint,
        vault_authority,
        base_vault,
        ..
    } = setup_active_market(9, TEST_PRICE_TICK_SIZE, TEST_QUANTITY_LOT_SIZE);
    let first_collateral =
        create_test_token_account(&mut svm, base_mint, payer.pubkey(), TEST_ORDER_QUANTITY);
    let (first_order, first_result) = send_initial_limit_order_with_collateral(
        &mut svm,
        &payer,
        market,
        tidebook::state::OrderSide::Ask,
        TEST_ORDER_PRICE,
        TEST_ORDER_QUANTITY,
        base_mint,
        first_collateral,
        vault_authority,
        base_vault,
    );
    assert!(
        first_result.is_ok(),
        "first ask placement failed: {first_result:?}"
    );

    let (second_order, second_result) = send_append_limit_order(
        &mut svm,
        &payer,
        market,
        tidebook::state::OrderSide::Ask,
        TEST_ORDER_PRICE,
        TEST_ORDER_QUANTITY,
        first_order,
    );
    assert!(
        second_result.is_ok(),
        "second ask append failed: {second_result:?}"
    );

    let (price_level, _) = tidebook::derive_price_level_pda(
        &tidebook::id(),
        &market,
        tidebook::state::OrderSide::Ask,
        TEST_ORDER_PRICE,
    );
    let market_state = load_market(&svm, market);
    let level_state = load_price_level(&svm, price_level);
    let first_state = load_order(&svm, first_order);
    let second_state = load_order(&svm, second_order);

    assert_eq!(market_state.best_bid, None);
    assert_eq!(market_state.best_ask, Some(TEST_ORDER_PRICE));
    assert_eq!(market_state.next_order_id, 3);
    assert_eq!(market_state.open_order_count, 2);
    assert_eq!(level_state.first_order, Some(first_order));
    assert_eq!(level_state.last_order, Some(second_order));
    assert_eq!(level_state.order_count, 2);
    assert_eq!(
        level_state.total_remaining_quantity,
        TEST_ORDER_QUANTITY * 2
    );
    assert_eq!(first_state.previous_order, None);
    assert_eq!(first_state.next_order, Some(second_order));
    assert_eq!(second_state.previous_order, Some(first_order));
    assert_eq!(second_state.next_order, None);
    assert_eq!(token_balance(&svm, base_vault), TEST_ORDER_QUANTITY * 2);
}

#[test]
fn append_is_rejected_while_market_is_paused_without_mutation() {
    let ActiveMarketFixture {
        mut svm,
        payer,
        market,
        quote_vault,
        ..
    } = setup_active_market(9, TEST_PRICE_TICK_SIZE, TEST_QUANTITY_LOT_SIZE);
    let (first_order, first_result) = send_initial_limit_order(
        &mut svm,
        &payer,
        market,
        TEST_ORDER_PRICE,
        TEST_ORDER_QUANTITY,
    );
    assert!(
        first_result.is_ok(),
        "first bid placement failed: {first_result:?}"
    );
    assert!(
        send_pause_market(&mut svm, &payer, market).is_ok(),
        "market pause failed"
    );

    let (price_level, _) = tidebook::derive_price_level_pda(
        &tidebook::id(),
        &market,
        tidebook::state::OrderSide::Bid,
        TEST_ORDER_PRICE,
    );
    let level_before = load_price_level(&svm, price_level);
    let vault_balance_before = token_balance(&svm, quote_vault);

    let (rejected_order, rejected_result) = send_append_limit_order(
        &mut svm,
        &payer,
        market,
        tidebook::state::OrderSide::Bid,
        TEST_ORDER_PRICE,
        TEST_ORDER_QUANTITY,
        first_order,
    );
    assert!(
        rejected_result.is_err(),
        "append was accepted while the market was paused"
    );

    let market_after = load_market(&svm, market);
    let level_after = load_price_level(&svm, price_level);
    let first_after = load_order(&svm, first_order);
    assert_eq!(market_after.status, tidebook::state::MarketStatus::Paused);
    assert_eq!(market_after.next_order_id, 2);
    assert_eq!(market_after.open_order_count, 1);
    assert_eq!(level_after.first_order, level_before.first_order);
    assert_eq!(level_after.last_order, level_before.last_order);
    assert_eq!(level_after.order_count, level_before.order_count);
    assert_eq!(
        level_after.total_remaining_quantity,
        level_before.total_remaining_quantity
    );
    assert_eq!(first_after.previous_order, None);
    assert_eq!(first_after.next_order, None);
    assert!(svm.get_account(&rejected_order).is_none());
    assert_eq!(token_balance(&svm, quote_vault), vault_balance_before);
}

#[test]
fn insufficient_append_collateral_rolls_back_fifo_and_counters() {
    let ActiveMarketFixture {
        mut svm,
        payer,
        market,
        quote_mint,
        vault_authority,
        quote_vault,
        ..
    } = setup_active_market(9, TEST_PRICE_TICK_SIZE, TEST_QUANTITY_LOT_SIZE);
    let (first_order, first_result) = send_initial_limit_order(
        &mut svm,
        &payer,
        market,
        TEST_ORDER_PRICE,
        TEST_ORDER_QUANTITY,
    );
    assert!(
        first_result.is_ok(),
        "first bid placement failed: {first_result:?}"
    );

    let (price_level, _) = tidebook::derive_price_level_pda(
        &tidebook::id(),
        &market,
        tidebook::state::OrderSide::Bid,
        TEST_ORDER_PRICE,
    );
    let insufficient_amount = 499_999;
    let trader_collateral =
        create_test_token_account(&mut svm, quote_mint, payer.pubkey(), insufficient_amount);
    let level_before = load_price_level(&svm, price_level);
    let vault_balance_before = token_balance(&svm, quote_vault);

    let (rejected_order, rejected_result) = send_append_limit_order_with_accounts(
        &mut svm,
        &payer,
        market,
        tidebook::state::OrderSide::Bid,
        TEST_ORDER_PRICE,
        TEST_ORDER_QUANTITY,
        first_order,
        price_level,
        quote_mint,
        trader_collateral,
        vault_authority,
        quote_vault,
    );
    assert!(
        rejected_result.is_err(),
        "append with insufficient collateral was accepted"
    );

    let market_after = load_market(&svm, market);
    let level_after = load_price_level(&svm, price_level);
    let first_after = load_order(&svm, first_order);
    assert_eq!(market_after.next_order_id, 2);
    assert_eq!(market_after.open_order_count, 1);
    assert_eq!(level_after.first_order, level_before.first_order);
    assert_eq!(level_after.last_order, level_before.last_order);
    assert_eq!(level_after.order_count, level_before.order_count);
    assert_eq!(
        level_after.total_remaining_quantity,
        level_before.total_remaining_quantity
    );
    assert_eq!(first_after.previous_order, None);
    assert_eq!(first_after.next_order, None);
    assert!(svm.get_account(&rejected_order).is_none());
    assert_eq!(token_balance(&svm, trader_collateral), insufficient_amount);
    assert_eq!(token_balance(&svm, quote_vault), vault_balance_before);
}

#[test]
fn opposite_side_price_level_is_rejected_without_mutation() {
    let ActiveMarketFixture {
        mut svm,
        payer,
        market,
        base_mint,
        quote_mint,
        vault_authority,
        base_vault,
        quote_vault,
    } = setup_active_market(9, TEST_PRICE_TICK_SIZE, TEST_QUANTITY_LOT_SIZE);
    let (bid_order, bid_result) = send_initial_limit_order(
        &mut svm,
        &payer,
        market,
        TEST_ORDER_PRICE,
        TEST_ORDER_QUANTITY,
    );
    assert!(bid_result.is_ok(), "bid placement failed: {bid_result:?}");

    let ask_collateral =
        create_test_token_account(&mut svm, base_mint, payer.pubkey(), TEST_ORDER_QUANTITY);
    let (_, ask_result) = send_initial_limit_order_with_collateral(
        &mut svm,
        &payer,
        market,
        tidebook::state::OrderSide::Ask,
        TEST_ORDER_PRICE,
        TEST_ORDER_QUANTITY,
        base_mint,
        ask_collateral,
        vault_authority,
        base_vault,
    );
    assert!(ask_result.is_ok(), "ask placement failed: {ask_result:?}");

    let (bid_level, _) = tidebook::derive_price_level_pda(
        &tidebook::id(),
        &market,
        tidebook::state::OrderSide::Bid,
        TEST_ORDER_PRICE,
    );
    let (ask_level, _) = tidebook::derive_price_level_pda(
        &tidebook::id(),
        &market,
        tidebook::state::OrderSide::Ask,
        TEST_ORDER_PRICE,
    );
    let trader_collateral =
        create_test_token_account(&mut svm, quote_mint, payer.pubkey(), u64::MAX);
    let bid_level_before = load_price_level(&svm, bid_level);
    let ask_level_before = load_price_level(&svm, ask_level);
    let vault_balance_before = token_balance(&svm, quote_vault);

    let (rejected_order, rejected_result) = send_append_limit_order_with_accounts(
        &mut svm,
        &payer,
        market,
        tidebook::state::OrderSide::Bid,
        TEST_ORDER_PRICE,
        TEST_ORDER_QUANTITY,
        bid_order,
        ask_level,
        quote_mint,
        trader_collateral,
        vault_authority,
        quote_vault,
    );
    assert!(
        rejected_result.is_err(),
        "opposite-side price level was accepted"
    );

    let market_after = load_market(&svm, market);
    let bid_level_after = load_price_level(&svm, bid_level);
    let ask_level_after = load_price_level(&svm, ask_level);
    let bid_after = load_order(&svm, bid_order);
    assert_eq!(market_after.next_order_id, 3);
    assert_eq!(market_after.open_order_count, 2);
    assert_eq!(bid_level_after.first_order, bid_level_before.first_order);
    assert_eq!(bid_level_after.last_order, bid_level_before.last_order);
    assert_eq!(bid_level_after.order_count, bid_level_before.order_count);
    assert_eq!(ask_level_after.first_order, ask_level_before.first_order);
    assert_eq!(ask_level_after.last_order, ask_level_before.last_order);
    assert_eq!(ask_level_after.order_count, ask_level_before.order_count);
    assert_eq!(bid_after.previous_order, None);
    assert_eq!(bid_after.next_order, None);
    assert!(svm.get_account(&rejected_order).is_none());
    assert_eq!(token_balance(&svm, quote_vault), vault_balance_before);
}

#[test]
fn better_bid_becomes_new_best_and_links_previous_best_as_worse() {
    let ActiveMarketFixture {
        mut svm,
        payer,
        market,
        quote_vault,
        ..
    } = setup_active_market(9, TEST_PRICE_TICK_SIZE, TEST_QUANTITY_LOT_SIZE);
    let original_price = TEST_ORDER_PRICE;
    let better_price = TEST_ORDER_PRICE + TEST_PRICE_TICK_SIZE;

    let (original_order, original_result) = send_initial_limit_order(
        &mut svm,
        &payer,
        market,
        original_price,
        TEST_ORDER_QUANTITY,
    );
    assert!(
        original_result.is_ok(),
        "original bid placement failed: {original_result:?}"
    );
    let (original_level, _) = tidebook::derive_price_level_pda(
        &tidebook::id(),
        &market,
        tidebook::state::OrderSide::Bid,
        original_price,
    );

    let (new_order, insert_result) = send_insert_limit_order(
        &mut svm,
        &payer,
        market,
        tidebook::state::OrderSide::Bid,
        better_price,
        TEST_ORDER_QUANTITY,
        None,
        Some(original_level),
    );
    assert!(
        insert_result.is_ok(),
        "better bid insertion failed: {insert_result:?}"
    );

    let (new_level, expected_bump) = tidebook::derive_price_level_pda(
        &tidebook::id(),
        &market,
        tidebook::state::OrderSide::Bid,
        better_price,
    );
    let market_state = load_market(&svm, market);
    let new_level_state = load_price_level(&svm, new_level);
    let original_level_state = load_price_level(&svm, original_level);
    let original_order_state = load_order(&svm, original_order);
    let new_order_state = load_order(&svm, new_order);

    assert_eq!(market_state.best_bid, Some(better_price));
    assert_eq!(market_state.best_ask, None);
    assert_eq!(market_state.next_order_id, 3);
    assert_eq!(market_state.open_order_count, 2);

    assert_eq!(new_level_state.market, market);
    assert_eq!(new_level_state.side, tidebook::state::OrderSide::Bid);
    assert_eq!(new_level_state.price, better_price);
    assert_eq!(new_level_state.better_price, None);
    assert_eq!(new_level_state.worse_price, Some(original_price));
    assert_eq!(new_level_state.first_order, Some(new_order));
    assert_eq!(new_level_state.last_order, Some(new_order));
    assert_eq!(
        new_level_state.total_remaining_quantity,
        TEST_ORDER_QUANTITY
    );
    assert_eq!(new_level_state.order_count, 1);
    assert_eq!(new_level_state.rent_payer, payer.pubkey());
    assert_eq!(new_level_state.bump, expected_bump);

    assert_eq!(original_level_state.better_price, Some(better_price));
    assert_eq!(original_level_state.worse_price, None);
    assert_eq!(original_level_state.first_order, Some(original_order));
    assert_eq!(original_level_state.last_order, Some(original_order));
    assert_eq!(original_level_state.order_count, 1);

    assert_eq!(original_order_state.price_level, original_level);
    assert_eq!(original_order_state.previous_order, None);
    assert_eq!(original_order_state.next_order, None);
    assert_eq!(new_order_state.price_level, new_level);
    assert_eq!(new_order_state.previous_order, None);
    assert_eq!(new_order_state.next_order, None);
    assert_eq!(
        token_balance(&svm, quote_vault),
        original_order_state.locked_collateral + new_order_state.locked_collateral
    );
}

#[test]
fn better_ask_becomes_new_best_and_links_previous_best_as_worse() {
    let ActiveMarketFixture {
        mut svm,
        payer,
        market,
        base_mint,
        vault_authority,
        base_vault,
        ..
    } = setup_active_market(9, TEST_PRICE_TICK_SIZE, TEST_QUANTITY_LOT_SIZE);
    let original_price = TEST_ORDER_PRICE;
    let better_price = TEST_ORDER_PRICE - TEST_PRICE_TICK_SIZE;
    let trader_base =
        create_test_token_account(&mut svm, base_mint, payer.pubkey(), TEST_ORDER_QUANTITY);

    let (_, original_result) = send_initial_limit_order_with_collateral(
        &mut svm,
        &payer,
        market,
        tidebook::state::OrderSide::Ask,
        original_price,
        TEST_ORDER_QUANTITY,
        base_mint,
        trader_base,
        vault_authority,
        base_vault,
    );
    assert!(
        original_result.is_ok(),
        "original ask placement failed: {original_result:?}"
    );
    let (original_level, _) = tidebook::derive_price_level_pda(
        &tidebook::id(),
        &market,
        tidebook::state::OrderSide::Ask,
        original_price,
    );

    let (new_order, insert_result) = send_insert_limit_order(
        &mut svm,
        &payer,
        market,
        tidebook::state::OrderSide::Ask,
        better_price,
        TEST_ORDER_QUANTITY,
        None,
        Some(original_level),
    );
    assert!(
        insert_result.is_ok(),
        "better ask insertion failed: {insert_result:?}"
    );

    let (new_level, _) = tidebook::derive_price_level_pda(
        &tidebook::id(),
        &market,
        tidebook::state::OrderSide::Ask,
        better_price,
    );
    let market_state = load_market(&svm, market);
    let new_level_state = load_price_level(&svm, new_level);
    let original_level_state = load_price_level(&svm, original_level);
    let new_order_state = load_order(&svm, new_order);

    assert_eq!(market_state.best_bid, None);
    assert_eq!(market_state.best_ask, Some(better_price));
    assert_eq!(market_state.next_order_id, 3);
    assert_eq!(market_state.open_order_count, 2);
    assert_eq!(new_level_state.better_price, None);
    assert_eq!(new_level_state.worse_price, Some(original_price));
    assert_eq!(new_level_state.first_order, Some(new_order));
    assert_eq!(new_level_state.last_order, Some(new_order));
    assert_eq!(original_level_state.better_price, Some(better_price));
    assert_eq!(original_level_state.worse_price, None);
    assert_eq!(new_order_state.price_level, new_level);
    assert_eq!(new_order_state.locked_collateral, TEST_ORDER_QUANTITY);
    assert_eq!(token_balance(&svm, base_vault), TEST_ORDER_QUANTITY * 2);
}

#[test]
fn empty_side_rejects_an_unexpected_neighbor_without_mutation() {
    let ActiveMarketFixture {
        mut svm,
        payer,
        market,
        ..
    } = setup_active_market(9, TEST_PRICE_TICK_SIZE, TEST_QUANTITY_LOT_SIZE);

    let (_, bid_result) = send_initial_limit_order(
        &mut svm,
        &payer,
        market,
        TEST_ORDER_PRICE,
        TEST_ORDER_QUANTITY,
    );
    assert!(bid_result.is_ok(), "bid placement failed: {bid_result:?}");
    let (bid_level, _) = tidebook::derive_price_level_pda(
        &tidebook::id(),
        &market,
        tidebook::state::OrderSide::Bid,
        TEST_ORDER_PRICE,
    );
    let rejected_ask_price = TEST_ORDER_PRICE - TEST_PRICE_TICK_SIZE;

    let (rejected_order, result) = send_insert_limit_order(
        &mut svm,
        &payer,
        market,
        tidebook::state::OrderSide::Ask,
        rejected_ask_price,
        TEST_ORDER_QUANTITY,
        None,
        Some(bid_level),
    );

    assert!(result.is_err(), "empty ask side accepted a worse neighbor");
    let market_state = load_market(&svm, market);
    assert_eq!(market_state.best_bid, Some(TEST_ORDER_PRICE));
    assert_eq!(market_state.best_ask, None);
    assert_eq!(market_state.next_order_id, 2);
    assert_eq!(market_state.open_order_count, 1);
    assert!(svm.get_account(&rejected_order).is_none());
}

#[test]
fn bid_levels_support_middle_and_new_worst_insertion() {
    let ActiveMarketFixture {
        mut svm,
        payer,
        market,
        ..
    } = setup_active_market(9, TEST_PRICE_TICK_SIZE, TEST_QUANTITY_LOT_SIZE);
    let side = tidebook::state::OrderSide::Bid;
    let first_price = TEST_ORDER_PRICE;
    let best_price = first_price + 3 * TEST_PRICE_TICK_SIZE;
    let middle_price = first_price + 2 * TEST_PRICE_TICK_SIZE;
    let worst_price = first_price - TEST_PRICE_TICK_SIZE;

    let (_, first_level) =
        insert_level_successfully(&mut svm, &payer, market, side, first_price, None, None);
    let (_, best_level) = insert_level_successfully(
        &mut svm,
        &payer,
        market,
        side,
        best_price,
        None,
        Some(first_level),
    );
    let (middle_order, middle_level) = insert_level_successfully(
        &mut svm,
        &payer,
        market,
        side,
        middle_price,
        Some(best_level),
        Some(first_level),
    );
    let (worst_order, worst_level) = insert_level_successfully(
        &mut svm,
        &payer,
        market,
        side,
        worst_price,
        Some(first_level),
        None,
    );

    let market_state = load_market(&svm, market);
    let best = load_price_level(&svm, best_level);
    let middle = load_price_level(&svm, middle_level);
    let first = load_price_level(&svm, first_level);
    let worst = load_price_level(&svm, worst_level);

    assert_eq!(market_state.best_bid, Some(best_price));
    assert_eq!(market_state.next_order_id, 5);
    assert_eq!(market_state.open_order_count, 4);
    assert_eq!(best.better_price, None);
    assert_eq!(best.worse_price, Some(middle_price));
    assert_eq!(middle.better_price, Some(best_price));
    assert_eq!(middle.worse_price, Some(first_price));
    assert_eq!(first.better_price, Some(middle_price));
    assert_eq!(first.worse_price, Some(worst_price));
    assert_eq!(worst.better_price, Some(first_price));
    assert_eq!(worst.worse_price, None);
    assert_eq!(load_order(&svm, middle_order).price_level, middle_level);
    assert_eq!(load_order(&svm, worst_order).price_level, worst_level);
}

#[test]
fn ask_levels_support_middle_and_new_worst_insertion() {
    let ActiveMarketFixture {
        mut svm,
        payer,
        market,
        ..
    } = setup_active_market(9, TEST_PRICE_TICK_SIZE, TEST_QUANTITY_LOT_SIZE);
    let side = tidebook::state::OrderSide::Ask;
    let first_price = TEST_ORDER_PRICE;
    let best_price = first_price - 3 * TEST_PRICE_TICK_SIZE;
    let middle_price = first_price - 2 * TEST_PRICE_TICK_SIZE;
    let worst_price = first_price + TEST_PRICE_TICK_SIZE;

    let (_, first_level) =
        insert_level_successfully(&mut svm, &payer, market, side, first_price, None, None);
    let (_, best_level) = insert_level_successfully(
        &mut svm,
        &payer,
        market,
        side,
        best_price,
        None,
        Some(first_level),
    );
    let (middle_order, middle_level) = insert_level_successfully(
        &mut svm,
        &payer,
        market,
        side,
        middle_price,
        Some(best_level),
        Some(first_level),
    );
    let (worst_order, worst_level) = insert_level_successfully(
        &mut svm,
        &payer,
        market,
        side,
        worst_price,
        Some(first_level),
        None,
    );

    let market_state = load_market(&svm, market);
    let best = load_price_level(&svm, best_level);
    let middle = load_price_level(&svm, middle_level);
    let first = load_price_level(&svm, first_level);
    let worst = load_price_level(&svm, worst_level);

    assert_eq!(market_state.best_ask, Some(best_price));
    assert_eq!(market_state.next_order_id, 5);
    assert_eq!(market_state.open_order_count, 4);
    assert_eq!(best.better_price, None);
    assert_eq!(best.worse_price, Some(middle_price));
    assert_eq!(middle.better_price, Some(best_price));
    assert_eq!(middle.worse_price, Some(first_price));
    assert_eq!(first.better_price, Some(middle_price));
    assert_eq!(first.worse_price, Some(worst_price));
    assert_eq!(worst.better_price, Some(first_price));
    assert_eq!(worst.worse_price, None);
    assert_eq!(load_order(&svm, middle_order).price_level, middle_level);
    assert_eq!(load_order(&svm, worst_order).price_level, worst_level);
}

#[test]
fn invalid_middle_and_worst_neighbors_roll_back_atomically() {
    let ActiveMarketFixture {
        mut svm,
        payer,
        market,
        quote_vault,
        ..
    } = setup_active_market(9, TEST_PRICE_TICK_SIZE, TEST_QUANTITY_LOT_SIZE);
    let side = tidebook::state::OrderSide::Bid;
    let first_price = TEST_ORDER_PRICE;
    let best_price = first_price + 3 * TEST_PRICE_TICK_SIZE;
    let worst_price = first_price - TEST_PRICE_TICK_SIZE;

    let (_, first_level) =
        insert_level_successfully(&mut svm, &payer, market, side, first_price, None, None);
    let (_, best_level) = insert_level_successfully(
        &mut svm,
        &payer,
        market,
        side,
        best_price,
        None,
        Some(first_level),
    );
    let (_, worst_level) = insert_level_successfully(
        &mut svm,
        &payer,
        market,
        side,
        worst_price,
        Some(first_level),
        None,
    );

    let vault_before = token_balance(&svm, quote_vault);

    // These levels are canonical but not adjacent because `first_level` sits
    // between them. The reciprocal-link check must reject the skipped node.
    let skipped_price = first_price + 2 * TEST_PRICE_TICK_SIZE;
    let (skipped_order, skipped_result) = send_insert_limit_order(
        &mut svm,
        &payer,
        market,
        side,
        skipped_price,
        TEST_ORDER_QUANTITY,
        Some(best_level),
        Some(worst_level),
    );
    assert!(
        skipped_result.is_err(),
        "nonadjacent neighbors were accepted"
    );

    // The supplied levels are adjacent, but the proposed price is outside
    // their strict interval and therefore cannot be placed between them.
    let unordered_price = first_price - 2 * TEST_PRICE_TICK_SIZE;
    let (unordered_order, unordered_result) = send_insert_limit_order(
        &mut svm,
        &payer,
        market,
        side,
        unordered_price,
        TEST_ORDER_QUANTITY,
        Some(best_level),
        Some(first_level),
    );
    assert!(
        unordered_result.is_err(),
        "out-of-range middle price was accepted"
    );

    // A new-worst insertion must extend the actual terminal node, not merely
    // any canonical level with a lower-priority proposed price.
    let false_worst_price = first_price - 3 * TEST_PRICE_TICK_SIZE;
    let (false_worst_order, false_worst_result) = send_insert_limit_order(
        &mut svm,
        &payer,
        market,
        side,
        false_worst_price,
        TEST_ORDER_QUANTITY,
        Some(first_level),
        None,
    );
    assert!(
        false_worst_result.is_err(),
        "nonterminal level was accepted as the worst boundary"
    );

    let market_state = load_market(&svm, market);
    let best = load_price_level(&svm, best_level);
    let first = load_price_level(&svm, first_level);
    let worst = load_price_level(&svm, worst_level);
    assert_eq!(market_state.best_bid, Some(best_price));
    assert_eq!(market_state.next_order_id, 4);
    assert_eq!(market_state.open_order_count, 3);
    assert_eq!(best.worse_price, Some(first_price));
    assert_eq!(first.better_price, Some(best_price));
    assert_eq!(first.worse_price, Some(worst_price));
    assert_eq!(worst.better_price, Some(first_price));
    assert_eq!(worst.worse_price, None);
    assert_eq!(token_balance(&svm, quote_vault), vault_before);
    assert!(svm.get_account(&skipped_order).is_none());
    assert!(svm.get_account(&unordered_order).is_none());
    assert!(svm.get_account(&false_worst_order).is_none());
}

#[test]
fn insert_rejects_neighbors_from_wrong_side_and_market() {
    let ActiveMarketFixture {
        mut svm,
        payer,
        market,
        quote_vault,
        ..
    } = setup_active_market(9, TEST_PRICE_TICK_SIZE, TEST_QUANTITY_LOT_SIZE);
    let bid = tidebook::state::OrderSide::Bid;
    let ask = tidebook::state::OrderSide::Ask;

    let (_, bid_level) =
        insert_level_successfully(&mut svm, &payer, market, bid, TEST_ORDER_PRICE, None, None);
    let (_, ask_level) =
        insert_level_successfully(&mut svm, &payer, market, ask, TEST_ORDER_PRICE, None, None);
    let vault_before = token_balance(&svm, quote_vault);

    let wrong_side_price = TEST_ORDER_PRICE - TEST_PRICE_TICK_SIZE;
    let (wrong_side_order, wrong_side_result) = send_insert_limit_order(
        &mut svm,
        &payer,
        market,
        bid,
        wrong_side_price,
        TEST_ORDER_QUANTITY,
        Some(ask_level),
        None,
    );
    assert!(
        wrong_side_result.is_err(),
        "ask level was accepted on bid side"
    );

    // Create a second valid market and level. Valid ownership and serialization
    // are not enough: a neighbor is scoped to exactly one market-side list.
    let other_base = create_test_mint(&mut svm, 9);
    let other_quote = create_test_mint(&mut svm, 6);
    let (other_market, initialize_result) =
        send_initialize_market(&mut svm, &payer, other_base, other_quote);
    assert!(initialize_result.is_ok(), "second market setup failed");
    let (_, other_level) = insert_level_successfully(
        &mut svm,
        &payer,
        other_market,
        bid,
        TEST_ORDER_PRICE,
        None,
        None,
    );

    let wrong_market_price = TEST_ORDER_PRICE - 2 * TEST_PRICE_TICK_SIZE;
    let (wrong_market_order, wrong_market_result) = send_insert_limit_order(
        &mut svm,
        &payer,
        market,
        bid,
        wrong_market_price,
        TEST_ORDER_QUANTITY,
        Some(other_level),
        None,
    );
    assert!(
        wrong_market_result.is_err(),
        "level from another market was accepted"
    );

    let market_state = load_market(&svm, market);
    let bid_state = load_price_level(&svm, bid_level);
    assert_eq!(market_state.best_bid, Some(TEST_ORDER_PRICE));
    assert_eq!(market_state.best_ask, Some(TEST_ORDER_PRICE));
    assert_eq!(market_state.next_order_id, 3);
    assert_eq!(market_state.open_order_count, 2);
    assert_eq!(bid_state.better_price, None);
    assert_eq!(bid_state.worse_price, None);
    assert_eq!(token_balance(&svm, quote_vault), vault_before);
    assert!(svm.get_account(&wrong_side_order).is_none());
    assert!(svm.get_account(&wrong_market_order).is_none());
}

#[test]
fn insert_rejects_noncanonical_neighbor_atomically() {
    let ActiveMarketFixture {
        mut svm,
        payer,
        market,
        quote_vault,
        ..
    } = setup_active_market(9, TEST_PRICE_TICK_SIZE, TEST_QUANTITY_LOT_SIZE);
    let side = tidebook::state::OrderSide::Bid;
    let (_, best_level) =
        insert_level_successfully(&mut svm, &payer, market, side, TEST_ORDER_PRICE, None, None);
    let vault_before = token_balance(&svm, quote_vault);

    // The data looks like a terminal level owned by Tidebook, but its address
    // is not derived from (market, side, price). The handler must not trust it.
    let fake_level = Pubkey::new_unique();
    let fake_price = TEST_ORDER_PRICE - TEST_PRICE_TICK_SIZE;
    store_price_level_at(
        &mut svm,
        fake_level,
        tidebook::state::PriceLevel {
            market,
            side,
            price: fake_price,
            better_price: Some(TEST_ORDER_PRICE),
            worse_price: None,
            first_order: None,
            last_order: None,
            total_remaining_quantity: 0,
            order_count: 0,
            rent_payer: payer.pubkey(),
            bump: 0,
        },
    );

    let rejected_price = fake_price - TEST_PRICE_TICK_SIZE;
    let (rejected_order, result) = send_insert_limit_order(
        &mut svm,
        &payer,
        market,
        side,
        rejected_price,
        TEST_ORDER_QUANTITY,
        Some(fake_level),
        None,
    );
    assert!(result.is_err(), "noncanonical neighbor was accepted");

    let market_state = load_market(&svm, market);
    let best = load_price_level(&svm, best_level);
    assert_eq!(market_state.best_bid, Some(TEST_ORDER_PRICE));
    assert_eq!(market_state.next_order_id, 2);
    assert_eq!(market_state.open_order_count, 1);
    assert_eq!(best.better_price, None);
    assert_eq!(best.worse_price, None);
    assert_eq!(token_balance(&svm, quote_vault), vault_before);
    assert!(svm.get_account(&rejected_order).is_none());
}

#[test]
fn edge_insertions_reject_wrong_priority_and_stale_best_atomically() {
    let ActiveMarketFixture {
        mut svm,
        payer,
        market,
        quote_vault,
        ..
    } = setup_active_market(9, TEST_PRICE_TICK_SIZE, TEST_QUANTITY_LOT_SIZE);
    let side = tidebook::state::OrderSide::Bid;
    let first_price = TEST_ORDER_PRICE;
    let best_price = first_price + 2 * TEST_PRICE_TICK_SIZE;

    let (_, first_level) =
        insert_level_successfully(&mut svm, &payer, market, side, first_price, None, None);
    let (_, best_level) = insert_level_successfully(
        &mut svm,
        &payer,
        market,
        side,
        best_price,
        None,
        Some(first_level),
    );
    let vault_before = token_balance(&svm, quote_vault);

    // The original level is the terminal node, but a numerically higher bid
    // cannot be appended as the new worst; it belongs between the two levels.
    let middle_price = first_price + TEST_PRICE_TICK_SIZE;
    let (rejected_order, wrong_worst_result) = send_insert_limit_order(
        &mut svm,
        &payer,
        market,
        side,
        middle_price,
        TEST_ORDER_QUANTITY,
        Some(first_level),
        None,
    );
    assert!(
        wrong_worst_result.is_err(),
        "higher-priority bid was accepted as the new worst"
    );

    // Supplying the actual best as the worse neighbor still does not make a
    // lower-priority price a valid new head.
    let (_, wrong_best_result) = send_insert_limit_order(
        &mut svm,
        &payer,
        market,
        side,
        middle_price,
        TEST_ORDER_QUANTITY,
        None,
        Some(best_level),
    );
    assert!(
        wrong_best_result.is_err(),
        "lower-priority bid was accepted as the new best"
    );

    // A genuine better price must still name the current head, not an older
    // canonical level left elsewhere in the same list.
    let better_price = best_price + TEST_PRICE_TICK_SIZE;
    let (_, stale_best_result) = send_insert_limit_order(
        &mut svm,
        &payer,
        market,
        side,
        better_price,
        TEST_ORDER_QUANTITY,
        None,
        Some(first_level),
    );
    assert!(
        stale_best_result.is_err(),
        "stale level was accepted as the current best"
    );

    let market_state = load_market(&svm, market);
    let best = load_price_level(&svm, best_level);
    let first = load_price_level(&svm, first_level);
    assert_eq!(market_state.best_bid, Some(best_price));
    assert_eq!(market_state.next_order_id, 3);
    assert_eq!(market_state.open_order_count, 2);
    assert_eq!(best.better_price, None);
    assert_eq!(best.worse_price, Some(first_price));
    assert_eq!(first.better_price, Some(best_price));
    assert_eq!(first.worse_price, None);
    assert_eq!(token_balance(&svm, quote_vault), vault_before);
    assert!(svm.get_account(&rejected_order).is_none());
}
