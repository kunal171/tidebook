use {
    anchor_lang::{
        prelude::Pubkey,
        solana_program::{instruction::Instruction, program_pack::Pack, system_program},
        AccountDeserialize, AccountSerialize, InstructionData, ToAccountMetas,
    },
    anchor_spl::token::spl_token::state::Mint as SplMint,
    litesvm::LiteSVM,
    solana_account::Account,
    solana_keypair::Keypair,
    solana_message::{Message, VersionedMessage},
    solana_signer::Signer,
    solana_transaction::versioned::VersionedTransaction,
    tidebook::state::{
        AdminRecord, AdminStatus, Market, MarketStatus, Order, OrderSide, OrderStatus,
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

fn initialize_market(svm: &mut LiteSVM, authority: &Keypair) -> Pubkey {
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
    market
}

fn place_order(svm: &mut LiteSVM, owner: &Keypair, market: Pubkey, order_id: u64) -> Pubkey {
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
        &tidebook::instruction::PlaceLimitOrder {
            side: OrderSide::Bid,
            price: TEST_ORDER_PRICE,
            quantity: TEST_ORDER_QUANTITY,
        }
        .data(),
        tidebook::accounts::PlaceLimitOrder {
            trader: owner.pubkey(),
            market,
            order,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    );

    let result = send_instruction(svm, owner, instruction);
    assert!(result.is_ok(), "order placement failed: {result:?}");
    order
}

fn cancel_order_instruction(
    signer: Pubkey,
    market: Pubkey,
    order: Pubkey,
    order_id: u64,
) -> Instruction {
    Instruction::new_with_bytes(
        tidebook::id(),
        &tidebook::instruction::CancelLimitOrder { order_id }.data(),
        tidebook::accounts::CancelLimitOrder {
            owner: signer,
            market,
            order,
        }
        .to_account_metas(None),
    )
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

fn setup_open_order() -> (LiteSVM, Keypair, Pubkey, Pubkey) {
    let mut svm = LiteSVM::new();
    let owner = Keypair::new();

    svm.add_program(tidebook::id(), PROGRAM_BYTES).unwrap();
    svm.airdrop(&owner.pubkey(), 2_000_000_000).unwrap();
    store_active_admin(&mut svm, owner.pubkey());

    let market = initialize_market(&mut svm, &owner);
    let order = place_order(&mut svm, &owner, market, ORDER_ID);

    (svm, owner, market, order)
}

#[test]
fn owner_cancels_open_order() {
    let (mut svm, owner, market, order) = setup_open_order();
    let instruction = cancel_order_instruction(owner.pubkey(), market, order, ORDER_ID);

    let result = send_instruction(&mut svm, &owner, instruction);

    assert!(result.is_ok(), "order cancellation failed: {result:?}");
    let state = load_order(&svm, order);
    assert_eq!(state.status, OrderStatus::Canceled);
    assert_eq!(state.remaining_quantity, TEST_ORDER_QUANTITY);
}

#[test]
fn non_owner_cannot_cancel_order() {
    let (mut svm, _owner, market, order) = setup_open_order();
    let attacker = Keypair::new();
    svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();
    let instruction = cancel_order_instruction(attacker.pubkey(), market, order, ORDER_ID);

    let result = send_instruction(&mut svm, &attacker, instruction);

    assert!(result.is_err(), "non-owner canceled the order");
    assert_eq!(load_order(&svm, order).status, OrderStatus::Open);
}

#[test]
fn canceled_order_cannot_be_canceled_twice() {
    let (mut svm, owner, market, order) = setup_open_order();

    let first = send_instruction(
        &mut svm,
        &owner,
        cancel_order_instruction(owner.pubkey(), market, order, ORDER_ID),
    );
    assert!(first.is_ok(), "first cancellation failed: {first:?}");

    let second = send_instruction(
        &mut svm,
        &owner,
        cancel_order_instruction(owner.pubkey(), market, order, ORDER_ID),
    );

    assert!(second.is_err(), "order was canceled twice");
    assert_eq!(load_order(&svm, order).status, OrderStatus::Canceled);
}

#[test]
fn order_cannot_be_canceled_with_different_market() {
    let (mut svm, owner, original_market, order) = setup_open_order();
    let different_market = initialize_market(&mut svm, &owner);
    assert_ne!(original_market, different_market);
    let instruction = cancel_order_instruction(owner.pubkey(), different_market, order, ORDER_ID);

    let result = send_instruction(&mut svm, &owner, instruction);

    assert!(result.is_err(), "order accepted a different market");
    assert_eq!(load_order(&svm, order).status, OrderStatus::Open);
}

#[test]
fn owner_can_cancel_order_while_market_is_paused() {
    let (mut svm, owner, market, order) = setup_open_order();
    let pause = Instruction::new_with_bytes(
        tidebook::id(),
        &tidebook::instruction::PauseMarket {}.data(),
        tidebook::accounts::PauseMarket {
            authority: owner.pubkey(),
            market,
        }
        .to_account_metas(None),
    );
    let pause_result = send_instruction(&mut svm, &owner, pause);
    assert!(
        pause_result.is_ok(),
        "market pause failed: {pause_result:?}"
    );
    assert_eq!(load_market(&svm, market).status, MarketStatus::Paused);

    let cancel = cancel_order_instruction(owner.pubkey(), market, order, ORDER_ID);
    let cancel_result = send_instruction(&mut svm, &owner, cancel);

    assert!(
        cancel_result.is_ok(),
        "paused-market cancellation failed: {cancel_result:?}"
    );
    assert_eq!(load_order(&svm, order).status, OrderStatus::Canceled);
}
