//! LiteSVM integration coverage for market creation, canonical vaults, market
//! lifecycle transitions, and limit-order validation.

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

fn send_place_limit_order(
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

    send_place_limit_order_with_collateral(
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
fn send_place_limit_order_with_collateral(
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
        &tidebook::instruction::PlaceLimitOrder {
            side,
            price,
            quantity,
        }
        .data(),
        tidebook::accounts::PlaceLimitOrder {
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

    let (order, result) = send_place_limit_order(
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

    let (order, result) = send_place_limit_order_with_collateral(
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

    let (order, result) = send_place_limit_order_with_collateral(
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

    let (order, result) = send_place_limit_order_with_collateral(
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

    let (order, result) = send_place_limit_order_with_collateral(
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

    let (order, result) = send_place_limit_order_with_collateral(
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

    let (order, result) = send_place_limit_order_with_collateral(
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

    let (order, result) = send_place_limit_order_with_collateral(
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

    let (order, result) = send_place_limit_order(
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

    let (order, result) = send_place_limit_order(
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

    let (order, result) = send_place_limit_order(&mut svm, &payer, market, 1, 1);

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

    let (order, result) = send_place_limit_order(
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

    let (order, result) = send_place_limit_order_with_collateral(
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
        send_place_limit_order(&mut svm, &payer, market, first_price, TEST_ORDER_QUANTITY);
    assert!(
        first_result.is_ok(),
        "first bid placement failed: {first_result:?}"
    );

    let (second_order, second_result) =
        send_place_limit_order(&mut svm, &payer, market, second_price, TEST_ORDER_QUANTITY);

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

    let (first_order, first_result) = send_place_limit_order(
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

    let (first_order, first_result) = send_place_limit_order(
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

    let (first_order, first_result) = send_place_limit_order(
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
    let (first_order, first_result) = send_place_limit_order_with_collateral(
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
    let (first_order, first_result) = send_place_limit_order(
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
    let (first_order, first_result) = send_place_limit_order(
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
    let (bid_order, bid_result) = send_place_limit_order(
        &mut svm,
        &payer,
        market,
        TEST_ORDER_PRICE,
        TEST_ORDER_QUANTITY,
    );
    assert!(bid_result.is_ok(), "bid placement failed: {bid_result:?}");

    let ask_collateral =
        create_test_token_account(&mut svm, base_mint, payer.pubkey(), TEST_ORDER_QUANTITY);
    let (_, ask_result) = send_place_limit_order_with_collateral(
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
