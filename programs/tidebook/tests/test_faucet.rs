//! LiteSVM coverage for governance-controlled, permissionless test-token claims.

#![allow(clippy::result_large_err)]

mod support;

use {
    anchor_lang::{
        prelude::Pubkey,
        solana_program::{
            instruction::Instruction, program_option::COption, program_pack::Pack, system_program,
        },
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
    tidebook::state::{Market, MarketStatus, ProtocolConfig, TestFaucet},
};

const PROGRAM_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../target/deploy/tidebook.so"
));

const BASE_CLAIM: u64 = 10_000_000_000;
const QUOTE_CLAIM: u64 = 10_000_000_000;

struct Fixture {
    svm: LiteSVM,
    super_admin: Keypair,
    market: Pubkey,
    protocol_config: Pubkey,
    base_mint: Pubkey,
    quote_mint: Pubkey,
}

fn store_anchor_account<T: AccountSerialize + Space>(
    svm: &mut LiteSVM,
    address: Pubkey,
    state: &T,
) {
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

fn store_mint(svm: &mut LiteSVM, address: Pubkey, authority: Pubkey, decimals: u8, supply: u64) {
    let state = SplMint {
        mint_authority: COption::Some(authority),
        supply,
        decimals,
        is_initialized: true,
        freeze_authority: COption::None,
    };
    let mut data = vec![0_u8; SplMint::LEN];
    SplMint::pack(state, &mut data).unwrap();
    svm.set_account(
        address,
        Account {
            lamports: svm.minimum_balance_for_rent_exemption(SplMint::LEN),
            data,
            owner: anchor_spl::token::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();
}

fn store_token_account(svm: &mut LiteSVM, address: Pubkey, mint: Pubkey, owner: Pubkey) {
    let state = SplTokenAccount {
        mint,
        owner,
        amount: 0,
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

fn setup() -> Fixture {
    let mut svm = LiteSVM::new();
    let super_admin = Keypair::new();
    let market = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let quote_mint = Pubkey::new_unique();
    let (protocol_config, protocol_bump) = Pubkey::find_program_address(
        &[tidebook::constants::PROTOCOL_CONFIG_SEED],
        &tidebook::id(),
    );

    svm.add_program(tidebook::id(), PROGRAM_BYTES).unwrap();
    svm.airdrop(&super_admin.pubkey(), 5_000_000_000).unwrap();

    store_anchor_account(
        &mut svm,
        protocol_config,
        &ProtocolConfig {
            super_admin: super_admin.pubkey(),
            taker_fee_bps: 0,
            bump: protocol_bump,
        },
    );
    store_anchor_account(
        &mut svm,
        market,
        &Market {
            authority: super_admin.pubkey(),
            base_mint,
            quote_mint,
            status: MarketStatus::Active,
            next_order_id: 1,
            base_decimals: 9,
            quote_decimals: 6,
            price_tick_size: 10_000,
            quantity_lot_size: 10_000_000,
            best_bid: None,
            best_ask: None,
            open_order_count: 0,
            bump: 255,
        },
    );
    store_mint(&mut svm, base_mint, super_admin.pubkey(), 9, 0);
    store_mint(&mut svm, quote_mint, super_admin.pubkey(), 6, 0);

    Fixture {
        svm,
        super_admin,
        market,
        protocol_config,
        base_mint,
        quote_mint,
    }
}

fn faucet_addresses(market: Pubkey) -> (Pubkey, Pubkey) {
    let test_faucet = Pubkey::find_program_address(
        &[tidebook::constants::TEST_FAUCET_SEED, market.as_ref()],
        &tidebook::id(),
    )
    .0;
    let faucet_authority = Pubkey::find_program_address(
        &[
            tidebook::constants::TEST_FAUCET_AUTHORITY_SEED,
            test_faucet.as_ref(),
        ],
        &tidebook::id(),
    )
    .0;
    (test_faucet, faucet_authority)
}

fn initialize_instruction(
    fixture: &Fixture,
    signer: Pubkey,
    base_claim_amount: u64,
    quote_claim_amount: u64,
) -> Instruction {
    let (test_faucet, faucet_authority) = faucet_addresses(fixture.market);
    Instruction::new_with_bytes(
        tidebook::id(),
        &tidebook::instruction::InitializeTestFaucet {
            base_claim_amount,
            quote_claim_amount,
        }
        .data(),
        tidebook::accounts::InitializeTestFaucet {
            super_admin: signer,
            protocol_config: fixture.protocol_config,
            market: fixture.market,
            test_faucet,
            faucet_authority,
            base_mint: fixture.base_mint,
            quote_mint: fixture.quote_mint,
            token_program: anchor_spl::token::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn claim_instruction(
    fixture: &Fixture,
    claimant: Pubkey,
    claimant_base_account: Pubkey,
    claimant_quote_account: Pubkey,
) -> Instruction {
    let (test_faucet, faucet_authority) = faucet_addresses(fixture.market);
    Instruction::new_with_bytes(
        tidebook::id(),
        &tidebook::instruction::ClaimTestTokens {}.data(),
        tidebook::accounts::ClaimTestTokens {
            claimant,
            market: fixture.market,
            test_faucet,
            faucet_authority,
            base_mint: fixture.base_mint,
            quote_mint: fixture.quote_mint,
            claimant_base_account,
            claimant_quote_account,
            token_program: anchor_spl::token::ID,
        }
        .to_account_metas(None),
    )
}

fn send(
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

fn initialize(fixture: &mut Fixture) -> litesvm::types::TransactionResult {
    let instruction = initialize_instruction(
        fixture,
        fixture.super_admin.pubkey(),
        BASE_CLAIM,
        QUOTE_CLAIM,
    );
    send(&mut fixture.svm, &fixture.super_admin, instruction)
}

fn load_mint(svm: &LiteSVM, address: Pubkey) -> SplMint {
    SplMint::unpack(&svm.get_account(&address).unwrap().data).unwrap()
}

fn load_token_amount(svm: &LiteSVM, address: Pubkey) -> u64 {
    SplTokenAccount::unpack(&svm.get_account(&address).unwrap().data)
        .unwrap()
        .amount
}

#[test]
fn super_admin_initializes_faucet_and_transfers_both_authorities() {
    let mut fixture = setup();
    let result = initialize(&mut fixture);
    assert!(result.is_ok());

    let (test_faucet, faucet_authority) = faucet_addresses(fixture.market);
    let faucet_account = fixture.svm.get_account(&test_faucet).unwrap();
    let mut data = faucet_account.data.as_slice();
    let state = TestFaucet::try_deserialize(&mut data).unwrap();

    assert_eq!(state.market, fixture.market);
    assert_eq!(state.base_claim_amount, BASE_CLAIM);
    assert_eq!(state.quote_claim_amount, QUOTE_CLAIM);
    assert_eq!(
        load_mint(&fixture.svm, fixture.base_mint).mint_authority,
        COption::Some(faucet_authority)
    );
    assert_eq!(
        load_mint(&fixture.svm, fixture.quote_mint).mint_authority,
        COption::Some(faucet_authority)
    );

    let event =
        support::events::single_event::<tidebook::events::TestFaucetInitializedEvent>(&result);
    assert_eq!(event.market, fixture.market);
    assert_eq!(event.test_faucet, test_faucet);
    assert_eq!(event.faucet_authority, faucet_authority);
    assert_eq!(event.base_mint, fixture.base_mint);
    assert_eq!(event.quote_mint, fixture.quote_mint);
    assert_eq!(event.base_claim_amount, BASE_CLAIM);
    assert_eq!(event.quote_claim_amount, QUOTE_CLAIM);
}

#[test]
fn any_wallet_can_claim_fixed_base_and_quote_amounts() {
    let mut fixture = setup();
    assert!(initialize(&mut fixture).is_ok());

    let claimant = Keypair::new();
    fixture
        .svm
        .airdrop(&claimant.pubkey(), 1_000_000_000)
        .unwrap();
    let base_account = Pubkey::new_unique();
    let quote_account = Pubkey::new_unique();
    store_token_account(
        &mut fixture.svm,
        base_account,
        fixture.base_mint,
        claimant.pubkey(),
    );
    store_token_account(
        &mut fixture.svm,
        quote_account,
        fixture.quote_mint,
        claimant.pubkey(),
    );

    let instruction = claim_instruction(&fixture, claimant.pubkey(), base_account, quote_account);
    let result = send(&mut fixture.svm, &claimant, instruction);
    assert!(result.is_ok());
    assert_eq!(load_token_amount(&fixture.svm, base_account), BASE_CLAIM);
    assert_eq!(load_token_amount(&fixture.svm, quote_account), QUOTE_CLAIM);

    let event = support::events::single_event::<tidebook::events::TestFaucetClaimedEvent>(&result);
    assert_eq!(event.market, fixture.market);
    assert_eq!(event.test_faucet, faucet_addresses(fixture.market).0);
    assert_eq!(event.claimant, claimant.pubkey());
    assert_eq!(event.base_amount, BASE_CLAIM);
    assert_eq!(event.quote_amount, QUOTE_CLAIM);
}

#[test]
fn rejects_non_super_admin_initialization() {
    let mut fixture = setup();
    let attacker = Keypair::new();
    fixture
        .svm
        .airdrop(&attacker.pubkey(), 1_000_000_000)
        .unwrap();
    let instruction = initialize_instruction(&fixture, attacker.pubkey(), BASE_CLAIM, QUOTE_CLAIM);
    assert!(send(&mut fixture.svm, &attacker, instruction).is_err());
}

#[test]
fn rejects_zero_claim_amount() {
    let mut fixture = setup();
    let instruction =
        initialize_instruction(&fixture, fixture.super_admin.pubkey(), 0, QUOTE_CLAIM);
    assert!(send(&mut fixture.svm, &fixture.super_admin, instruction).is_err());
}

#[test]
fn rejects_mints_with_existing_supply() {
    let mut fixture = setup();
    store_mint(
        &mut fixture.svm,
        fixture.base_mint,
        fixture.super_admin.pubkey(),
        9,
        1,
    );
    assert!(initialize(&mut fixture).is_err());
}

#[test]
fn rejects_destination_owned_by_another_wallet() {
    let mut fixture = setup();
    assert!(initialize(&mut fixture).is_ok());

    let claimant = Keypair::new();
    let other_owner = Pubkey::new_unique();
    fixture
        .svm
        .airdrop(&claimant.pubkey(), 1_000_000_000)
        .unwrap();
    let base_account = Pubkey::new_unique();
    let quote_account = Pubkey::new_unique();
    store_token_account(
        &mut fixture.svm,
        base_account,
        fixture.base_mint,
        other_owner,
    );
    store_token_account(
        &mut fixture.svm,
        quote_account,
        fixture.quote_mint,
        claimant.pubkey(),
    );

    let instruction = claim_instruction(&fixture, claimant.pubkey(), base_account, quote_account);
    assert!(send(&mut fixture.svm, &claimant, instruction).is_err());
}
