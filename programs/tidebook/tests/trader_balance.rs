//! LiteSVM integration coverage for canonical per-market trader balances.

// LiteSVM intentionally returns rich transaction-failure metadata. Boxing it in
// every test helper would add indirection without reducing production account or
// instruction size, so this integration-test boundary permits the large error.
#![allow(clippy::result_large_err)]

mod support;

use {
    anchor_lang::{
        prelude::Pubkey,
        solana_program::{instruction::Instruction, program_pack::Pack, system_program},
        AccountDeserialize, AccountSerialize, InstructionData, Space, ToAccountMetas,
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
    tidebook::state::{Market, MarketStatus, OrderSide, TraderBalance},
};

const PROGRAM_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../target/deploy/tidebook.so"
));

fn store_market(svm: &mut LiteSVM) -> Pubkey {
    let market_address = Pubkey::new_unique();
    let market = Market {
        authority: Pubkey::new_unique(),
        base_mint: Pubkey::new_unique(),
        quote_mint: Pubkey::new_unique(),
        status: MarketStatus::Active,
        next_order_id: 1,
        base_decimals: 9,
        quote_decimals: 6,
        price_tick_size: 10_000,
        quantity_lot_size: 1_000_000,
        best_bid: None,
        best_ask: None,
        open_order_count: 0,
        bump: 255,
    };
    let mut data = Vec::new();
    market.try_serialize(&mut data).unwrap();

    svm.set_account(
        market_address,
        Account {
            lamports: svm.minimum_balance_for_rent_exemption(data.len()),
            data,
            owner: tidebook::id(),
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    market_address
}

fn setup() -> (LiteSVM, Pubkey) {
    let mut svm = LiteSVM::new();
    svm.add_program(tidebook::id(), PROGRAM_BYTES).unwrap();
    let market = store_market(&mut svm);
    (svm, market)
}

fn initialize_trader_balance_instruction(owner: Pubkey, market: Pubkey) -> (Pubkey, Instruction) {
    let (trader_balance, _) = tidebook::derive_trader_balance_pda(&tidebook::id(), &market, &owner);
    let instruction = Instruction::new_with_bytes(
        tidebook::id(),
        &tidebook::instruction::InitializeTraderBalance {}.data(),
        tidebook::accounts::InitializeTraderBalance {
            owner,
            market,
            trader_balance,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    );

    (trader_balance, instruction)
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

fn initialize_for(
    svm: &mut LiteSVM,
    owner: &Keypair,
    market: Pubkey,
) -> (Pubkey, litesvm::types::TransactionResult) {
    svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
    let (trader_balance, instruction) =
        initialize_trader_balance_instruction(owner.pubkey(), market);
    let result = send_instruction(svm, owner, instruction);
    (trader_balance, result)
}

fn load_trader_balance(svm: &LiteSVM, address: Pubkey) -> TraderBalance {
    let account = svm.get_account(&address).unwrap();
    let mut data: &[u8] = &account.data;
    TraderBalance::try_deserialize(&mut data).unwrap()
}

#[test]
fn initializes_canonical_zeroed_balance_for_owner_and_market() {
    let (mut svm, market) = setup();
    let owner = Keypair::new();
    let (trader_balance, result) = initialize_for(&mut svm, &owner, market);

    assert!(result.is_ok(), "balance initialization failed: {result:?}");

    let event =
        support::events::single_event::<tidebook::events::TraderBalanceInitializedEvent>(&result);
    assert_eq!(event.market, market);
    assert_eq!(event.owner, owner.pubkey());
    assert_eq!(event.trader_balance, trader_balance);

    let state = load_trader_balance(&svm, trader_balance);
    let (_, expected_bump) =
        tidebook::derive_trader_balance_pda(&tidebook::id(), &market, &owner.pubkey());
    assert_eq!(state.market, market);
    assert_eq!(state.owner, owner.pubkey());
    assert_eq!(state.base_free, 0);
    assert_eq!(state.base_locked, 0);
    assert_eq!(state.quote_free, 0);
    assert_eq!(state.quote_locked, 0);
    assert_eq!(state.bump, expected_bump);
}

#[test]
fn rejects_duplicate_initialization() {
    let (mut svm, market) = setup();
    let owner = Keypair::new();
    let (_, first_result) = initialize_for(&mut svm, &owner, market);
    assert!(first_result.is_ok());

    let (_, duplicate_instruction) = initialize_trader_balance_instruction(owner.pubkey(), market);
    let duplicate_result = send_instruction(&mut svm, &owner, duplicate_instruction);

    assert!(duplicate_result.is_err());
}

#[test]
fn different_traders_receive_distinct_balance_accounts() {
    let (mut svm, market) = setup();
    let first_owner = Keypair::new();
    let second_owner = Keypair::new();

    let (first_balance, first_result) = initialize_for(&mut svm, &first_owner, market);
    let (second_balance, second_result) = initialize_for(&mut svm, &second_owner, market);

    assert!(first_result.is_ok());
    assert!(second_result.is_ok());
    assert_ne!(first_balance, second_balance);
    assert_eq!(
        load_trader_balance(&svm, first_balance).owner,
        first_owner.pubkey()
    );
    assert_eq!(
        load_trader_balance(&svm, second_balance).owner,
        second_owner.pubkey()
    );
}
struct DepositFixture {
    market: Pubkey,
    base_mint: Pubkey,
    quote_mint: Pubkey,
    vault_authority: Pubkey,
    base_vault: Pubkey,
    quote_vault: Pubkey,
}

fn store_test_mint(svm: &mut LiteSVM, decimals: u8) -> Pubkey {
    let mint = Pubkey::new_unique();
    let state = SplMint {
        decimals,
        is_initialized: true,
        ..SplMint::default()
    };
    let mut data = vec![0_u8; SplMint::LEN];
    SplMint::pack(state, &mut data).unwrap();
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

fn store_test_token_account(
    svm: &mut LiteSVM,
    address: Pubkey,
    mint: Pubkey,
    owner: Pubkey,
    amount: u64,
) {
    let state = SplTokenAccount {
        mint,
        owner,
        amount,
        state: AccountState::Initialized,
        ..SplTokenAccount::default()
    };
    let mut data = vec![0_u8; SplTokenAccount::LEN];
    SplTokenAccount::pack(state, &mut data).unwrap();
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
}

fn create_test_token_account(
    svm: &mut LiteSVM,
    mint: Pubkey,
    owner: Pubkey,
    amount: u64,
) -> Pubkey {
    let address = Pubkey::new_unique();
    store_test_token_account(svm, address, mint, owner, amount);
    address
}

fn store_deposit_market(svm: &mut LiteSVM, status: MarketStatus) -> DepositFixture {
    let market = Pubkey::new_unique();
    let base_mint = store_test_mint(svm, 9);
    let quote_mint = store_test_mint(svm, 6);
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

    store_test_token_account(svm, base_vault, base_mint, vault_authority, 0);
    store_test_token_account(svm, quote_vault, quote_mint, vault_authority, 0);

    let state = Market {
        authority: Pubkey::new_unique(),
        base_mint,
        quote_mint,
        status,
        next_order_id: 1,
        base_decimals: 9,
        quote_decimals: 6,
        price_tick_size: 10_000,
        quantity_lot_size: 1_000_000,
        best_bid: None,
        best_ask: None,
        open_order_count: 0,
        bump: 255,
    };
    let mut data = Vec::new();
    state.try_serialize(&mut data).unwrap();
    // Match the fixed allocation used by `initialize_market`. The serialized
    // empty book uses fewer bytes because both best-price Options are `None`,
    // but placement must later be able to persist `Some(price)` in-place.
    data.resize(8 + Market::INIT_SPACE, 0);
    svm.set_account(
        market,
        Account {
            lamports: svm.minimum_balance_for_rent_exemption(data.len()),
            data,
            owner: tidebook::id(),
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    DepositFixture {
        market,
        base_mint,
        quote_mint,
        vault_authority,
        base_vault,
        quote_vault,
    }
}

fn setup_deposit(status: MarketStatus) -> (LiteSVM, Keypair, DepositFixture, Pubkey) {
    let mut svm = LiteSVM::new();
    let owner = Keypair::new();
    svm.add_program(tidebook::id(), PROGRAM_BYTES).unwrap();
    let fixture = store_deposit_market(&mut svm, status);
    let (trader_balance, result) = initialize_for(&mut svm, &owner, fixture.market);
    assert!(result.is_ok(), "balance initialization failed: {result:?}");
    (svm, owner, fixture, trader_balance)
}

fn deposit_instruction(
    owner: Pubkey,
    fixture: &DepositFixture,
    trader_balance: Pubkey,
    deposit_mint: Pubkey,
    trader_token_account: Pubkey,
    market_vault: Pubkey,
    amount: u64,
) -> Instruction {
    Instruction::new_with_bytes(
        tidebook::id(),
        &tidebook::instruction::Deposit { amount }.data(),
        tidebook::accounts::Deposit {
            owner,
            market: fixture.market,
            trader_balance,
            deposit_mint,
            trader_token_account,
            vault_authority: fixture.vault_authority,
            market_vault,
            token_program: anchor_spl::token::ID,
        }
        .to_account_metas(None),
    )
}

fn load_token_amount(svm: &LiteSVM, address: Pubkey) -> u64 {
    let account = svm.get_account(&address).unwrap();
    SplTokenAccount::unpack(&account.data).unwrap().amount
}

fn store_trader_balance(svm: &mut LiteSVM, address: Pubkey, state: &TraderBalance) {
    let mut account = svm.get_account(&address).unwrap();
    let mut data = Vec::new();
    state.try_serialize(&mut data).unwrap();
    account.data = data;
    svm.set_account(address, account).unwrap();
}

#[test]
fn base_deposit_moves_tokens_and_credits_only_base_free() {
    let (mut svm, owner, fixture, trader_balance) = setup_deposit(MarketStatus::Active);
    let source = create_test_token_account(&mut svm, fixture.base_mint, owner.pubkey(), 500);
    let instruction = deposit_instruction(
        owner.pubkey(),
        &fixture,
        trader_balance,
        fixture.base_mint,
        source,
        fixture.base_vault,
        120,
    );

    let result = send_instruction(&mut svm, &owner, instruction);
    assert!(result.is_ok(), "base deposit failed: {result:?}");

    let event = support::events::single_event::<tidebook::events::DepositEvent>(&result);
    assert_eq!(event.market, fixture.market);
    assert_eq!(event.owner, owner.pubkey());
    assert_eq!(event.trader_balance, trader_balance);
    assert_eq!(event.mint, fixture.base_mint);
    assert_eq!(event.market_vault, fixture.base_vault);
    assert_eq!(event.amount, 120);
    assert_eq!(event.free_balance, 120);

    let balance = load_trader_balance(&svm, trader_balance);
    assert_eq!(load_token_amount(&svm, source), 380);
    assert_eq!(load_token_amount(&svm, fixture.base_vault), 120);
    assert_eq!(balance.base_free, 120);
    assert_eq!(balance.base_locked, 0);
    assert_eq!(balance.quote_free, 0);
    assert_eq!(balance.quote_locked, 0);
}

#[test]
fn quote_deposit_moves_tokens_and_credits_only_quote_free() {
    let (mut svm, owner, fixture, trader_balance) = setup_deposit(MarketStatus::Active);
    let source = create_test_token_account(&mut svm, fixture.quote_mint, owner.pubkey(), 900);
    let instruction = deposit_instruction(
        owner.pubkey(),
        &fixture,
        trader_balance,
        fixture.quote_mint,
        source,
        fixture.quote_vault,
        250,
    );

    let result = send_instruction(&mut svm, &owner, instruction);
    assert!(result.is_ok(), "quote deposit failed: {result:?}");

    let balance = load_trader_balance(&svm, trader_balance);
    assert_eq!(load_token_amount(&svm, source), 650);
    assert_eq!(load_token_amount(&svm, fixture.quote_vault), 250);
    assert_eq!(balance.base_free, 0);
    assert_eq!(balance.quote_free, 250);
}

#[test]
fn deposit_remains_available_while_market_is_paused() {
    let (mut svm, owner, fixture, trader_balance) = setup_deposit(MarketStatus::Paused);
    let source = create_test_token_account(&mut svm, fixture.base_mint, owner.pubkey(), 50);
    let instruction = deposit_instruction(
        owner.pubkey(),
        &fixture,
        trader_balance,
        fixture.base_mint,
        source,
        fixture.base_vault,
        20,
    );

    let result = send_instruction(&mut svm, &owner, instruction);
    assert!(result.is_ok(), "paused-market deposit failed: {result:?}");
    assert_eq!(load_token_amount(&svm, fixture.base_vault), 20);
    assert_eq!(load_trader_balance(&svm, trader_balance).base_free, 20);
}

#[test]
fn zero_deposit_is_rejected_without_mutation() {
    let (mut svm, owner, fixture, trader_balance) = setup_deposit(MarketStatus::Active);
    let source = create_test_token_account(&mut svm, fixture.base_mint, owner.pubkey(), 50);
    let instruction = deposit_instruction(
        owner.pubkey(),
        &fixture,
        trader_balance,
        fixture.base_mint,
        source,
        fixture.base_vault,
        0,
    );

    let result = send_instruction(&mut svm, &owner, instruction);
    assert!(result.is_err());
    assert_eq!(load_token_amount(&svm, source), 50);
    assert_eq!(load_token_amount(&svm, fixture.base_vault), 0);
    assert_eq!(load_trader_balance(&svm, trader_balance).base_free, 0);
}

#[test]
fn insufficient_deposit_funds_are_rejected_without_mutation() {
    let (mut svm, owner, fixture, trader_balance) = setup_deposit(MarketStatus::Active);
    let source = create_test_token_account(&mut svm, fixture.quote_mint, owner.pubkey(), 40);
    let instruction = deposit_instruction(
        owner.pubkey(),
        &fixture,
        trader_balance,
        fixture.quote_mint,
        source,
        fixture.quote_vault,
        41,
    );

    let result = send_instruction(&mut svm, &owner, instruction);
    assert!(result.is_err());
    assert_eq!(load_token_amount(&svm, source), 40);
    assert_eq!(load_token_amount(&svm, fixture.quote_vault), 0);
    assert_eq!(load_trader_balance(&svm, trader_balance).quote_free, 0);
}

#[test]
fn non_market_mint_is_rejected_without_mutation() {
    let (mut svm, owner, fixture, trader_balance) = setup_deposit(MarketStatus::Active);
    let unrelated_mint = store_test_mint(&mut svm, 6);
    let source = create_test_token_account(&mut svm, unrelated_mint, owner.pubkey(), 50);
    let (unrelated_vault, _) = Pubkey::find_program_address(
        &[
            tidebook::constants::VAULT_SEED,
            fixture.market.as_ref(),
            unrelated_mint.as_ref(),
        ],
        &tidebook::id(),
    );
    store_test_token_account(
        &mut svm,
        unrelated_vault,
        unrelated_mint,
        fixture.vault_authority,
        0,
    );
    let instruction = deposit_instruction(
        owner.pubkey(),
        &fixture,
        trader_balance,
        unrelated_mint,
        source,
        unrelated_vault,
        10,
    );

    let result = send_instruction(&mut svm, &owner, instruction);
    assert!(result.is_err());
    assert_eq!(load_token_amount(&svm, source), 50);
    assert_eq!(load_token_amount(&svm, unrelated_vault), 0);
    let balance = load_trader_balance(&svm, trader_balance);
    assert_eq!(balance.base_free, 0);
    assert_eq!(balance.quote_free, 0);
}

#[test]
fn token_account_owned_by_another_wallet_cannot_be_deposited() {
    let (mut svm, owner, fixture, trader_balance) = setup_deposit(MarketStatus::Active);
    let source = create_test_token_account(&mut svm, fixture.base_mint, Pubkey::new_unique(), 50);
    let instruction = deposit_instruction(
        owner.pubkey(),
        &fixture,
        trader_balance,
        fixture.base_mint,
        source,
        fixture.base_vault,
        10,
    );

    let result = send_instruction(&mut svm, &owner, instruction);
    assert!(result.is_err());
    assert_eq!(load_token_amount(&svm, source), 50);
    assert_eq!(load_token_amount(&svm, fixture.base_vault), 0);
    assert_eq!(load_trader_balance(&svm, trader_balance).base_free, 0);
}

#[test]
fn free_balance_overflow_rolls_back_before_token_transfer() {
    let (mut svm, owner, fixture, trader_balance) = setup_deposit(MarketStatus::Active);
    let mut balance = load_trader_balance(&svm, trader_balance);
    balance.base_free = u64::MAX;
    store_trader_balance(&mut svm, trader_balance, &balance);

    let source = create_test_token_account(&mut svm, fixture.base_mint, owner.pubkey(), 10);
    let instruction = deposit_instruction(
        owner.pubkey(),
        &fixture,
        trader_balance,
        fixture.base_mint,
        source,
        fixture.base_vault,
        1,
    );

    let result = send_instruction(&mut svm, &owner, instruction);
    assert!(result.is_err());
    assert_eq!(load_token_amount(&svm, source), 10);
    assert_eq!(load_token_amount(&svm, fixture.base_vault), 0);
    assert_eq!(
        load_trader_balance(&svm, trader_balance).base_free,
        u64::MAX
    );
}
fn withdraw_instruction(
    owner: Pubkey,
    fixture: &DepositFixture,
    trader_balance: Pubkey,
    withdrawal_mint: Pubkey,
    owner_token_account: Pubkey,
    market_vault: Pubkey,
    amount: u64,
) -> Instruction {
    Instruction::new_with_bytes(
        tidebook::id(),
        &tidebook::instruction::Withdraw { amount }.data(),
        tidebook::accounts::Withdraw {
            owner,
            market: fixture.market,
            trader_balance,
            withdrawal_mint,
            owner_token_account,
            vault_authority: fixture.vault_authority,
            market_vault,
            token_program: anchor_spl::token::ID,
        }
        .to_account_metas(None),
    )
}

fn deposit_for_test(
    svm: &mut LiteSVM,
    owner: &Keypair,
    fixture: &DepositFixture,
    trader_balance: Pubkey,
    mint: Pubkey,
    vault: Pubkey,
    amount: u64,
) {
    let source = create_test_token_account(svm, mint, owner.pubkey(), amount);
    let instruction = deposit_instruction(
        owner.pubkey(),
        fixture,
        trader_balance,
        mint,
        source,
        vault,
        amount,
    );
    let result = send_instruction(svm, owner, instruction);
    assert!(result.is_ok(), "deposit setup failed: {result:?}");
}

#[test]
fn base_withdrawal_debits_only_base_free_and_transfers_tokens() {
    let (mut svm, owner, fixture, trader_balance) = setup_deposit(MarketStatus::Active);
    deposit_for_test(
        &mut svm,
        &owner,
        &fixture,
        trader_balance,
        fixture.base_mint,
        fixture.base_vault,
        120,
    );
    let destination = create_test_token_account(&mut svm, fixture.base_mint, owner.pubkey(), 5);
    let instruction = withdraw_instruction(
        owner.pubkey(),
        &fixture,
        trader_balance,
        fixture.base_mint,
        destination,
        fixture.base_vault,
        50,
    );

    let result = send_instruction(&mut svm, &owner, instruction);
    assert!(result.is_ok(), "base withdrawal failed: {result:?}");

    let event = support::events::single_event::<tidebook::events::WithdrawalEvent>(&result);
    assert_eq!(event.market, fixture.market);
    assert_eq!(event.owner, owner.pubkey());
    assert_eq!(event.trader_balance, trader_balance);
    assert_eq!(event.mint, fixture.base_mint);
    assert_eq!(event.market_vault, fixture.base_vault);
    assert_eq!(event.amount, 50);
    assert_eq!(event.free_balance, 70);

    let balance = load_trader_balance(&svm, trader_balance);
    assert_eq!(load_token_amount(&svm, fixture.base_vault), 70);
    assert_eq!(load_token_amount(&svm, destination), 55);
    assert_eq!(balance.base_free, 70);
    assert_eq!(balance.base_locked, 0);
    assert_eq!(balance.quote_free, 0);
    assert_eq!(balance.quote_locked, 0);
}

#[test]
fn quote_withdrawal_debits_only_quote_free_and_transfers_tokens() {
    let (mut svm, owner, fixture, trader_balance) = setup_deposit(MarketStatus::Active);
    deposit_for_test(
        &mut svm,
        &owner,
        &fixture,
        trader_balance,
        fixture.quote_mint,
        fixture.quote_vault,
        300,
    );
    let destination = create_test_token_account(&mut svm, fixture.quote_mint, owner.pubkey(), 10);
    let instruction = withdraw_instruction(
        owner.pubkey(),
        &fixture,
        trader_balance,
        fixture.quote_mint,
        destination,
        fixture.quote_vault,
        125,
    );

    let result = send_instruction(&mut svm, &owner, instruction);
    assert!(result.is_ok(), "quote withdrawal failed: {result:?}");

    let balance = load_trader_balance(&svm, trader_balance);
    assert_eq!(load_token_amount(&svm, fixture.quote_vault), 175);
    assert_eq!(load_token_amount(&svm, destination), 135);
    assert_eq!(balance.base_free, 0);
    assert_eq!(balance.quote_free, 175);
    assert_eq!(balance.quote_locked, 0);
}

#[test]
fn withdrawal_remains_available_while_market_is_paused() {
    let (mut svm, owner, fixture, trader_balance) = setup_deposit(MarketStatus::Paused);
    deposit_for_test(
        &mut svm,
        &owner,
        &fixture,
        trader_balance,
        fixture.base_mint,
        fixture.base_vault,
        50,
    );
    let destination = create_test_token_account(&mut svm, fixture.base_mint, owner.pubkey(), 0);
    let instruction = withdraw_instruction(
        owner.pubkey(),
        &fixture,
        trader_balance,
        fixture.base_mint,
        destination,
        fixture.base_vault,
        20,
    );

    let result = send_instruction(&mut svm, &owner, instruction);
    assert!(
        result.is_ok(),
        "paused-market withdrawal failed: {result:?}"
    );
    assert_eq!(load_token_amount(&svm, fixture.base_vault), 30);
    assert_eq!(load_token_amount(&svm, destination), 20);
    assert_eq!(load_trader_balance(&svm, trader_balance).base_free, 30);
}

#[test]
fn zero_withdrawal_is_rejected_without_mutation() {
    let (mut svm, owner, fixture, trader_balance) = setup_deposit(MarketStatus::Active);
    deposit_for_test(
        &mut svm,
        &owner,
        &fixture,
        trader_balance,
        fixture.base_mint,
        fixture.base_vault,
        50,
    );
    let destination = create_test_token_account(&mut svm, fixture.base_mint, owner.pubkey(), 3);
    let instruction = withdraw_instruction(
        owner.pubkey(),
        &fixture,
        trader_balance,
        fixture.base_mint,
        destination,
        fixture.base_vault,
        0,
    );

    let result = send_instruction(&mut svm, &owner, instruction);
    assert!(result.is_err());
    assert_eq!(load_token_amount(&svm, fixture.base_vault), 50);
    assert_eq!(load_token_amount(&svm, destination), 3);
    assert_eq!(load_trader_balance(&svm, trader_balance).base_free, 50);
}

#[test]
fn locked_balance_cannot_be_withdrawn() {
    let (mut svm, owner, fixture, trader_balance) = setup_deposit(MarketStatus::Active);
    deposit_for_test(
        &mut svm,
        &owner,
        &fixture,
        trader_balance,
        fixture.base_mint,
        fixture.base_vault,
        100,
    );
    let mut balance = load_trader_balance(&svm, trader_balance);
    balance.base_free = 30;
    balance.base_locked = 70;
    store_trader_balance(&mut svm, trader_balance, &balance);

    let destination = create_test_token_account(&mut svm, fixture.base_mint, owner.pubkey(), 0);
    let instruction = withdraw_instruction(
        owner.pubkey(),
        &fixture,
        trader_balance,
        fixture.base_mint,
        destination,
        fixture.base_vault,
        31,
    );

    let result = send_instruction(&mut svm, &owner, instruction);
    assert!(result.is_err());
    assert_eq!(load_token_amount(&svm, fixture.base_vault), 100);
    assert_eq!(load_token_amount(&svm, destination), 0);
    let balance = load_trader_balance(&svm, trader_balance);
    assert_eq!(balance.base_free, 30);
    assert_eq!(balance.base_locked, 70);
}

#[test]
fn withdrawal_to_another_wallet_is_rejected_without_mutation() {
    let (mut svm, owner, fixture, trader_balance) = setup_deposit(MarketStatus::Active);
    deposit_for_test(
        &mut svm,
        &owner,
        &fixture,
        trader_balance,
        fixture.quote_mint,
        fixture.quote_vault,
        80,
    );
    let destination =
        create_test_token_account(&mut svm, fixture.quote_mint, Pubkey::new_unique(), 0);
    let instruction = withdraw_instruction(
        owner.pubkey(),
        &fixture,
        trader_balance,
        fixture.quote_mint,
        destination,
        fixture.quote_vault,
        20,
    );

    let result = send_instruction(&mut svm, &owner, instruction);
    assert!(result.is_err());
    assert_eq!(load_token_amount(&svm, fixture.quote_vault), 80);
    assert_eq!(load_token_amount(&svm, destination), 0);
    assert_eq!(load_trader_balance(&svm, trader_balance).quote_free, 80);
}

#[test]
fn non_market_withdrawal_mint_is_rejected_without_mutation() {
    let (mut svm, owner, fixture, trader_balance) = setup_deposit(MarketStatus::Active);
    let unrelated_mint = store_test_mint(&mut svm, 6);
    let destination = create_test_token_account(&mut svm, unrelated_mint, owner.pubkey(), 0);
    let (unrelated_vault, _) = Pubkey::find_program_address(
        &[
            tidebook::constants::VAULT_SEED,
            fixture.market.as_ref(),
            unrelated_mint.as_ref(),
        ],
        &tidebook::id(),
    );
    store_test_token_account(
        &mut svm,
        unrelated_vault,
        unrelated_mint,
        fixture.vault_authority,
        25,
    );
    let instruction = withdraw_instruction(
        owner.pubkey(),
        &fixture,
        trader_balance,
        unrelated_mint,
        destination,
        unrelated_vault,
        10,
    );

    let result = send_instruction(&mut svm, &owner, instruction);
    assert!(result.is_err());
    assert_eq!(load_token_amount(&svm, unrelated_vault), 25);
    assert_eq!(load_token_amount(&svm, destination), 0);
    let balance = load_trader_balance(&svm, trader_balance);
    assert_eq!(balance.base_free, 0);
    assert_eq!(balance.quote_free, 0);
}

#[test]
fn insufficient_vault_backing_rejects_withdrawal_without_ledger_mutation() {
    let (mut svm, owner, fixture, trader_balance) = setup_deposit(MarketStatus::Active);
    deposit_for_test(
        &mut svm,
        &owner,
        &fixture,
        trader_balance,
        fixture.base_mint,
        fixture.base_vault,
        100,
    );

    // Simulate corrupted custody backing: accounting says 100, vault holds 20.
    store_test_token_account(
        &mut svm,
        fixture.base_vault,
        fixture.base_mint,
        fixture.vault_authority,
        20,
    );

    let destination = create_test_token_account(&mut svm, fixture.base_mint, owner.pubkey(), 0);
    let instruction = withdraw_instruction(
        owner.pubkey(),
        &fixture,
        trader_balance,
        fixture.base_mint,
        destination,
        fixture.base_vault,
        30,
    );

    let result = send_instruction(&mut svm, &owner, instruction);
    assert!(result.is_err());
    assert_eq!(load_token_amount(&svm, fixture.base_vault), 20);
    assert_eq!(load_token_amount(&svm, destination), 0);
    assert_eq!(load_trader_balance(&svm, trader_balance).base_free, 100);
}

#[test]
fn deposit_place_cancel_and_withdraw_preserve_vault_backing() {
    const PRICE: u64 = 10_000;
    const QUANTITY: u64 = 1_000_000;
    const DEPOSIT_AMOUNT: u64 = 25;
    const LOCKED_QUOTE: u64 = 10;

    let (mut svm, owner, fixture, trader_balance) = setup_deposit(MarketStatus::Active);
    let source =
        create_test_token_account(&mut svm, fixture.quote_mint, owner.pubkey(), DEPOSIT_AMOUNT);
    let deposit = deposit_instruction(
        owner.pubkey(),
        &fixture,
        trader_balance,
        fixture.quote_mint,
        source,
        fixture.quote_vault,
        DEPOSIT_AMOUNT,
    );
    let deposit_result = send_instruction(&mut svm, &owner, deposit);
    assert!(deposit_result.is_ok(), "deposit failed: {deposit_result:?}");

    let (order, _) = Pubkey::find_program_address(
        &[
            tidebook::constants::ORDER_SEED,
            fixture.market.as_ref(),
            1_u64.to_le_bytes().as_ref(),
        ],
        &tidebook::id(),
    );
    let (price_level, _) =
        tidebook::derive_price_level_pda(&tidebook::id(), &fixture.market, OrderSide::Bid, PRICE);
    let place = Instruction::new_with_bytes(
        tidebook::id(),
        &tidebook::instruction::InsertLimitOrder {
            side: OrderSide::Bid,
            price: PRICE,
            quantity: QUANTITY,
        }
        .data(),
        tidebook::accounts::InsertLimitOrder {
            trader: owner.pubkey(),
            market: fixture.market,
            order,
            price_level,
            trader_balance,
            system_program: system_program::ID,
            better_level: None,
            worse_level: None,
        }
        .to_account_metas(None),
    );
    let place_result = send_instruction(&mut svm, &owner, place);
    assert!(place_result.is_ok(), "placement failed: {place_result:?}");

    let placed_balance = load_trader_balance(&svm, trader_balance);
    assert_eq!(placed_balance.quote_free, DEPOSIT_AMOUNT - LOCKED_QUOTE);
    assert_eq!(placed_balance.quote_locked, LOCKED_QUOTE);
    assert_eq!(load_token_amount(&svm, fixture.quote_vault), DEPOSIT_AMOUNT);

    let cancel = Instruction::new_with_bytes(
        tidebook::id(),
        &tidebook::instruction::CancelLimitOrder { order_id: 1 }.data(),
        tidebook::accounts::CancelLimitOrder {
            owner: owner.pubkey(),
            market: fixture.market,
            order,
            price_level,
            previous_order: None,
            next_order: None,
            better_level: None,
            worse_level: None,
            level_rent_recipient: Some(owner.pubkey()),
            trader_balance,
        }
        .to_account_metas(None),
    );
    let cancel_result = send_instruction(&mut svm, &owner, cancel);
    assert!(
        cancel_result.is_ok(),
        "cancellation failed: {cancel_result:?}"
    );

    let canceled_balance = load_trader_balance(&svm, trader_balance);
    assert_eq!(canceled_balance.quote_free, DEPOSIT_AMOUNT);
    assert_eq!(canceled_balance.quote_locked, 0);
    assert_eq!(load_token_amount(&svm, fixture.quote_vault), DEPOSIT_AMOUNT);

    let destination = create_test_token_account(&mut svm, fixture.quote_mint, owner.pubkey(), 0);
    let withdraw = withdraw_instruction(
        owner.pubkey(),
        &fixture,
        trader_balance,
        fixture.quote_mint,
        destination,
        fixture.quote_vault,
        DEPOSIT_AMOUNT,
    );
    let withdraw_result = send_instruction(&mut svm, &owner, withdraw);
    assert!(
        withdraw_result.is_ok(),
        "withdrawal failed: {withdraw_result:?}"
    );

    let final_balance = load_trader_balance(&svm, trader_balance);
    assert_eq!(final_balance.quote_free, 0);
    assert_eq!(final_balance.quote_locked, 0);
    assert_eq!(load_token_amount(&svm, fixture.quote_vault), 0);
    assert_eq!(load_token_amount(&svm, destination), DEPOSIT_AMOUNT);
}
