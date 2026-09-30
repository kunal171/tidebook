//! LiteSVM integration coverage for protocol-fee withdrawal, safe market
//! shutdown, and vault closure.

// LiteSVM intentionally returns rich transaction-failure metadata. Boxing it in
// every test helper would add indirection without reducing production account or
// instruction size, so this integration-test boundary permits the large error.
#![allow(clippy::result_large_err)]

mod support;

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
    tidebook::state::{AdminRecord, AdminStatus, Market, OrderSide, TestFaucet},
};

const PROGRAM_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../target/deploy/tidebook.so"
));

const ORDER_ID: u64 = 1;
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

const TEST_PRICE_TICK_SIZE: u64 = 10_000;
const TEST_QUANTITY_LOT_SIZE: u64 = 1_000_000;
const TEST_ORDER_PRICE: u64 = 100_000_000;
const TEST_ORDER_QUANTITY: u64 = 5_000_000;

struct MarketFixture {
    market: Pubkey,
    market_fees: Pubkey,
    base_mint: Pubkey,
    quote_mint: Pubkey,
    vault_authority: Pubkey,
    base_vault: Pubkey,
    quote_vault: Pubkey,
}

struct TestContext {
    svm: LiteSVM,
    authority: Keypair,
    market: MarketFixture,
}

struct OpenOrderFixture {
    order: Pubkey,
    price_level: Pubkey,
}

fn send_instruction(
    svm: &mut LiteSVM,
    payer: &Keypair,
    instruction: Instruction,
) -> litesvm::types::TransactionResult {
    send_instructions(svm, payer, &[instruction])
}

