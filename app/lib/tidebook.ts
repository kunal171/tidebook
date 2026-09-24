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
export const PRICE_LEVEL_SEED = "price_level";
export const TRADER_BALANCE_SEED = "trader_balance";
export const VAULT_AUTHORITY_SEED = "vault-authority";
export const VAULT_SEED = "vault";

// Markets currently use the original SPL Token program, not Token-2022.
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
  openOrderCount: BN;
  bump: number;
}

export interface MarketView extends MarketAccount {
  address: PublicKey;
}

export interface PriceLevelAccount {
  market: PublicKey;
  side: { bid?: object; ask?: object };
  price: BN;
  betterPrice: BN | null;
  worsePrice: BN | null;
  firstOrder: PublicKey | null;
  lastOrder: PublicKey | null;
  totalRemainingQuantity: BN;
  orderCount: BN;
  rentPayer: PublicKey;
  bump: number;
}

export interface PriceLevelNeighbors {
  betterLevel: PublicKey | null;
  worseLevel: PublicKey | null;
}

export interface OrderAccount {
  owner: PublicKey;
  market: PublicKey;
  orderId: BN;
  side: { bid?: object; ask?: object };
  price: BN;
  priceLevel: PublicKey;
  previousOrder: PublicKey | null;
  nextOrder: PublicKey | null;
  quantity: BN;
  remainingQuantity: BN;
  lockedCollateral: BN;
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

export interface TraderBalanceAccount {
  market: PublicKey;
  owner: PublicKey;
  baseFree: BN;
  baseLocked: BN;
  quoteFree: BN;
  quoteLocked: BN;
  bump: number;
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
  priceLevel: AccountClient<PriceLevelAccount>;
  order: AccountClient<OrderAccount>;
  traderBalance: AccountClient<TraderBalanceAccount>;
}

export function deriveVaultAuthorityPda(market: PublicKey) {
  // This PDA has no account data; it exists only as the signing authority for
  // both token vaults belonging to the market.
  return PublicKey.findProgramAddressSync(
    [Buffer.from(VAULT_AUTHORITY_SEED), market.toBuffer()],
    PROGRAM_ID,
  )[0];
}

export function deriveVaultPda(market: PublicKey, mint: PublicKey) {
  // Including the mint gives each market exactly one canonical vault per asset.
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

export function derivePriceLevelPda(
  market: PublicKey,
  side: OrderSide,
  price: BN,
) {
  return PublicKey.findProgramAddressSync(
    [
      Buffer.from(PRICE_LEVEL_SEED),
      market.toBuffer(),
      Buffer.from(side),
      price.toArrayLike(Buffer, "le", 8),
    ],
    PROGRAM_ID,
  )[0];
}

export function deriveTraderBalancePda(market: PublicKey, owner: PublicKey) {
  return PublicKey.findProgramAddressSync(
    [
      Buffer.from(TRADER_BALANCE_SEED),
      market.toBuffer(),
      owner.toBuffer(),
    ],
    PROGRAM_ID,
  )[0];
}

/**
 * Walks the program-maintained price-level list to locate the insertion gap.
 *
 * The RPC traversal is intentionally treated as advisory: it detects malformed
 * or cyclic client state early, while the instruction repeats all security
 * checks atomically in case the book changes before the transaction lands.
 * Traversal is O(number of price levels); a production client can replace it
 * with an indexer without changing the on-chain neighbor contract.
 */
export async function findPriceLevelNeighbors(
  program: Program,
  market: PublicKey,
  side: OrderSide,
  price: BN,
  bestPrice: BN | null,
): Promise<PriceLevelNeighbors> {
  if (!bestPrice) {
    return { betterLevel: null, worseLevel: null };
  }

  const accounts = getTidebookAccounts(program);
  const visited = new Set<string>();
  let expectedBetterPrice: BN | null = null;
  let betterLevel: PublicKey | null = null;
  let currentPrice = bestPrice;

  while (true) {
    const currentLevel = derivePriceLevelPda(market, side, currentPrice);
    const currentAddress = currentLevel.toBase58();

    if (visited.has(currentAddress)) {
      throw new Error("Price-level index contains a cycle");
    }
    visited.add(currentAddress);

    const level = await accounts.priceLevel.fetchNullable(currentLevel);
    if (!level) {
      throw new Error(
        "Price-level index points to missing account " + currentAddress,
      );
    }

    const decodedSide: OrderSide = "bid" in level.side ? "bid" : "ask";
    if (!level.market.equals(market) || decodedSide !== side) {
      throw new Error("Price-level index crosses a market or side boundary");
    }
    if (!level.price.eq(currentPrice)) {
      throw new Error("Price-level PDA and stored price do not match");
    }

    const betterLinkMatches = expectedBetterPrice
      ? level.betterPrice?.eq(expectedBetterPrice) === true
      : level.betterPrice === null;
    if (!betterLinkMatches) {
      throw new Error("Price-level index has a broken reciprocal link");
    }

    if (expectedBetterPrice) {
      const strictlyOrdered =
        side === "bid"
          ? expectedBetterPrice.gt(level.price)
          : expectedBetterPrice.lt(level.price);
      if (!strictlyOrdered) {
        throw new Error("Price-level index is not strictly ordered");
      }
    }

    if (price.eq(level.price)) {
      throw new Error("Price level appeared during traversal; refresh and retry");
    }

    const belongsBeforeCurrent =
      side === "bid" ? price.gt(level.price) : price.lt(level.price);
    if (belongsBeforeCurrent) {
      return { betterLevel, worseLevel: currentLevel };
    }

    if (!level.worsePrice) {
      return { betterLevel: currentLevel, worseLevel: null };
    }

    expectedBetterPrice = level.price;
    betterLevel = currentLevel;
    currentPrice = level.worsePrice;
  }
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

export async function findOwnedTokenAccount(
  connection: Connection,
  owner: PublicKey,
  mint: PublicKey,
  minimumAmount = new BN(0),
) {
  const response = await connection.getParsedTokenAccountsByOwner(
    owner,
    { mint },
    "confirmed",
  );

  const tokenAccount = response.value.find(({ account }) => {
    if (!("parsed" in account.data)) return false;

    const amount = account.data.parsed?.info?.tokenAmount?.amount;
    return typeof amount === "string" && new BN(amount, 10).gte(minimumAmount);
  });

  if (!tokenAccount) {
    throw new Error(
      `No token account for ${mint.toBase58()} has enough available balance`,
    );
  }

  return tokenAccount.pubkey;
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
