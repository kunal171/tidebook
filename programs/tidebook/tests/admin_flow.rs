//! LiteSVM integration coverage for protocol initialization and administrator
//! lifecycle authorization.

// LiteSVM intentionally returns rich transaction-failure metadata. Boxing it in
// every test helper would add indirection without reducing production account or
// instruction size, so this integration-test boundary permits the large error.
#![allow(clippy::result_large_err)]

mod support;

use {
    anchor_lang::{
        prelude::Pubkey,
        solana_program::{bpf_loader_upgradeable, instruction::Instruction, system_program},
        AccountDeserialize, AccountSerialize, InstructionData, ToAccountMetas,
    },
    litesvm::LiteSVM,
    solana_account::Account,
    solana_keypair::Keypair,
    solana_loader_v3_interface::state::UpgradeableLoaderState,
    solana_message::{Message, VersionedMessage},
    solana_signer::Signer,
    solana_transaction::versioned::VersionedTransaction,
    tidebook::state::{AdminRecord, AdminStatus, ProtocolConfig},
    wincode::{Deserialize as WincodeDeserialize, Serialize as WincodeSerialize},
};

const PROGRAM_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../target/deploy/tidebook.so"
));

fn protocol_config_address() -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[tidebook::constants::PROTOCOL_CONFIG_SEED],
        &tidebook::id(),
    )
}

fn admin_record_address(authority: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[tidebook::constants::ADMIN_SEED, authority.as_ref()],
        &tidebook::id(),
    )
}

fn program_data_address() -> Pubkey {
    Pubkey::find_program_address(&[tidebook::id().as_ref()], &bpf_loader_upgradeable::ID).0
}

fn set_program_upgrade_authority(svm: &mut LiteSVM, authority: Pubkey) -> Pubkey {
    let address = program_data_address();
    let mut account = svm.get_account(&address).unwrap();
    let metadata_len = UpgradeableLoaderState::size_of_programdata_metadata();
    let metadata = UpgradeableLoaderState::deserialize(&account.data[..metadata_len]).unwrap();
    let slot = match metadata {
        UpgradeableLoaderState::ProgramData { slot, .. } => slot,
        state => panic!("expected ProgramData account, got {state:?}"),
    };

    let mut data = UpgradeableLoaderState::serialize(&UpgradeableLoaderState::ProgramData {
        slot,
        upgrade_authority_address: Some(authority.to_bytes().into()),
    })
    .unwrap();
    data.extend_from_slice(&account.data[metadata_len..]);
    account.data = data;
    svm.set_account(address, account).unwrap();

    address
}