fn send_instructions(
    svm: &mut LiteSVM,
    payer: &Keypair,
    instructions: &[Instruction],
) -> litesvm::types::TransactionResult {
    svm.expire_blockhash();
    let message =
        Message::new_with_blockhash(instructions, Some(&payer.pubkey()), &svm.latest_blockhash());
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

fn set_token_balance(svm: &mut LiteSVM, address: Pubkey, amount: u64) {
    let mut account = svm.get_account(&address).unwrap();
    let mut token_state = SplTokenAccount::unpack(&account.data).unwrap();
    token_state.amount = amount;
    SplTokenAccount::pack(token_state, &mut account.data).unwrap();
    svm.set_account(address, account).unwrap();
}

fn token_balance(svm: &LiteSVM, address: Pubkey) -> u64 {
    let account = svm.get_account(&address).unwrap();
    SplTokenAccount::unpack(&account.data).unwrap().amount
}

fn load_market_fees(svm: &LiteSVM, address: Pubkey) -> tidebook::state::MarketFees {
    let account = svm.get_account(&address).unwrap();
    let mut data: &[u8] = &account.data;
    tidebook::state::MarketFees::try_deserialize(&mut data).unwrap()
}

fn mutate_market_fees(
    svm: &mut LiteSVM,
    address: Pubkey,
    mutate: impl FnOnce(&mut tidebook::state::MarketFees),
) {
    let mut account = svm.get_account(&address).unwrap();
    let mut account_data: &[u8] = &account.data;
    let mut state = tidebook::state::MarketFees::try_deserialize(&mut account_data).unwrap();
    mutate(&mut state);

    let mut serialized = Vec::new();
    state.try_serialize(&mut serialized).unwrap();
    account.data = serialized;
    svm.set_account(address, account).unwrap();
}

fn store_noncanonical_market_fees(svm: &mut LiteSVM, market: Pubkey, quote_mint: Pubkey) -> Pubkey {
    let address = Pubkey::new_unique();
    let state = tidebook::state::MarketFees {
        market,
        quote_mint,
        accrued_quote_fees: 0,
        bump: 0,
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

fn store_protocol_config(svm: &mut LiteSVM, super_admin: Pubkey) -> Pubkey {
    let (address, bump) = Pubkey::find_program_address(
        &[tidebook::constants::PROTOCOL_CONFIG_SEED],
        &tidebook::id(),
    );
    let state = tidebook::state::ProtocolConfig {
        super_admin,
        taker_fee_bps: 25,
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

    address
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
    let (market_fees, _) = tidebook::derive_market_fees_pda(&tidebook::id(), &market);
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
            market_fees,
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
        market_fees,
        base_mint,
        quote_mint,
        vault_authority,
        base_vault,
        quote_vault,
    }
}

fn setup_market() -> TestContext {
    let mut svm = LiteSVM::new();
    let authority = Keypair::new();
    svm.add_program(tidebook::id(), PROGRAM_BYTES).unwrap();
    svm.airdrop(&authority.pubkey(), 2_000_000_000).unwrap();
    store_active_admin(&mut svm, authority.pubkey());
    store_protocol_config(&mut svm, authority.pubkey());
    let market = initialize_market(&mut svm, &authority);

    TestContext {
        svm,
        authority,
        market,
    }
}

fn load_market(svm: &LiteSVM, address: Pubkey) -> Market {
    let account = svm.get_account(&address).unwrap();
    let mut data: &[u8] = &account.data;
    Market::try_deserialize(&mut data).unwrap()
}

fn pause_market(context: &mut TestContext) {
    let instruction = Instruction::new_with_bytes(
        tidebook::id(),
        &tidebook::instruction::PauseMarket {}.data(),
        tidebook::accounts::PauseMarket {
            authority: context.authority.pubkey(),
            market: context.market.market,
        }
        .to_account_metas(None),
    );
    let result = send_instruction(&mut context.svm, &context.authority, instruction);
    assert!(result.is_ok(), "market pause failed: {result:?}");
}

fn withdraw_protocol_fees_instruction(
    super_admin: Pubkey,
    market: &MarketFixture,
    destination_quote_account: Pubkey,
    amount: u64,
) -> Instruction {
    Instruction::new_with_bytes(
        tidebook::id(),
        &tidebook::instruction::WithdrawProtocolFees { amount }.data(),
        tidebook::accounts::WithdrawProtocolFees {
            super_admin,
            protocol_config: Pubkey::find_program_address(
                &[tidebook::constants::PROTOCOL_CONFIG_SEED],
                &tidebook::id(),
            )
            .0,
            market: market.market,
            market_fees: market.market_fees,
            quote_mint: market.quote_mint,
            destination_quote_account,
            vault_authority: market.vault_authority,
            quote_vault: market.quote_vault,
            token_program: anchor_spl::token::ID,
        }
        .to_account_metas(None),
    )
}

#[allow(clippy::too_many_arguments)]
fn close_market_instruction_with_accounts(
    authority: Pubkey,
    market: Pubkey,
    market_fees: Pubkey,
    vault_authority: Pubkey,
    base_vault: Pubkey,
    quote_vault: Pubkey,
) -> Instruction {
    Instruction::new_with_bytes(
        tidebook::id(),
        &tidebook::instruction::CloseMarket {}.data(),
        tidebook::accounts::CloseMarket {
            authority,
            market,
            market_fees,
            test_faucet: Pubkey::find_program_address(
                &[tidebook::constants::TEST_FAUCET_SEED, market.as_ref()],
                &tidebook::id(),
            )
            .0,
            vault_authority,
            base_vault,
            quote_vault,
            token_program: anchor_spl::token::ID,
        }
        .to_account_metas(None),
    )
}

fn close_market_instruction(authority: Pubkey, market: &MarketFixture) -> Instruction {
    close_market_instruction_with_accounts(
        authority,
        market.market,
        market.market_fees,
        market.vault_authority,
        market.base_vault,
        market.quote_vault,
    )
}

fn close_market_instruction_with_faucet(
    authority: Pubkey,
    market: &MarketFixture,
    test_faucet: Pubkey,
) -> Instruction {
    Instruction::new_with_bytes(
        tidebook::id(),
        &tidebook::instruction::CloseMarket {}.data(),
        tidebook::accounts::CloseMarket {
            authority,
            market: market.market,
            market_fees: market.market_fees,
            test_faucet,
            vault_authority: market.vault_authority,
            base_vault: market.base_vault,
            quote_vault: market.quote_vault,
            token_program: anchor_spl::token::ID,
        }
        .to_account_metas(None),
    )
}

fn close_test_faucet_instruction(
    authority: Pubkey,
    market: &MarketFixture,
    test_faucet: Pubkey,
) -> Instruction {
    Instruction::new_with_bytes(
        tidebook::id(),
        &tidebook::instruction::CloseTestFaucet {}.data(),
        tidebook::accounts::CloseTestFaucet {
            authority,
            market: market.market,
            test_faucet,
        }
        .to_account_metas(None),
    )
}

fn store_test_faucet(svm: &mut LiteSVM, market: &MarketFixture) -> Pubkey {
    let (test_faucet, bump) = Pubkey::find_program_address(
        &[
            tidebook::constants::TEST_FAUCET_SEED,
            market.market.as_ref(),
        ],
        &tidebook::id(),
    );
    let (_, authority_bump) = Pubkey::find_program_address(
        &[
            tidebook::constants::TEST_FAUCET_AUTHORITY_SEED,
            test_faucet.as_ref(),
        ],
        &tidebook::id(),
    );
    let state = TestFaucet {
        market: market.market,
        base_mint: market.base_mint,
        quote_mint: market.quote_mint,
        base_claim_amount: 10_000_000_000,
        quote_claim_amount: 10_000_000_000,
        authority_bump,
        bump,
    };
    let mut data = Vec::new();
    state.try_serialize(&mut data).unwrap();
    svm.set_account(
        test_faucet,
        Account {
            lamports: svm.minimum_balance_for_rent_exemption(data.len()),
            data,
            owner: tidebook::id(),
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();
    test_faucet
}

fn place_order(context: &mut TestContext, side: OrderSide) -> OpenOrderFixture {
    let (order, _) = Pubkey::find_program_address(
        &[
            tidebook::constants::ORDER_SEED,
            context.market.market.as_ref(),
            ORDER_ID.to_le_bytes().as_ref(),
        ],
        &tidebook::id(),
    );
    let (price_level, _) = tidebook::derive_price_level_pda(
        &tidebook::id(),
        &context.market.market,
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
            trader: context.authority.pubkey(),
            market: context.market.market,
            order,
            price_level,
            trader_balance: ensure_trader_balance(
                &mut context.svm,
                context.market.market,
                context.authority.pubkey(),
            ),
            system_program: system_program::ID,
            better_level: None,
            worse_level: None,
        }
        .to_account_metas(None),
    );
    let result = send_instruction(&mut context.svm, &context.authority, instruction);
    assert!(result.is_ok(), "order placement failed: {result:?}");

    OpenOrderFixture { order, price_level }
}

fn cancel_order(context: &mut TestContext, order: &OpenOrderFixture) {
    let instruction = Instruction::new_with_bytes(
        tidebook::id(),
        &tidebook::instruction::CancelLimitOrder { order_id: ORDER_ID }.data(),
        tidebook::accounts::CancelLimitOrder {
            owner: context.authority.pubkey(),
            market: context.market.market,
            order: order.order,
            price_level: order.price_level,
            previous_order: None,
            next_order: None,
            better_level: None,
            worse_level: None,
            level_rent_recipient: Some(context.authority.pubkey()),
            trader_balance: trader_balance_address(
                context.market.market,
                context.authority.pubkey(),
            ),
        }
        .to_account_metas(None),
    );
    let result = send_instruction(&mut context.svm, &context.authority, instruction);
    assert!(result.is_ok(), "order cancellation failed: {result:?}");
}

#[test]
fn active_market_cannot_close() {
    let mut context = setup_market();
    let instruction = close_market_instruction(context.authority.pubkey(), &context.market);

    let result = send_instruction(&mut context.svm, &context.authority, instruction);

    assert!(result.is_err(), "active market was closed");
    assert!(context.svm.get_account(&context.market.market).is_some());
    assert!(context
        .svm
        .get_account(&context.market.base_vault)
        .is_some());
    assert!(context
        .svm
        .get_account(&context.market.quote_vault)
        .is_some());
}

#[test]
fn paused_empty_market_closes_market_and_both_vaults() {
    let mut context = setup_market();
    pause_market(&mut context);
    let instruction = close_market_instruction(context.authority.pubkey(), &context.market);

    let result = send_instruction(&mut context.svm, &context.authority, instruction);

    assert!(result.is_ok(), "empty market closure failed: {result:?}");

    let event = support::events::single_event::<tidebook::events::MarketClosedEvent>(&result);
    assert_eq!(event.market, context.market.market);
    assert_eq!(event.market_fees, context.market.market_fees);
    assert_eq!(event.authority, context.authority.pubkey());
    assert_eq!(event.base_mint, context.market.base_mint);
    assert_eq!(event.quote_mint, context.market.quote_mint);
    assert_eq!(event.base_vault, context.market.base_vault);
    assert_eq!(event.quote_vault, context.market.quote_vault);

    assert!(context.svm.get_account(&context.market.market).is_none());
    assert!(context
        .svm
        .get_account(&context.market.market_fees)
        .is_none());
    assert!(context
        .svm
        .get_account(&context.market.base_vault)
        .is_none());
    assert!(context
        .svm
        .get_account(&context.market.quote_vault)
        .is_none());
}

#[test]
fn paused_market_with_live_faucet_cannot_close_directly() {
    let mut context = setup_market();
    let test_faucet = store_test_faucet(&mut context.svm, &context.market);
    pause_market(&mut context);
    let instruction = close_market_instruction_with_faucet(
        context.authority.pubkey(),
        &context.market,
        test_faucet,
    );
    let result = send_instruction(&mut context.svm, &context.authority, instruction);

    assert!(result.is_err(), "market orphaned a live faucet");
    assert!(context.svm.get_account(&context.market.market).is_some());
    assert!(context.svm.get_account(&test_faucet).is_some());
}

#[test]
fn faucet_and_paused_market_close_atomically() {
    let mut context = setup_market();
    let test_faucet = store_test_faucet(&mut context.svm, &context.market);
    pause_market(&mut context);

    let close_faucet =
        close_test_faucet_instruction(context.authority.pubkey(), &context.market, test_faucet);
    let close_market = close_market_instruction_with_faucet(
        context.authority.pubkey(),
        &context.market,
        test_faucet,
    );
    let result = send_instructions(
        &mut context.svm,
        &context.authority,
        &[close_faucet, close_market],
    );

    assert!(
        result.is_ok(),
        "market and faucet closure failed: {result:?}"
    );
    let event = support::events::single_event::<tidebook::events::TestFaucetClosedEvent>(&result);
    assert_eq!(event.market, context.market.market);
    assert_eq!(event.test_faucet, test_faucet);
    assert_eq!(event.authority, context.authority.pubkey());
    assert_eq!(event.base_mint, context.market.base_mint);
    assert_eq!(event.quote_mint, context.market.quote_mint);

    assert!(context.svm.get_account(&context.market.market).is_none());
    assert!(context.svm.get_account(&test_faucet).is_none());
    assert!(context
        .svm
        .get_account(&context.market.market_fees)
        .is_none());
}

#[test]
fn paused_market_with_open_bid_cannot_close() {
    let mut context = setup_market();
    place_order(&mut context, OrderSide::Bid);
    assert_eq!(
        load_market(&context.svm, context.market.market).open_order_count,
        1
    );
    pause_market(&mut context);
    let instruction = close_market_instruction(context.authority.pubkey(), &context.market);

    let result = send_instruction(&mut context.svm, &context.authority, instruction);

    assert!(result.is_err(), "market with an open bid was closed");
    assert!(context.svm.get_account(&context.market.market).is_some());
    assert!(context
        .svm
        .get_account(&context.market.quote_vault)
        .is_some());
}

#[test]
fn paused_market_with_open_ask_cannot_close() {
    let mut context = setup_market();
    place_order(&mut context, OrderSide::Ask);
    assert_eq!(
        load_market(&context.svm, context.market.market).open_order_count,
        1
    );
    pause_market(&mut context);
    let instruction = close_market_instruction(context.authority.pubkey(), &context.market);

    let result = send_instruction(&mut context.svm, &context.authority, instruction);

    assert!(result.is_err(), "market with an open ask was closed");
    assert!(context.svm.get_account(&context.market.market).is_some());
    assert!(context
        .svm
        .get_account(&context.market.base_vault)
        .is_some());
}

#[test]
fn final_cancellation_allows_paused_market_to_close() {
    let mut context = setup_market();
    let order = place_order(&mut context, OrderSide::Bid);
    assert_eq!(
        load_market(&context.svm, context.market.market).open_order_count,
        1
    );
    pause_market(&mut context);

    cancel_order(&mut context, &order);
    assert_eq!(
        load_market(&context.svm, context.market.market).open_order_count,
        0
    );

    let instruction = close_market_instruction(context.authority.pubkey(), &context.market);
    let result = send_instruction(&mut context.svm, &context.authority, instruction);

    assert!(
        result.is_ok(),
        "market closure after cancellation failed: {result:?}"
    );
    assert!(context.svm.get_account(&context.market.market).is_none());
    assert!(context
        .svm
        .get_account(&context.market.market_fees)
        .is_none());
    assert!(context
        .svm
        .get_account(&context.market.base_vault)
        .is_none());
    assert!(context
        .svm
        .get_account(&context.market.quote_vault)
        .is_none());
}

#[test]
fn nonzero_vault_balance_prevents_closure_without_open_orders() {
    let mut context = setup_market();
    set_token_balance(&mut context.svm, context.market.base_vault, 1);
    assert_eq!(
        load_market(&context.svm, context.market.market).open_order_count,
        0
    );
    pause_market(&mut context);
    let instruction = close_market_instruction(context.authority.pubkey(), &context.market);

    let result = send_instruction(&mut context.svm, &context.authority, instruction);

    assert!(result.is_err(), "market with a funded vault was closed");
    assert!(context.svm.get_account(&context.market.market).is_some());
    assert!(context
        .svm
        .get_account(&context.market.base_vault)
        .is_some());
    assert!(context
        .svm
        .get_account(&context.market.quote_vault)
        .is_some());
}

#[test]
fn accrued_protocol_fees_prevent_market_closure() {
    let mut context = setup_market();
    mutate_market_fees(&mut context.svm, context.market.market_fees, |fees| {
        fees.accrued_quote_fees = 1
    });
    pause_market(&mut context);
    let instruction = close_market_instruction(context.authority.pubkey(), &context.market);

    let result = send_instruction(&mut context.svm, &context.authority, instruction);

    assert!(result.is_err(), "market discarded accrued protocol fees");
    assert!(context.svm.get_account(&context.market.market).is_some());
    assert!(context
        .svm
        .get_account(&context.market.market_fees)
        .is_some());
    assert!(context
        .svm
        .get_account(&context.market.base_vault)
        .is_some());
    assert!(context
        .svm
        .get_account(&context.market.quote_vault)
        .is_some());
}

#[test]
fn noncanonical_market_fees_account_is_rejected() {
    let mut context = setup_market();
    pause_market(&mut context);
    let noncanonical_market_fees = store_noncanonical_market_fees(
        &mut context.svm,
        context.market.market,
        context.market.quote_mint,
    );
    let instruction = close_market_instruction_with_accounts(
        context.authority.pubkey(),
        context.market.market,
        noncanonical_market_fees,
        context.market.vault_authority,
        context.market.base_vault,
        context.market.quote_vault,
    );

    let result = send_instruction(&mut context.svm, &context.authority, instruction);

    assert!(result.is_err(), "noncanonical market-fees PDA was accepted");
    assert!(context.svm.get_account(&context.market.market).is_some());
    assert!(context
        .svm
        .get_account(&context.market.market_fees)
        .is_some());
}

#[test]
fn market_fees_for_a_different_market_are_rejected() {
    let mut context = setup_market();
    mutate_market_fees(&mut context.svm, context.market.market_fees, |fees| {
        fees.market = Pubkey::new_unique()
    });
    pause_market(&mut context);
    let instruction = close_market_instruction(context.authority.pubkey(), &context.market);

    let result = send_instruction(&mut context.svm, &context.authority, instruction);

    assert!(result.is_err(), "mismatched fee market was accepted");
    assert!(context.svm.get_account(&context.market.market).is_some());
    assert!(context
        .svm
        .get_account(&context.market.market_fees)
        .is_some());
}

#[test]
fn market_fees_with_a_different_quote_mint_are_rejected() {
    let mut context = setup_market();
    mutate_market_fees(&mut context.svm, context.market.market_fees, |fees| {
        fees.quote_mint = Pubkey::new_unique()
    });
    pause_market(&mut context);
    let instruction = close_market_instruction(context.authority.pubkey(), &context.market);

    let result = send_instruction(&mut context.svm, &context.authority, instruction);

    assert!(result.is_err(), "mismatched fee quote mint was accepted");
    assert!(context.svm.get_account(&context.market.market).is_some());
    assert!(context
        .svm
        .get_account(&context.market.market_fees)
        .is_some());
}

#[test]
fn noncanonical_base_vault_is_rejected() {
    let mut context = setup_market();
    pause_market(&mut context);
    let noncanonical_base_vault = create_test_token_account(
        &mut context.svm,
        context.market.base_mint,
        context.market.vault_authority,
        0,
    );
    let instruction = close_market_instruction_with_accounts(
        context.authority.pubkey(),
        context.market.market,
        context.market.market_fees,
        context.market.vault_authority,
        noncanonical_base_vault,
        context.market.quote_vault,
    );

    let result = send_instruction(&mut context.svm, &context.authority, instruction);

    assert!(result.is_err(), "noncanonical base vault was accepted");
    assert!(context.svm.get_account(&context.market.market).is_some());
    assert!(context
        .svm
        .get_account(&context.market.base_vault)
        .is_some());
    assert!(context
        .svm
        .get_account(&context.market.quote_vault)
        .is_some());
}

#[test]
fn noncanonical_quote_vault_is_rejected() {
    let mut context = setup_market();
    pause_market(&mut context);
    let noncanonical_quote_vault = create_test_token_account(
        &mut context.svm,
        context.market.quote_mint,
        context.market.vault_authority,
        0,
    );
    let instruction = close_market_instruction_with_accounts(
        context.authority.pubkey(),
        context.market.market,
        context.market.market_fees,
        context.market.vault_authority,
        context.market.base_vault,
        noncanonical_quote_vault,
    );

    let result = send_instruction(&mut context.svm, &context.authority, instruction);

    assert!(result.is_err(), "noncanonical quote vault was accepted");
    assert!(context.svm.get_account(&context.market.market).is_some());
    assert!(context
        .svm
        .get_account(&context.market.base_vault)
        .is_some());
    assert!(context
        .svm
        .get_account(&context.market.quote_vault)
        .is_some());
}

#[test]
fn non_authority_cannot_close_market() {
    let mut context = setup_market();
    pause_market(&mut context);
    let attacker = Keypair::new();
    context
        .svm
        .airdrop(&attacker.pubkey(), 1_000_000_000)
        .unwrap();
    let instruction = close_market_instruction(attacker.pubkey(), &context.market);

    let result = send_instruction(&mut context.svm, &attacker, instruction);

    assert!(result.is_err(), "non-authority closed the market");
    assert!(context.svm.get_account(&context.market.market).is_some());
    assert!(context
        .svm
        .get_account(&context.market.base_vault)
        .is_some());
    assert!(context
        .svm
        .get_account(&context.market.quote_vault)
        .is_some());
}

#[test]
fn super_admin_withdraws_accrued_quote_fees_to_a_treasury() {
    let mut context = setup_market();
    let treasury_owner = Pubkey::new_unique();
    let treasury = create_test_token_account(
        &mut context.svm,
        context.market.quote_mint,
        treasury_owner,
        0,
    );
    mutate_market_fees(&mut context.svm, context.market.market_fees, |fees| {
        fees.accrued_quote_fees = 100;
    });
    set_token_balance(&mut context.svm, context.market.quote_vault, 100);
    let instruction = withdraw_protocol_fees_instruction(
        context.authority.pubkey(),
        &context.market,
        treasury,
        60,
    );

    let result = send_instruction(&mut context.svm, &context.authority, instruction);

    assert!(result.is_ok(), "fee withdrawal failed: {result:?}");
    assert_eq!(token_balance(&context.svm, context.market.quote_vault), 40);
    assert_eq!(token_balance(&context.svm, treasury), 60);
    assert_eq!(
        load_market_fees(&context.svm, context.market.market_fees).accrued_quote_fees,
        40
    );

    let event =
        support::events::single_event::<tidebook::events::ProtocolFeesWithdrawnEvent>(&result);
    assert_eq!(event.market, context.market.market);
    assert_eq!(event.market_fees, context.market.market_fees);
    assert_eq!(event.super_admin, context.authority.pubkey());
    assert_eq!(event.quote_mint, context.market.quote_mint);
    assert_eq!(event.quote_vault, context.market.quote_vault);
    assert_eq!(event.destination_quote_account, treasury);
    assert_eq!(event.amount, 60);
    assert_eq!(event.remaining_accrued_quote_fees, 40);
}

#[test]
fn full_fee_withdrawal_allows_a_paused_market_to_close() {
    let mut context = setup_market();
    let treasury = create_test_token_account(
        &mut context.svm,
        context.market.quote_mint,
        Pubkey::new_unique(),
        0,
    );
    mutate_market_fees(&mut context.svm, context.market.market_fees, |fees| {
        fees.accrued_quote_fees = 100;
    });
    set_token_balance(&mut context.svm, context.market.quote_vault, 100);
    pause_market(&mut context);

    let withdrawal = withdraw_protocol_fees_instruction(
        context.authority.pubkey(),
        &context.market,
        treasury,
        100,
    );
    let withdrawal_result = send_instruction(&mut context.svm, &context.authority, withdrawal);
    assert!(withdrawal_result.is_ok());

    let close = close_market_instruction(context.authority.pubkey(), &context.market);
    let close_result = send_instruction(&mut context.svm, &context.authority, close);

    assert!(close_result.is_ok(), "closure after fee withdrawal failed");
    assert_eq!(token_balance(&context.svm, treasury), 100);
    assert!(context.svm.get_account(&context.market.market).is_none());
    assert!(context
        .svm
        .get_account(&context.market.market_fees)
        .is_none());
    assert!(context
        .svm
        .get_account(&context.market.quote_vault)
        .is_none());
}

#[test]
fn non_super_admin_cannot_withdraw_protocol_fees() {
    let mut context = setup_market();
    let attacker = Keypair::new();
    context
        .svm
        .airdrop(&attacker.pubkey(), 1_000_000_000)
        .unwrap();
    let treasury = create_test_token_account(
        &mut context.svm,
        context.market.quote_mint,
        attacker.pubkey(),
        0,
    );
    mutate_market_fees(&mut context.svm, context.market.market_fees, |fees| {
        fees.accrued_quote_fees = 100;
    });
    set_token_balance(&mut context.svm, context.market.quote_vault, 100);
    let instruction =
        withdraw_protocol_fees_instruction(attacker.pubkey(), &context.market, treasury, 60);

    let result = send_instruction(&mut context.svm, &attacker, instruction);

    assert!(result.is_err(), "non-super-admin withdrew fees");
    assert_eq!(token_balance(&context.svm, context.market.quote_vault), 100);
    assert_eq!(token_balance(&context.svm, treasury), 0);
    assert_eq!(
        load_market_fees(&context.svm, context.market.market_fees).accrued_quote_fees,
        100
    );
}

#[test]
fn invalid_fee_withdrawal_amounts_are_rejected_without_mutation() {
    let mut context = setup_market();
    let treasury = create_test_token_account(
        &mut context.svm,
        context.market.quote_mint,
        Pubkey::new_unique(),
        0,
    );
    mutate_market_fees(&mut context.svm, context.market.market_fees, |fees| {
        fees.accrued_quote_fees = 100;
    });
    set_token_balance(&mut context.svm, context.market.quote_vault, 100);

    for amount in [0, 101] {
        let instruction = withdraw_protocol_fees_instruction(
            context.authority.pubkey(),
            &context.market,
            treasury,
            amount,
        );
        let result = send_instruction(&mut context.svm, &context.authority, instruction);
        assert!(result.is_err(), "invalid amount {amount} was accepted");
    }

    assert_eq!(token_balance(&context.svm, context.market.quote_vault), 100);
    assert_eq!(token_balance(&context.svm, treasury), 0);
    assert_eq!(
        load_market_fees(&context.svm, context.market.market_fees).accrued_quote_fees,
        100
    );
}

#[test]
fn underfunded_quote_vault_rejects_fee_withdrawal_atomically() {
    let mut context = setup_market();
    let treasury = create_test_token_account(
        &mut context.svm,
        context.market.quote_mint,
        Pubkey::new_unique(),
        0,
    );
    mutate_market_fees(&mut context.svm, context.market.market_fees, |fees| {
        fees.accrued_quote_fees = 100;
    });
    set_token_balance(&mut context.svm, context.market.quote_vault, 50);
    let instruction = withdraw_protocol_fees_instruction(
        context.authority.pubkey(),
        &context.market,
        treasury,
        60,
    );

    let result = send_instruction(&mut context.svm, &context.authority, instruction);

    assert!(result.is_err(), "underfunded vault paid protocol fees");
    assert_eq!(token_balance(&context.svm, context.market.quote_vault), 50);
    assert_eq!(token_balance(&context.svm, treasury), 0);
    assert_eq!(
        load_market_fees(&context.svm, context.market.market_fees).accrued_quote_fees,
        100
    );
}

#[test]
fn wrong_mint_treasury_is_rejected() {
    let mut context = setup_market();
    let treasury = create_test_token_account(
        &mut context.svm,
        context.market.base_mint,
        Pubkey::new_unique(),
        0,
    );
    mutate_market_fees(&mut context.svm, context.market.market_fees, |fees| {
        fees.accrued_quote_fees = 100;
    });
    set_token_balance(&mut context.svm, context.market.quote_vault, 100);
    let instruction = withdraw_protocol_fees_instruction(
        context.authority.pubkey(),
        &context.market,
        treasury,
        60,
    );

    let result = send_instruction(&mut context.svm, &context.authority, instruction);

    assert!(result.is_err(), "base-mint treasury accepted quote fees");
    assert_eq!(token_balance(&context.svm, context.market.quote_vault), 100);
    assert_eq!(token_balance(&context.svm, treasury), 0);
    assert_eq!(
        load_market_fees(&context.svm, context.market.market_fees).accrued_quote_fees,
        100
    );
}
