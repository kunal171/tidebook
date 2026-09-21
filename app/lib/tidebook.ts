import {
  AnchorProvider,
  BN,
  Program,
  type Idl,
} from "@anchor-lang/core";
import {
  Connection,
  PublicKey,
  type GetProgramAccountsFilter,
} from "@solana/web3.js";
import tidebookIdl from "../idl/tidebook.json";
import { PROGRAM_ID } from "./solana";

export const PROTOCOL_CONFIG_SEED = "protocol_config";
export const ADMIN_SEED = "admin";
export const MARKET_SEED = "market";
export const ORDER_SEED = "order";
export const VAULT_AUTHORITY_SEED = "vault-authority";
export const VAULT_SEED = "vault";

export const TOKEN_PROGRAM_ID = new PublicKey(
  "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
);

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
export type MarketStatus = "active" | "paused";

export type OrderSide = "bid" | "ask";
export type OrderStatus = "open" | "filled" | "canceled";

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

export interface MarketAccount {
  authority: PublicKey;
  baseMint: PublicKey;
  quoteMint: PublicKey;
  status: { active?: object; paused?: object };
  nextOrderId: BN;

  baseDecimals: number;
  quoteDecimals: number;
  priceTickSize: BN;
  quantityLotSize: BN;

  bestBid: BN | null;
  bestAsk: BN | null;
  bump: number;
}

export interface MarketView extends MarketAccount {
  address: PublicKey;
}

export interface OrderAccount {
  owner: PublicKey;
  market: PublicKey;
  orderId: BN;
  side: { bid?: object; ask?: object };
  price: BN;
  quantity: BN;
  remainingQuantity: BN;
  status: {
    open?: object;
    filled?: object;
    canceled?: object;
  };
  bump: number;
}

export interface OrderView extends OrderAccount {
  address: PublicKey;
}

interface AccountClient<T> {
  fetchNullable(address: PublicKey): Promise<T | null>;

  all(
    filters?: GetProgramAccountsFilter[],
  ): Promise<Array<{ publicKey: PublicKey; account: T }>>;
}

interface TidebookAccounts {
  protocolConfig: AccountClient<ProtocolConfigAccount>;
  adminRecord: AccountClient<AdminRecordAccount>;
  market: AccountClient<MarketAccount>;
  order: AccountClient<OrderAccount>;
}

export function deriveVaultAuthorityPda(market: PublicKey) {
  return PublicKey.findProgramAddressSync(
    [Buffer.from(VAULT_AUTHORITY_SEED), market.toBuffer()],
    PROGRAM_ID,
  )[0];
}

export function deriveVaultPda(market: PublicKey, mint: PublicKey) {
  return PublicKey.findProgramAddressSync(
    [
      Buffer.from(VAULT_SEED),
      market.toBuffer(),
      mint.toBuffer(),
    ],
    PROGRAM_ID,
  )[0];
}

export function getTidebookProgram(connection: Connection, wallet: BrowserWallet) {
  const provider = new AnchorProvider(connection, wallet, {
    commitment: "confirmed",
    preflightCommitment: "confirmed",
  });

  return new Program(tidebookIdl as Idl, provider);
}

export function getTidebookReadProgram(connection: Connection) {
  return new Program(tidebookIdl as Idl, { connection });
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

export function deriveOrderPda(market: PublicKey, orderId: BN) {
  return PublicKey.findProgramAddressSync(
    [
      Buffer.from(ORDER_SEED),
      market.toBuffer(),
      orderId.toArrayLike(Buffer, "le", 8),
    ],
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

export function decodeMarketStatus(status: MarketAccount["status"]): MarketStatus {
  return "paused" in status ? "paused" : "active";
}

export function decodeOrderSide(
  side: OrderAccount["side"],
): OrderSide {
  return "bid" in side ? "bid" : "ask";
}

export function decodeOrderStatus(
  status: OrderAccount["status"],
): OrderStatus {
  if ("open" in status) return "open";
  if ("filled" in status) return "filled";
  return "canceled";
}

export function formatAtomicAmount(value: BN, decimals: number) {
  const raw = value.toString(10);

  if (decimals === 0) return raw;

  const padded = raw.padStart(decimals + 1, "0");
  const whole = padded.slice(0, -decimals);
  const fraction = padded.slice(-decimals).replace(/0+$/, "");

  return fraction ? `${whole}.${fraction}` : whole;
}

export function transactionExplorerUrl(signature: string) {
  return `https://explorer.solana.com/tx/${signature}?cluster=devnet`;
}

export function accountExplorerUrl(address: PublicKey) {
  return `https://explorer.solana.com/address/${address.toBase58()}?cluster=devnet`;
}
