import {
  AnchorProvider,
  Program,
  type Idl,
} from "@anchor-lang/core";
import {
  Connection,
  PublicKey,
} from "@solana/web3.js";
import tidebookIdl from "../idl/tidebook.json";
import { PROGRAM_ID } from "./solana";

export const PROTOCOL_CONFIG_SEED = "protocol_config";
export const ADMIN_SEED = "admin";
export const MARKET_SEED = "market";

const BPF_LOADER_UPGRADEABLE_PROGRAM_ID = new PublicKey(
  "BPFLoaderUpgradeab1e11111111111111111111111",
);

type BrowserWallet = ConstructorParameters<typeof AnchorProvider>[1];

export type ProtocolRole =
  | "disconnected"
  | "uninitialized"
  | "super-admin"
  | "admin"
  | "disabled"
  | "user";

export type AdminStatus = "active" | "disabled";

export interface ProtocolConfigAccount {
  superAdmin: PublicKey;
  bump: number;
}

export interface AdminRecordAccount {
  authority: PublicKey;
  addedBy: PublicKey;
  status: { active?: object; disabled?: object };
  bump: number;
}

export interface AdminRecordView {
  address: PublicKey;
  authority: PublicKey;
  addedBy: PublicKey;
  status: AdminStatus;
}

interface AccountClient<T> {
  fetchNullable(address: PublicKey): Promise<T | null>;
  all(): Promise<Array<{ publicKey: PublicKey; account: T }>>;
}

interface TidebookAccounts {
  protocolConfig: AccountClient<ProtocolConfigAccount>;
  adminRecord: AccountClient<AdminRecordAccount>;
}

export function getTidebookProgram(connection: Connection, wallet: BrowserWallet) {
  const provider = new AnchorProvider(connection, wallet, {
    commitment: "confirmed",
    preflightCommitment: "confirmed",
  });

  return new Program(tidebookIdl as Idl, provider);
}

export function getTidebookAccounts(program: Program): TidebookAccounts {
  return program.account as unknown as TidebookAccounts;
}

export function deriveProtocolConfigPda() {
  return PublicKey.findProgramAddressSync(
    [Buffer.from(PROTOCOL_CONFIG_SEED)],
    PROGRAM_ID,
  )[0];
}

export function deriveAdminRecordPda(authority: PublicKey) {
  return PublicKey.findProgramAddressSync(
    [Buffer.from(ADMIN_SEED), authority.toBuffer()],
    PROGRAM_ID,
  )[0];
}

export function deriveMarketPda(baseMint: PublicKey, quoteMint: PublicKey) {
  return PublicKey.findProgramAddressSync(
    [Buffer.from(MARKET_SEED), baseMint.toBuffer(), quoteMint.toBuffer()],
    PROGRAM_ID,
  )[0];
}

export function deriveProgramDataAddress() {
  return PublicKey.findProgramAddressSync(
    [PROGRAM_ID.toBuffer()],
    BPF_LOADER_UPGRADEABLE_PROGRAM_ID,
  )[0];
}

export function decodeAdminStatus(status: AdminRecordAccount["status"]): AdminStatus {
  return "disabled" in status ? "disabled" : "active";
}

export function transactionExplorerUrl(signature: string) {
  return `https://explorer.solana.com/tx/${signature}?cluster=devnet`;
}
