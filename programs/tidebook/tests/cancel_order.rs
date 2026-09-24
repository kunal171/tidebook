//! LiteSVM integration coverage for owner-authorized order cancellation and
//! collateral refunds, including cancellation while a market is paused.

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
    tidebook::state::{
        AdminRecord, AdminStatus, Market, MarketStatus, Order, OrderSide, OrderStatus, PriceLevel,
    },
};

const PROGRAM_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../target/deploy/tidebook.so"
));

const ORDER_ID: u64 = 1;
const TEST_PRICE_TICK_SIZE: u64 = 10_000;
const TEST_QUANTITY_LOT_SIZE: u64 = 1_000_000;
const TEST_ORDER_PRICE: u64 = 100_000_000;
const TEST_ORDER_QUANTITY: u64 = 5_000_000;
fn trader_balance_address(market: Pubkey, owner: Pubkey) -> Pubkey {
    tidebook::derive_trader_balance_pda(&tidebook::id(), &market, &owner).0
}

fn ensure_trader_balance(svm: &mut LiteSVM, market: Pubkey, owner: Pubkey) -> Pubkey {
    let (address, bump) = tidebook::derive_trader_balance_pda(&tidebook::id(), &market, &owner);

    if svm.get_account(&address).is_none() {
        let state = tidebook::state::TraderBalance {
            market,
            owner,
            base_free: u64::MAX / 4,
            base_locked: 0,
            quote_free: u64::MAX / 4,
            quote_locked: 0,
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

    address
}

const TEST_QUOTE_COLLATERAL: u64 = 500_000;

struct MarketFixture {
    market: Pubkey,
    base_mint: Pubkey,
    quote_mint: Pubkey,
    vault_authority: Pubkey,
    base_vault: Pubkey,
    quote_vault: Pubkey,
}

struct DistinctBidOrder {
    order: Pubkey,
    price_level: Pubkey,
    owner_collateral: Pubkey,
    locked_collateral: u64,
}

struct OpenOrderFixture {
    svm: LiteSVM,
    owner: Keypair,
    market: MarketFixture,
    order: Pubkey,
    price_level: Pubkey,
    owner_collateral: Pubkey,
    collateral_mint: Pubkey,
    market_vault: Pubkey,
    locked_collateral: u64,
}

fn send_instruction(
    svm: &mut LiteSVM,
    payer: &Keypair,
    instruction: Instruction,
) -> litesvm::types::TransactionResult {
    svm.expire_blockhash();
    let message = Message::new_with_blockhash(
        &[instruction],
        Some(&payer.pubkey()),
        &svm.latest_blockhash(),
    );
    let transaction =
        VersionedTransaction::try_new(VersionedMessage::Legacy(message), &[payer]).unwrap();

    svm.send_transaction(transaction)
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

fn store_active_admin(svm: &mut LiteSVM, authority: Pubkey) {
    let (address, bump) = Pubkey::find_program_address(
        &[tidebook::constants::ADMIN_SEED, authority.as_ref()],
        &tidebook::id(),
    );
    let state = AdminRecord {
        authority,
        added_by: authority,
        status: AdminStatus::Active,
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

fn initialize_market(svm: &mut LiteSVM, authority: &Keypair) -> MarketFixture {
    let base_mint = create_test_mint(svm, 9);
    let quote_mint = create_test_mint(svm, 6);
    let (admin_record, _) = Pubkey::find_program_address(
        &[tidebook::constants::ADMIN_SEED, authority.pubkey().as_ref()],
        &tidebook::id(),
    );
    let (market, _) = Pubkey::find_program_address(
        &[
            tidebook::constants::MARKET_SEED,
            base_mint.as_ref(),
            quote_mint.as_ref(),
        ],
        &tidebook::id(),
    );
    let (vault_authority, _) = Pubkey::find_program_address(
        &[tidebook::constants::VAULT_AUTHORITY_SEED, market.as_ref()],
        &tidebook::id(),
    );
    let (base_vault, _) = Pubkey::find_program_address(
        &[
            tidebook::constants::VAULT_SEED,
            market.as_ref(),
            base_mint.as_ref(),
        ],
        &tidebook::id(),
    );
    let (quote_vault, _) = Pubkey::find_program_address(
        &[
            tidebook::constants::VAULT_SEED,
            market.as_ref(),
            quote_mint.as_ref(),
        ],
        &tidebook::id(),
    );
    let instruction = Instruction::new_with_bytes(
        tidebook::id(),
        &tidebook::instruction::InitializeMarket {
            price_tick_size: TEST_PRICE_TICK_SIZE,
            quantity_lot_size: TEST_QUANTITY_LOT_SIZE,
        }
        .data(),
        tidebook::accounts::InitializeMarket {
            authority: authority.pubkey(),
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

    let result = send_instruction(svm, authority, instruction);
    assert!(result.is_ok(), "market initialization failed: {result:?}");
    MarketFixture {
        market,
        base_mint,
        quote_mint,
        vault_authority,
        base_vault,
        quote_vault,
    }
}

fn place_order(
    svm: &mut LiteSVM,
    owner: &Keypair,
    market_fixture: &MarketFixture,
    order_id: u64,
    side: OrderSide,
) -> (Pubkey, Pubkey, Pubkey, Pubkey, u64) {
    let (collateral_mint, market_vault, locked_collateral) = match side {
        OrderSide::Bid => (
            market_fixture.quote_mint,
            market_fixture.quote_vault,
            TEST_QUOTE_COLLATERAL,
        ),
        OrderSide::Ask => (
            market_fixture.base_mint,
            market_fixture.base_vault,
            TEST_ORDER_QUANTITY,
        ),
    };
    let trader_collateral =
        create_test_token_account(svm, collateral_mint, owner.pubkey(), locked_collateral);
    let (order, _) = Pubkey::find_program_address(
        &[
            tidebook::constants::ORDER_SEED,
            market_fixture.market.as_ref(),
            order_id.to_le_bytes().as_ref(),
        ],
        &tidebook::id(),
    );
    let (price_level, _) = tidebook::derive_price_level_pda(
        &tidebook::id(),
        &market_fixture.market,
        side,
        TEST_ORDER_PRICE,
    );
    let instruction = Instruction::new_with_bytes(
        tidebook::id(),
        &tidebook::instruction::InsertLimitOrder {
            side,
            price: TEST_ORDER_PRICE,
            quantity: TEST_ORDER_QUANTITY,
        }
        .data(),
        tidebook::accounts::InsertLimitOrder {
            trader: owner.pubkey(),
            market: market_fixture.market,
            order,
            price_level,
            trader_balance: ensure_trader_balance(svm, market_fixture.market, owner.pubkey()),
            system_program: system_program::ID,
            better_level: None,
            worse_level: None,
        }
        .to_account_metas(None),
    );

    let result = send_instruction(svm, owner, instruction);
    assert!(result.is_ok(), "order placement failed: {result:?}");
    (
        order,
        trader_collateral,
        collateral_mint,
        market_vault,
        locked_collateral,
    )
}

fn append_bid_order(
    fixture: &mut OpenOrderFixture,
    order_id: u64,
    previous_order: Pubkey,
) -> (Pubkey, Pubkey) {
    let trader_collateral = create_test_token_account(
        &mut fixture.svm,
        fixture.market.quote_mint,
        fixture.owner.pubkey(),
        TEST_QUOTE_COLLATERAL,
    );
    let (order, _) = Pubkey::find_program_address(
        &[
            tidebook::constants::ORDER_SEED,
            fixture.market.market.as_ref(),
            order_id.to_le_bytes().as_ref(),
        ],
        &tidebook::id(),
    );
    let instruction = Instruction::new_with_bytes(
        tidebook::id(),
        &tidebook::instruction::AppendLimitOrder {
            side: OrderSide::Bid,
            price: TEST_ORDER_PRICE,
            quantity: TEST_ORDER_QUANTITY,
        }
        .data(),
        tidebook::accounts::AppendLimitOrder {
            trader: fixture.owner.pubkey(),
            market: fixture.market.market,
            order,
            price_level: fixture.price_level,
            previous_order,
            trader_balance: ensure_trader_balance(
                &mut fixture.svm,
                fixture.market.market,
                fixture.owner.pubkey(),
            ),
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    );

    let result = send_instruction(&mut fixture.svm, &fixture.owner, instruction);
    assert!(result.is_ok(), "FIFO append failed: {result:?}");

    (order, trader_collateral)
}

fn insert_bid_level(
    fixture: &mut OpenOrderFixture,
    order_id: u64,
    price: u64,
    better_level: Option<Pubkey>,
    worse_level: Option<Pubkey>,
) -> DistinctBidOrder {
    let locked_collateral =
        u64::try_from(u128::from(price) * u128::from(TEST_ORDER_QUANTITY) / 1_000_000_000_u128)
            .unwrap();
    let owner_collateral = create_test_token_account(
        &mut fixture.svm,
        fixture.market.quote_mint,
        fixture.owner.pubkey(),
        locked_collateral,
    );
    let (order, _) = Pubkey::find_program_address(
        &[
            tidebook::constants::ORDER_SEED,
            fixture.market.market.as_ref(),
            order_id.to_le_bytes().as_ref(),
        ],
        &tidebook::id(),
    );
    let price_level = tidebook::derive_price_level_pda(
        &tidebook::id(),
        &fixture.market.market,
        OrderSide::Bid,
        price,
    )
    .0;
    let instruction = Instruction::new_with_bytes(
        tidebook::id(),
        &tidebook::instruction::InsertLimitOrder {
            side: OrderSide::Bid,
            price,
            quantity: TEST_ORDER_QUANTITY,
        }
        .data(),
        tidebook::accounts::InsertLimitOrder {
            trader: fixture.owner.pubkey(),
            market: fixture.market.market,
            order,
            price_level,
            trader_balance: ensure_trader_balance(
                &mut fixture.svm,
                fixture.market.market,
                fixture.owner.pubkey(),
            ),
            system_program: system_program::ID,
            better_level,
            worse_level,
        }
        .to_account_metas(None),
    );

    let result = send_instruction(&mut fixture.svm, &fixture.owner, instruction);
    assert!(result.is_ok(), "price-level insertion failed: {result:?}");

    DistinctBidOrder {
        order,
        price_level,
        owner_collateral,
        locked_collateral,
    }
}

#[allow(clippy::too_many_arguments)]
fn cancel_order_instruction_with_accounts(
    signer: Pubkey,
    market: Pubkey,
    order: Pubkey,
    order_id: u64,
    price_level: Pubkey,
    _collateral_mint: Pubkey,
    _owner_collateral: Pubkey,
    _vault_authority: Pubkey,
    _market_vault: Pubkey,
) -> Instruction {
    Instruction::new_with_bytes(
        tidebook::id(),
        &tidebook::instruction::CancelLimitOrder { order_id }.data(),
        tidebook::accounts::CancelLimitOrder {
            owner: signer,
            market,
            order,
            price_level,
            previous_order: None,
            next_order: None,
            better_level: None,
            worse_level: None,
            level_rent_recipient: Some(signer),
            trader_balance: trader_balance_address(market, signer),
        }
        .to_account_metas(None),
    )
}

fn cancel_order_instruction(fixture: &OpenOrderFixture, signer: Pubkey) -> Instruction {
    cancel_order_instruction_with_accounts(
        signer,
        fixture.market.market,
        fixture.order,
        ORDER_ID,
        fixture.price_level,
        fixture.collateral_mint,
        fixture.owner_collateral,
        fixture.market.vault_authority,
        fixture.market_vault,
    )
}

fn cancel_queued_order_instruction(
    fixture: &OpenOrderFixture,
    order: Pubkey,
    order_id: u64,
    previous_order: Option<Pubkey>,
    next_order: Option<Pubkey>,
    _owner_collateral: Pubkey,
) -> Instruction {
    Instruction::new_with_bytes(
        tidebook::id(),
        &tidebook::instruction::CancelLimitOrder { order_id }.data(),
        tidebook::accounts::CancelLimitOrder {
            owner: fixture.owner.pubkey(),
            market: fixture.market.market,
            order,
            price_level: fixture.price_level,
            previous_order,
            next_order,
            better_level: None,
            worse_level: None,
            level_rent_recipient: None,
            trader_balance: trader_balance_address(fixture.market.market, fixture.owner.pubkey()),
        }
        .to_account_metas(None),
    )
}

fn cancel_distinct_bid_level_instruction(
    fixture: &OpenOrderFixture,
    order: &DistinctBidOrder,
    order_id: u64,
    better_level: Option<Pubkey>,
    worse_level: Option<Pubkey>,
) -> Instruction {
    Instruction::new_with_bytes(
        tidebook::id(),
        &tidebook::instruction::CancelLimitOrder { order_id }.data(),
        tidebook::accounts::CancelLimitOrder {
            owner: fixture.owner.pubkey(),
            market: fixture.market.market,
            order: order.order,
            price_level: order.price_level,
            previous_order: None,
            next_order: None,
            better_level,
            worse_level,
            level_rent_recipient: Some(fixture.owner.pubkey()),
            trader_balance: trader_balance_address(fixture.market.market, fixture.owner.pubkey()),
        }
        .to_account_metas(None),
    )
}

fn setup_three_bid_levels() -> (OpenOrderFixture, DistinctBidOrder, DistinctBidOrder) {
    let mut fixture = setup_open_order(OrderSide::Bid);
    let original_level = fixture.price_level;
    let better = insert_bid_level(&mut fixture, 2, 110_000_000, None, Some(original_level));
    let worse = insert_bid_level(&mut fixture, 3, 90_000_000, Some(original_level), None);

    (fixture, better, worse)
}

fn load_order(svm: &LiteSVM, address: Pubkey) -> Order {
    let account = svm.get_account(&address).unwrap();
    let mut data: &[u8] = &account.data;
    Order::try_deserialize(&mut data).unwrap()
}

fn load_market(svm: &LiteSVM, address: Pubkey) -> Market {
    let account = svm.get_account(&address).unwrap();
    let mut data: &[u8] = &account.data;
    Market::try_deserialize(&mut data).unwrap()
}

fn load_trader_balance(
    svm: &LiteSVM,
    market: Pubkey,
    owner: Pubkey,
) -> tidebook::state::TraderBalance {
    let address = trader_balance_address(market, owner);
    let account = svm.get_account(&address).unwrap();
    let mut data: &[u8] = &account.data;
    tidebook::state::TraderBalance::try_deserialize(&mut data).unwrap()
}

fn store_trader_balance(
    svm: &mut LiteSVM,
    market: Pubkey,
    owner: Pubkey,
    state: &tidebook::state::TraderBalance,
) {
    let address = trader_balance_address(market, owner);
    let mut account = svm.get_account(&address).unwrap();
    let mut data = Vec::new();
    state.try_serialize(&mut data).unwrap();
    account.data = data;
    svm.set_account(address, account).unwrap();
}

fn load_price_level(svm: &LiteSVM, address: Pubkey) -> PriceLevel {
    let account = svm.get_account(&address).unwrap();
    let mut data: &[u8] = &account.data;
    PriceLevel::try_deserialize(&mut data).unwrap()
}

fn token_balance(svm: &LiteSVM, address: Pubkey) -> u64 {
    let account = svm.get_account(&address).unwrap();
    SplTokenAccount::unpack(&account.data).unwrap().amount
}

fn setup_open_order(side: OrderSide) -> OpenOrderFixture {
    let mut svm = LiteSVM::new();
    let owner = Keypair::new();

    svm.add_program(tidebook::id(), PROGRAM_BYTES).unwrap();
    svm.airdrop(&owner.pubkey(), 2_000_000_000).unwrap();
    store_active_admin(&mut svm, owner.pubkey());

    let market = initialize_market(&mut svm, &owner);
    let (order, owner_collateral, collateral_mint, market_vault, locked_collateral) =
        place_order(&mut svm, &owner, &market, ORDER_ID, side);
    let price_level =
        tidebook::derive_price_level_pda(&tidebook::id(), &market.market, side, TEST_ORDER_PRICE).0;

    OpenOrderFixture {
        svm,
        owner,
        market,
        order,
        price_level,
        owner_collateral,
        collateral_mint,
        market_vault,
        locked_collateral,
    }
}

fn assert_order_and_collateral_unchanged(fixture: &OpenOrderFixture) {
    let order = load_order(&fixture.svm, fixture.order);
    assert_eq!(order.status, OrderStatus::Open);
    assert_eq!(order.locked_collateral, fixture.locked_collateral);
    assert_eq!(
        token_balance(&fixture.svm, fixture.owner_collateral),
        fixture.locked_collateral
    );
    assert_eq!(token_balance(&fixture.svm, fixture.market_vault), 0);
}

#[test]
fn canceling_bid_refunds_quote_collateral() {
    let mut fixture = setup_open_order(OrderSide::Bid);
    assert_order_and_collateral_unchanged(&fixture);
    let instruction = cancel_order_instruction(&fixture, fixture.owner.pubkey());

    let result = send_instruction(&mut fixture.svm, &fixture.owner, instruction);

    assert!(result.is_ok(), "order cancellation failed: {result:?}");
    let state = load_order(&fixture.svm, fixture.order);
    assert_eq!(state.status, OrderStatus::Canceled);
    assert_eq!(state.remaining_quantity, 0);
    assert!(
        fixture.svm.get_account(&fixture.price_level).is_none(),
        "empty bid price level remained allocated"
    );
    assert_eq!(
        load_market(&fixture.svm, fixture.market.market).best_bid,
        None
    );
    assert_eq!(state.locked_collateral, 0);
    assert_eq!(
        token_balance(&fixture.svm, fixture.owner_collateral),
        TEST_QUOTE_COLLATERAL
    );
    assert_eq!(token_balance(&fixture.svm, fixture.market_vault), 0);
}

#[test]
fn canceling_ask_refunds_base_collateral() {
    let mut fixture = setup_open_order(OrderSide::Ask);
    assert_order_and_collateral_unchanged(&fixture);
    let instruction = cancel_order_instruction(&fixture, fixture.owner.pubkey());

    let result = send_instruction(&mut fixture.svm, &fixture.owner, instruction);

    assert!(result.is_ok(), "ask cancellation failed: {result:?}");
    let state = load_order(&fixture.svm, fixture.order);
    assert_eq!(state.status, OrderStatus::Canceled);
    assert_eq!(state.locked_collateral, 0);
    assert!(
        fixture.svm.get_account(&fixture.price_level).is_none(),
        "empty ask price level remained allocated"
    );
    assert_eq!(
        load_market(&fixture.svm, fixture.market.market).best_ask,
        None
    );
    assert_eq!(
        token_balance(&fixture.svm, fixture.owner_collateral),
        TEST_ORDER_QUANTITY
    );
    assert_eq!(token_balance(&fixture.svm, fixture.market_vault), 0);
}

#[test]
fn canceling_fifo_head_promotes_the_next_order() {
    let mut fixture = setup_open_order(OrderSide::Bid);
    let first_order = fixture.order;
    let (second_order, _) = append_bid_order(&mut fixture, 2, first_order);
    let instruction = cancel_queued_order_instruction(
        &fixture,
        first_order,
        1,
        None,
        Some(second_order),
        fixture.owner_collateral,
    );

    let result = send_instruction(&mut fixture.svm, &fixture.owner, instruction);

    assert!(result.is_ok(), "head cancellation failed: {result:?}");
    let canceled = load_order(&fixture.svm, first_order);
    let second = load_order(&fixture.svm, second_order);
    let level = load_price_level(&fixture.svm, fixture.price_level);
    assert_eq!(canceled.status, OrderStatus::Canceled);
    assert_eq!(canceled.previous_order, None);
    assert_eq!(canceled.next_order, None);
    assert_eq!(second.previous_order, None);
    assert_eq!(level.first_order, Some(second_order));
    assert_eq!(level.last_order, Some(second_order));
    assert_eq!(level.order_count, 1);
    assert_eq!(level.total_remaining_quantity, TEST_ORDER_QUANTITY);
    assert_eq!(
        load_market(&fixture.svm, fixture.market.market).open_order_count,
        1
    );
    assert_eq!(token_balance(&fixture.svm, fixture.market_vault), 0);
    assert_eq!(
        load_trader_balance(&fixture.svm, fixture.market.market, fixture.owner.pubkey(),)
            .quote_locked,
        TEST_QUOTE_COLLATERAL
    );
}

#[test]
fn canceling_fifo_tail_promotes_the_previous_order() {
    let mut fixture = setup_open_order(OrderSide::Bid);
    let first_order = fixture.order;
    let (second_order, second_collateral) = append_bid_order(&mut fixture, 2, first_order);
    let instruction = cancel_queued_order_instruction(
        &fixture,
        second_order,
        2,
        Some(first_order),
        None,
        second_collateral,
    );

    let result = send_instruction(&mut fixture.svm, &fixture.owner, instruction);

    assert!(result.is_ok(), "tail cancellation failed: {result:?}");
    let first = load_order(&fixture.svm, first_order);
    let canceled = load_order(&fixture.svm, second_order);
    let level = load_price_level(&fixture.svm, fixture.price_level);
    assert_eq!(first.next_order, None);
    assert_eq!(canceled.status, OrderStatus::Canceled);
    assert_eq!(canceled.previous_order, None);
    assert_eq!(canceled.next_order, None);
    assert_eq!(level.first_order, Some(first_order));
    assert_eq!(level.last_order, Some(first_order));
    assert_eq!(level.order_count, 1);
    assert_eq!(level.total_remaining_quantity, TEST_ORDER_QUANTITY);
    assert_eq!(
        token_balance(&fixture.svm, second_collateral),
        TEST_QUOTE_COLLATERAL
    );
    assert_eq!(token_balance(&fixture.svm, fixture.market_vault), 0);
}

#[test]
fn canceling_fifo_middle_connects_its_neighbors() {
    let mut fixture = setup_open_order(OrderSide::Bid);
    let first_order = fixture.order;
    let (second_order, second_collateral) = append_bid_order(&mut fixture, 2, first_order);
    let (third_order, _) = append_bid_order(&mut fixture, 3, second_order);
    let instruction = cancel_queued_order_instruction(
        &fixture,
        second_order,
        2,
        Some(first_order),
        Some(third_order),
        second_collateral,
    );

    let result = send_instruction(&mut fixture.svm, &fixture.owner, instruction);

    assert!(result.is_ok(), "middle cancellation failed: {result:?}");
    let first = load_order(&fixture.svm, first_order);
    let canceled = load_order(&fixture.svm, second_order);
    let third = load_order(&fixture.svm, third_order);
    let level = load_price_level(&fixture.svm, fixture.price_level);
    assert_eq!(first.next_order, Some(third_order));
    assert_eq!(third.previous_order, Some(first_order));
    assert_eq!(canceled.status, OrderStatus::Canceled);
    assert_eq!(canceled.previous_order, None);
    assert_eq!(canceled.next_order, None);
    assert_eq!(level.first_order, Some(first_order));
    assert_eq!(level.last_order, Some(third_order));
    assert_eq!(level.order_count, 2);
    assert_eq!(level.total_remaining_quantity, TEST_ORDER_QUANTITY * 2);
    assert_eq!(
        load_market(&fixture.svm, fixture.market.market).open_order_count,
        2
    );
    assert_eq!(token_balance(&fixture.svm, fixture.market_vault), 0);
    assert_eq!(
        load_trader_balance(&fixture.svm, fixture.market.market, fixture.owner.pubkey(),)
            .quote_locked,
        TEST_QUOTE_COLLATERAL * 2
    );
}

#[test]
fn omitting_a_required_fifo_neighbor_is_rejected_atomically() {
    let mut fixture = setup_open_order(OrderSide::Bid);
    let first_order = fixture.order;
    let (second_order, _) = append_bid_order(&mut fixture, 2, first_order);
    let level_before = load_price_level(&fixture.svm, fixture.price_level);
    let instruction = cancel_order_instruction(&fixture, fixture.owner.pubkey());

    let result = send_instruction(&mut fixture.svm, &fixture.owner, instruction);

    assert!(
        result.is_err(),
        "cancellation omitted the required next order"
    );
    let first = load_order(&fixture.svm, first_order);
    let second = load_order(&fixture.svm, second_order);
    let level_after = load_price_level(&fixture.svm, fixture.price_level);
    assert_eq!(first.status, OrderStatus::Open);
    assert_eq!(first.next_order, Some(second_order));
    assert_eq!(second.previous_order, Some(first_order));
    assert_eq!(level_after.first_order, level_before.first_order);
    assert_eq!(level_after.last_order, level_before.last_order);
    assert_eq!(level_after.order_count, level_before.order_count);
    assert_eq!(
        level_after.total_remaining_quantity,
        level_before.total_remaining_quantity
    );
    assert_eq!(
        token_balance(&fixture.svm, fixture.owner_collateral),
        TEST_QUOTE_COLLATERAL
    );
    assert_eq!(token_balance(&fixture.svm, fixture.market_vault), 0);
}

#[test]
fn canceling_the_best_price_level_promotes_the_worse_level() {
    let (mut fixture, better, worse) = setup_three_bid_levels();
    let original_level = fixture.price_level;
    let instruction =
        cancel_distinct_bid_level_instruction(&fixture, &better, 2, None, Some(original_level));

    let result = send_instruction(&mut fixture.svm, &fixture.owner, instruction);

    assert!(result.is_ok(), "best-level cancellation failed: {result:?}");
    assert!(fixture.svm.get_account(&better.price_level).is_none());
    let market = load_market(&fixture.svm, fixture.market.market);
    let original = load_price_level(&fixture.svm, original_level);
    let worse_state = load_price_level(&fixture.svm, worse.price_level);
    assert_eq!(market.best_bid, Some(TEST_ORDER_PRICE));
    assert_eq!(original.better_price, None);
    assert_eq!(original.worse_price, Some(90_000_000));
    assert_eq!(worse_state.better_price, Some(TEST_ORDER_PRICE));
    assert_eq!(market.open_order_count, 2);
    assert_eq!(
        token_balance(&fixture.svm, better.owner_collateral),
        better.locked_collateral
    );
}

#[test]
fn canceling_a_middle_price_level_connects_adjacent_levels() {
    let (mut fixture, better, worse) = setup_three_bid_levels();
    let original = DistinctBidOrder {
        order: fixture.order,
        price_level: fixture.price_level,
        owner_collateral: fixture.owner_collateral,
        locked_collateral: fixture.locked_collateral,
    };
    let instruction = cancel_distinct_bid_level_instruction(
        &fixture,
        &original,
        1,
        Some(better.price_level),
        Some(worse.price_level),
    );

    let result = send_instruction(&mut fixture.svm, &fixture.owner, instruction);

    assert!(
        result.is_ok(),
        "middle-level cancellation failed: {result:?}"
    );
    assert!(fixture.svm.get_account(&original.price_level).is_none());
    let market = load_market(&fixture.svm, fixture.market.market);
    let better_state = load_price_level(&fixture.svm, better.price_level);
    let worse_state = load_price_level(&fixture.svm, worse.price_level);
    assert_eq!(market.best_bid, Some(110_000_000));
    assert_eq!(better_state.worse_price, Some(90_000_000));
    assert_eq!(worse_state.better_price, Some(110_000_000));
    assert_eq!(market.open_order_count, 2);
    assert_eq!(
        token_balance(&fixture.svm, original.owner_collateral),
        original.locked_collateral
    );
}

#[test]
fn canceling_the_worst_price_level_clears_the_better_tail_link() {
    let (mut fixture, better, worse) = setup_three_bid_levels();
    let original_level = fixture.price_level;
    let instruction =
        cancel_distinct_bid_level_instruction(&fixture, &worse, 3, Some(original_level), None);

    let result = send_instruction(&mut fixture.svm, &fixture.owner, instruction);

    assert!(
        result.is_ok(),
        "worst-level cancellation failed: {result:?}"
    );
    assert!(fixture.svm.get_account(&worse.price_level).is_none());
    let market = load_market(&fixture.svm, fixture.market.market);
    let original = load_price_level(&fixture.svm, original_level);
    let better_state = load_price_level(&fixture.svm, better.price_level);
    assert_eq!(market.best_bid, Some(110_000_000));
    assert_eq!(better_state.worse_price, Some(TEST_ORDER_PRICE));
    assert_eq!(original.better_price, Some(110_000_000));
    assert_eq!(original.worse_price, None);
    assert_eq!(market.open_order_count, 2);
    assert_eq!(
        token_balance(&fixture.svm, worse.owner_collateral),
        worse.locked_collateral
    );
}

#[test]
fn missing_price_level_neighbor_is_rejected_atomically() {
    let (mut fixture, better, worse) = setup_three_bid_levels();
    let original = DistinctBidOrder {
        order: fixture.order,
        price_level: fixture.price_level,
        owner_collateral: fixture.owner_collateral,
        locked_collateral: fixture.locked_collateral,
    };
    let instruction = cancel_distinct_bid_level_instruction(
        &fixture,
        &original,
        1,
        Some(better.price_level),
        None,
    );

    let result = send_instruction(&mut fixture.svm, &fixture.owner, instruction);

    assert!(
        result.is_err(),
        "cancellation omitted the worse price level"
    );
    let order = load_order(&fixture.svm, original.order);
    let original_state = load_price_level(&fixture.svm, original.price_level);
    let better_state = load_price_level(&fixture.svm, better.price_level);
    let worse_state = load_price_level(&fixture.svm, worse.price_level);
    assert_eq!(order.status, OrderStatus::Open);
    assert_eq!(original_state.better_price, Some(110_000_000));
    assert_eq!(original_state.worse_price, Some(90_000_000));
    assert_eq!(better_state.worse_price, Some(TEST_ORDER_PRICE));
    assert_eq!(worse_state.better_price, Some(TEST_ORDER_PRICE));
    assert_eq!(
        load_market(&fixture.svm, fixture.market.market).open_order_count,
        3
    );
    assert_eq!(
        token_balance(&fixture.svm, original.owner_collateral),
        original.locked_collateral
    );
}

#[test]
fn non_owner_cannot_cancel_order() {
    let mut fixture = setup_open_order(OrderSide::Bid);
    let attacker = Keypair::new();
    fixture
        .svm
        .airdrop(&attacker.pubkey(), 1_000_000_000)
        .unwrap();
    let instruction = cancel_order_instruction(&fixture, attacker.pubkey());

    let result = send_instruction(&mut fixture.svm, &attacker, instruction);

    assert!(result.is_err(), "non-owner canceled the order");
    assert_order_and_collateral_unchanged(&fixture);
}

#[test]
fn canceled_order_cannot_be_canceled_or_refunded_twice() {
    let mut fixture = setup_open_order(OrderSide::Bid);
    let first_instruction = cancel_order_instruction(&fixture, fixture.owner.pubkey());
    let first = send_instruction(&mut fixture.svm, &fixture.owner, first_instruction);
    assert!(first.is_ok(), "first cancellation failed: {first:?}");

    let owner_balance_after_first = token_balance(&fixture.svm, fixture.owner_collateral);
    let second_instruction = cancel_order_instruction(&fixture, fixture.owner.pubkey());
    let second = send_instruction(&mut fixture.svm, &fixture.owner, second_instruction);

    assert!(second.is_err(), "order was canceled twice");
    let state = load_order(&fixture.svm, fixture.order);
    assert_eq!(state.status, OrderStatus::Canceled);
    assert_eq!(state.locked_collateral, 0);
    assert_eq!(
        token_balance(&fixture.svm, fixture.owner_collateral),
        owner_balance_after_first
    );
    assert_eq!(token_balance(&fixture.svm, fixture.market_vault), 0);
}

#[test]
fn order_cannot_be_canceled_with_different_market() {
    let mut fixture = setup_open_order(OrderSide::Bid);
    let different_market = initialize_market(&mut fixture.svm, &fixture.owner);
    assert_ne!(fixture.market.market, different_market.market);
    let instruction = cancel_order_instruction_with_accounts(
        fixture.owner.pubkey(),
        different_market.market,
        fixture.order,
        ORDER_ID,
        fixture.price_level,
        fixture.collateral_mint,
        fixture.owner_collateral,
        different_market.vault_authority,
        different_market.quote_vault,
    );

    let result = send_instruction(&mut fixture.svm, &fixture.owner, instruction);

    assert!(result.is_err(), "order accepted a different market");
    assert_order_and_collateral_unchanged(&fixture);
}

#[test]
fn owner_can_cancel_and_receive_refund_while_market_is_paused() {
    let mut fixture = setup_open_order(OrderSide::Bid);
    let pause = Instruction::new_with_bytes(
        tidebook::id(),
        &tidebook::instruction::PauseMarket {}.data(),
        tidebook::accounts::PauseMarket {
            authority: fixture.owner.pubkey(),
            market: fixture.market.market,
        }
        .to_account_metas(None),
    );
    let pause_result = send_instruction(&mut fixture.svm, &fixture.owner, pause);
    assert!(
        pause_result.is_ok(),
        "market pause failed: {pause_result:?}"
    );
    assert_eq!(
        load_market(&fixture.svm, fixture.market.market).status,
        MarketStatus::Paused
    );

    let cancel = cancel_order_instruction(&fixture, fixture.owner.pubkey());
    let cancel_result = send_instruction(&mut fixture.svm, &fixture.owner, cancel);

    assert!(
        cancel_result.is_ok(),
        "paused-market cancellation failed: {cancel_result:?}"
    );
    let state = load_order(&fixture.svm, fixture.order);
    assert_eq!(state.status, OrderStatus::Canceled);
    assert_eq!(state.locked_collateral, 0);
    assert_eq!(
        token_balance(&fixture.svm, fixture.owner_collateral),
        TEST_QUOTE_COLLATERAL
    );
    assert_eq!(token_balance(&fixture.svm, fixture.market_vault), 0);
}

#[test]
fn locked_balance_underflow_rejects_cancellation_atomically() {
    let mut fixture = setup_open_order(OrderSide::Bid);
    let mut balance =
        load_trader_balance(&fixture.svm, fixture.market.market, fixture.owner.pubkey());
    balance.quote_locked = fixture.locked_collateral - 1;
    store_trader_balance(
        &mut fixture.svm,
        fixture.market.market,
        fixture.owner.pubkey(),
        &balance,
    );
    let instruction = cancel_order_instruction(&fixture, fixture.owner.pubkey());

    let result = send_instruction(&mut fixture.svm, &fixture.owner, instruction);

    assert!(result.is_err(), "locked-balance underflow was accepted");
    let order = load_order(&fixture.svm, fixture.order);
    assert_eq!(order.status, OrderStatus::Open);
    assert_eq!(order.locked_collateral, fixture.locked_collateral);
    assert_eq!(
        load_trader_balance(&fixture.svm, fixture.market.market, fixture.owner.pubkey(),)
            .quote_locked,
        fixture.locked_collateral - 1
    );
}

#[test]
fn free_balance_overflow_rejects_cancellation_atomically() {
    let mut fixture = setup_open_order(OrderSide::Bid);
    let mut balance =
        load_trader_balance(&fixture.svm, fixture.market.market, fixture.owner.pubkey());
    balance.quote_free = u64::MAX;
    store_trader_balance(
        &mut fixture.svm,
        fixture.market.market,
        fixture.owner.pubkey(),
        &balance,
    );
    let instruction = cancel_order_instruction(&fixture, fixture.owner.pubkey());

    let result = send_instruction(&mut fixture.svm, &fixture.owner, instruction);

    assert!(result.is_err(), "free-balance overflow was accepted");
    assert_eq!(
        load_order(&fixture.svm, fixture.order).status,
        OrderStatus::Open
    );
    assert_eq!(
        load_trader_balance(&fixture.svm, fixture.market.market, fixture.owner.pubkey(),)
            .quote_free,
        u64::MAX
    );
}

#[test]
fn corrupted_trader_balance_owner_rejects_cancellation() {
    let mut fixture = setup_open_order(OrderSide::Bid);
    let mut balance =
        load_trader_balance(&fixture.svm, fixture.market.market, fixture.owner.pubkey());
    balance.owner = Pubkey::new_unique();
    store_trader_balance(
        &mut fixture.svm,
        fixture.market.market,
        fixture.owner.pubkey(),
        &balance,
    );
    let instruction = cancel_order_instruction(&fixture, fixture.owner.pubkey());

    let result = send_instruction(&mut fixture.svm, &fixture.owner, instruction);

    assert!(result.is_err(), "corrupted balance owner was accepted");
    assert_eq!(
        load_order(&fixture.svm, fixture.order).status,
        OrderStatus::Open
    );
}