fn store_protocol_config(svm: &mut LiteSVM, super_admin: Pubkey) -> Pubkey {
    let (address, bump) = protocol_config_address();
    let state = ProtocolConfig { super_admin, bump };
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

fn store_admin_record(
    svm: &mut LiteSVM,
    authority: Pubkey,
    added_by: Pubkey,
    status: AdminStatus,
) -> Pubkey {
    let (address, bump) = admin_record_address(&authority);
    let state = AdminRecord {
        authority,
        added_by,
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

    address
}

fn setup() -> (LiteSVM, Keypair, Pubkey) {
    let mut svm = LiteSVM::new();
    let super_admin = Keypair::new();

    svm.add_program(tidebook::id(), PROGRAM_BYTES).unwrap();
    svm.airdrop(&super_admin.pubkey(), 2_000_000_000).unwrap();

    let protocol_config = store_protocol_config(&mut svm, super_admin.pubkey());
    store_admin_record(
        &mut svm,
        super_admin.pubkey(),
        super_admin.pubkey(),
        AdminStatus::Active,
    );

    (svm, super_admin, protocol_config)
}

fn setup_uninitialized_protocol() -> (LiteSVM, Keypair, Pubkey) {
    let mut svm = LiteSVM::new();
    let deployer = Keypair::new();

    svm.add_program(tidebook::id(), PROGRAM_BYTES).unwrap();
    svm.airdrop(&deployer.pubkey(), 2_000_000_000).unwrap();
    let program_data = set_program_upgrade_authority(&mut svm, deployer.pubkey());

    (svm, deployer, program_data)
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

fn add_admin_instruction(
    super_admin: Pubkey,
    protocol_config: Pubkey,
    new_admin: Pubkey,
) -> Instruction {
    let (admin_record, _) = admin_record_address(&new_admin);

    Instruction::new_with_bytes(
        tidebook::id(),
        &tidebook::instruction::AddAdmin { new_admin }.data(),
        tidebook::accounts::AddAdmin {
            super_admin,
            protocol_config,
            admin_record,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn initialize_protocol_instruction(deployer: Pubkey, program_data: Pubkey) -> Instruction {
    let (protocol_config, _) = protocol_config_address();
    let (deployer_admin, _) = admin_record_address(&deployer);

    Instruction::new_with_bytes(
        tidebook::id(),
        &tidebook::instruction::InitializeProtocol {}.data(),
        tidebook::accounts::InitializeProtocol {
            deployer,
            protocol_config,
            deployer_admin,
            program: tidebook::id(),
            program_data,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn disable_admin_instruction(
    super_admin: Pubkey,
    protocol_config: Pubkey,
    target_admin: Pubkey,
) -> Instruction {
    let (admin_record, _) = admin_record_address(&target_admin);

    Instruction::new_with_bytes(
        tidebook::id(),
        &tidebook::instruction::DisableAdmin { target_admin }.data(),
        tidebook::accounts::ManageAdmin {
            super_admin,
            protocol_config,
            admin_record,
        }
        .to_account_metas(None),
    )
}

fn enable_admin_instruction(
    super_admin: Pubkey,
    protocol_config: Pubkey,
    target_admin: Pubkey,
) -> Instruction {
    let (admin_record, _) = admin_record_address(&target_admin);

    Instruction::new_with_bytes(
        tidebook::id(),
        &tidebook::instruction::EnableAdmin { target_admin }.data(),
        tidebook::accounts::ManageAdmin {
            super_admin,
            protocol_config,
            admin_record,
        }
        .to_account_metas(None),
    )
}

fn remove_admin_instruction(
    super_admin: Pubkey,
    protocol_config: Pubkey,
    target_admin: Pubkey,
) -> Instruction {
    let (admin_record, _) = admin_record_address(&target_admin);

    Instruction::new_with_bytes(
        tidebook::id(),
        &tidebook::instruction::RemoveAdmin { target_admin }.data(),
        tidebook::accounts::RemoveAdmin {
            super_admin,
            protocol_config,
            admin_record,
        }
        .to_account_metas(None),
    )
}

fn load_admin_record(svm: &LiteSVM, authority: &Pubkey) -> AdminRecord {
    let (address, _) = admin_record_address(authority);
    let account = svm.get_account(&address).unwrap();
    let mut data: &[u8] = &account.data;
    AdminRecord::try_deserialize(&mut data).unwrap()
}

fn add_admin(svm: &mut LiteSVM, super_admin: &Keypair, protocol_config: Pubkey, new_admin: Pubkey) {
    let instruction = add_admin_instruction(super_admin.pubkey(), protocol_config, new_admin);
    let result = send_instruction(svm, super_admin, instruction);
    assert!(result.is_ok(), "adding admin failed: {result:?}");
}

#[test]
fn upgrade_authority_initializes_protocol() {
    let (mut svm, deployer, program_data) = setup_uninitialized_protocol();
    let instruction = initialize_protocol_instruction(deployer.pubkey(), program_data);

    let result = send_instruction(&mut svm, &deployer, instruction);

    assert!(result.is_ok(), "protocol initialization failed: {result:?}");

    let (protocol_config, _) = protocol_config_address();
    let (admin_record, _) = admin_record_address(&deployer.pubkey());
    let event =
        support::events::single_event::<tidebook::events::ProtocolInitializedEvent>(&result);
    assert_eq!(event.protocol_config, protocol_config);
    assert_eq!(event.super_admin, deployer.pubkey());
    assert_eq!(event.admin_record, admin_record);
}

#[test]
fn protocol_initialization_creates_config_and_deployer_admin() {
    let (mut svm, deployer, program_data) = setup_uninitialized_protocol();
    let instruction = initialize_protocol_instruction(deployer.pubkey(), program_data);
    assert!(send_instruction(&mut svm, &deployer, instruction).is_ok());

    let (protocol_config, config_bump) = protocol_config_address();
    let config_account = svm.get_account(&protocol_config).unwrap();
    let mut config_data: &[u8] = &config_account.data;
    let config = ProtocolConfig::try_deserialize(&mut config_data).unwrap();
    assert_eq!(config.super_admin, deployer.pubkey());
    assert_eq!(config.bump, config_bump);

    let (_, admin_bump) = admin_record_address(&deployer.pubkey());
    let admin = load_admin_record(&svm, &deployer.pubkey());
    assert_eq!(admin.authority, deployer.pubkey());
    assert_eq!(admin.added_by, deployer.pubkey());
    assert_eq!(admin.status, AdminStatus::Active);
    assert_eq!(admin.bump, admin_bump);
}

#[test]
fn non_upgrade_authority_cannot_initialize_protocol() {
    let (mut svm, _deployer, program_data) = setup_uninitialized_protocol();
    let unauthorized = Keypair::new();
    svm.airdrop(&unauthorized.pubkey(), 2_000_000_000).unwrap();
    let instruction = initialize_protocol_instruction(unauthorized.pubkey(), program_data);

    let result = send_instruction(&mut svm, &unauthorized, instruction);

    assert!(
        result.is_err(),
        "non-upgrade authority initialized the protocol"
    );
    let (protocol_config, _) = protocol_config_address();
    let (unauthorized_admin, _) = admin_record_address(&unauthorized.pubkey());
    assert!(svm.get_account(&protocol_config).is_none());
    assert!(svm.get_account(&unauthorized_admin).is_none());
}

#[test]
fn protocol_cannot_be_initialized_twice() {
    let (mut svm, deployer, program_data) = setup_uninitialized_protocol();
    let first = initialize_protocol_instruction(deployer.pubkey(), program_data);
    assert!(send_instruction(&mut svm, &deployer, first).is_ok());

    let second = initialize_protocol_instruction(deployer.pubkey(), program_data);
    let result = send_instruction(&mut svm, &deployer, second);

    assert!(result.is_err(), "protocol was initialized twice");
}

#[test]
fn super_admin_adds_active_admin() {
    let (mut svm, super_admin, protocol_config) = setup();
    let new_admin = Pubkey::new_unique();

    let instruction = add_admin_instruction(super_admin.pubkey(), protocol_config, new_admin);
    let result = send_instruction(&mut svm, &super_admin, instruction);
    assert!(result.is_ok(), "adding admin failed: {result:?}");

    let record = load_admin_record(&svm, &new_admin);
    assert_eq!(record.authority, new_admin);
    assert_eq!(record.added_by, super_admin.pubkey());
    assert_eq!(record.status, AdminStatus::Active);

    let (admin_record, _) = admin_record_address(&new_admin);
    let event = support::events::single_event::<tidebook::events::AdminAddedEvent>(&result);
    assert_eq!(event.admin_record, admin_record);
    assert_eq!(event.authority, new_admin);
    assert_eq!(event.added_by, super_admin.pubkey());
}

#[test]
fn non_super_admin_cannot_add_admin() {
    let (mut svm, _super_admin, protocol_config) = setup();
    let unauthorized = Keypair::new();
    let new_admin = Pubkey::new_unique();
    svm.airdrop(&unauthorized.pubkey(), 1_000_000_000).unwrap();

    let instruction = add_admin_instruction(unauthorized.pubkey(), protocol_config, new_admin);
    let result = send_instruction(&mut svm, &unauthorized, instruction);

    assert!(result.is_err(), "unauthorized signer added an admin");
    let (admin_record, _) = admin_record_address(&new_admin);
    assert!(svm.get_account(&admin_record).is_none());
}

#[test]
fn super_admin_disables_and_enables_admin() {
    let (mut svm, super_admin, protocol_config) = setup();
    let target_admin = Pubkey::new_unique();
    add_admin(&mut svm, &super_admin, protocol_config, target_admin);

    let disable = disable_admin_instruction(super_admin.pubkey(), protocol_config, target_admin);
    let disable_result = send_instruction(&mut svm, &super_admin, disable);
    assert!(disable_result.is_ok());
    assert_eq!(
        load_admin_record(&svm, &target_admin).status,
        AdminStatus::Disabled
    );
    let disabled =
        support::events::single_event::<tidebook::events::AdminStatusChangedEvent>(&disable_result);
    assert_eq!(disabled.authority, target_admin);
    assert_eq!(disabled.changed_by, super_admin.pubkey());
    assert_eq!(disabled.status, AdminStatus::Disabled);

    let enable = enable_admin_instruction(super_admin.pubkey(), protocol_config, target_admin);
    let enable_result = send_instruction(&mut svm, &super_admin, enable);
    assert!(enable_result.is_ok());
    assert_eq!(
        load_admin_record(&svm, &target_admin).status,
        AdminStatus::Active
    );
    let enabled =
        support::events::single_event::<tidebook::events::AdminStatusChangedEvent>(&enable_result);
    assert_eq!(enabled.authority, target_admin);
    assert_eq!(enabled.changed_by, super_admin.pubkey());
    assert_eq!(enabled.status, AdminStatus::Active);
}

#[test]
fn non_super_admin_cannot_disable_admin() {
    let (mut svm, super_admin, protocol_config) = setup();
    let target_admin = Pubkey::new_unique();
    let unauthorized = Keypair::new();
    svm.airdrop(&unauthorized.pubkey(), 1_000_000_000).unwrap();
    add_admin(&mut svm, &super_admin, protocol_config, target_admin);

    let instruction =
        disable_admin_instruction(unauthorized.pubkey(), protocol_config, target_admin);
    let result = send_instruction(&mut svm, &unauthorized, instruction);

    assert!(result.is_err(), "unauthorized signer disabled an admin");
    assert_eq!(
        load_admin_record(&svm, &target_admin).status,
        AdminStatus::Active
    );
}

#[test]
fn super_admin_cannot_disable_itself() {
    let (mut svm, super_admin, protocol_config) = setup();
    let instruction =
        disable_admin_instruction(super_admin.pubkey(), protocol_config, super_admin.pubkey());

    let result = send_instruction(&mut svm, &super_admin, instruction);

    assert!(result.is_err(), "super admin disabled itself");
    assert_eq!(
        load_admin_record(&svm, &super_admin.pubkey()).status,
        AdminStatus::Active
    );
}

#[test]
fn active_admin_cannot_be_removed() {
    let (mut svm, super_admin, protocol_config) = setup();
    let target_admin = Pubkey::new_unique();
    add_admin(&mut svm, &super_admin, protocol_config, target_admin);

    let instruction = remove_admin_instruction(super_admin.pubkey(), protocol_config, target_admin);
    let result = send_instruction(&mut svm, &super_admin, instruction);

    assert!(result.is_err(), "active admin was removed");
    assert_eq!(
        load_admin_record(&svm, &target_admin).status,
        AdminStatus::Active
    );
}

#[test]
fn disabled_admin_can_be_removed_and_added_again() {
    let (mut svm, super_admin, protocol_config) = setup();
    let target_admin = Pubkey::new_unique();
    let (admin_record, _) = admin_record_address(&target_admin);
    add_admin(&mut svm, &super_admin, protocol_config, target_admin);

    let disable = disable_admin_instruction(super_admin.pubkey(), protocol_config, target_admin);
    assert!(send_instruction(&mut svm, &super_admin, disable).is_ok());

    let remove = remove_admin_instruction(super_admin.pubkey(), protocol_config, target_admin);
    let remove_result = send_instruction(&mut svm, &super_admin, remove);
    assert!(remove_result.is_ok());
    assert!(svm.get_account(&admin_record).is_none());

    let event =
        support::events::single_event::<tidebook::events::AdminRemovedEvent>(&remove_result);
    assert_eq!(event.admin_record, admin_record);
    assert_eq!(event.authority, target_admin);
    assert_eq!(event.removed_by, super_admin.pubkey());

    add_admin(&mut svm, &super_admin, protocol_config, target_admin);
    assert_eq!(
        load_admin_record(&svm, &target_admin).status,
        AdminStatus::Active
    );
}
