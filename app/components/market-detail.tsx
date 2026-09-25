"use client";

import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  type FormEvent,
} from "react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { AnchorProvider, BN } from "@anchor-lang/core";
import { useAnchorWallet, useConnection } from "@solana/wallet-adapter-react";
import { PublicKey, SystemProgram, Transaction } from "@solana/web3.js";
import {
  accountExplorerUrl,
  decodeMarketStatus,
  deriveOrderPda,
  derivePriceLevelPda,
  deriveTraderBalancePda,
  deriveVaultAuthorityPda,
  formatAtomicAmount,
  findOwnedTokenAccount,
  findPriceLevelNeighbors,
  getTidebookAccounts,
  getTidebookProgram,
  getTidebookReadProgram,
  transactionExplorerUrl,
  deriveVaultPda,
  TOKEN_PROGRAM_ID,
  type MarketAccount,
  type OrderSide,
  type TraderBalanceAccount,
} from "../lib/tidebook";
import {
  MAX_MATCHES_PER_TRANSACTION,
  planBoundedMatches,
} from "../lib/matching-plan";
import { AppHeader } from "./app-header";

function shortAddress(value: string) {
  return `${value.slice(0, 8)}…${value.slice(-8)}`;
}

function getErrorMessage(cause: unknown) {
  return cause instanceof Error ? cause.message : "Transaction failed";
}

function parsePositiveU64(value: string, label: string) {
  const normalized = value.trim();

  if (!/^[0-9]+$/.test(normalized)) {
    throw new Error(`${label} must be a positive whole number`);
  }

  const number = new BN(normalized, 10);
  if (number.isZero()) {
    throw new Error(`${label} must be greater than zero`);
  }
  if (number.bitLength() > 64) {
    throw new Error(`${label} exceeds the u64 limit`);
  }

  return number;
}

type SubmissionResult =
  | {
      kind: "placed";
      signature: string;
      order: PublicKey;
    }
  | {
      kind: "matched";
      signature: string;
      makerCount: number;
      filledQuantity: BN;
      remainingQuantity: BN;
    };

export function MarketDetail({ address }: { address: string }) {
  const router = useRouter();
  const { connection } = useConnection();
  const wallet = useAnchorWallet();
  const [market, setMarket] = useState<MarketAccount | null>(null);
  const [loading, setLoading] = useState(true);
  const [side, setSide] = useState<OrderSide>("bid");
  const [price, setPrice] = useState("");
  const [quantity, setQuantity] = useState("");
  const [pending, setPending] = useState(false);
  const [traderBalance, setTraderBalance] =
    useState<TraderBalanceAccount | null>(null);
  const [balanceLoading, setBalanceLoading] = useState(false);
  const [balanceAsset, setBalanceAsset] = useState<"base" | "quote">("base");
  const [balanceAmount, setBalanceAmount] = useState("");
  const [balancePending, setBalancePending] = useState<
    "initialize" | "deposit" | "withdraw" | null
  >(null);
  const [balanceError, setBalanceError] = useState<string | null>(null);
  const [balanceSignature, setBalanceSignature] = useState<string | null>(null);
  const [lifecyclePending, setLifecyclePending] = useState<
    "pause" | "unpause" | "close" | null
  >(null);
  const [lifecycleSignature, setLifecycleSignature] = useState<string | null>(
    null,
  );
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<SubmissionResult | null>(null);
  const marketAddress = useMemo(() => {
    try {
      return new PublicKey(address);
    } catch {
      return null;
    }
  }, [address]);

  const readProgram = useMemo(
    () => getTidebookReadProgram(connection),
    [connection],
  );
  const signedProgram = useMemo(
    () => (wallet ? getTidebookProgram(connection, wallet) : null),
    [connection, wallet],
  );
  const traderBalanceAddress = useMemo(
    () =>
      marketAddress && wallet
        ? deriveTraderBalancePda(marketAddress, wallet.publicKey)
        : null,
    [marketAddress, wallet],
  );
  // Vault addresses are deterministic children of the market and mint, so the
  // UI can link to them without storing extra addresses in the Market account.
  const baseVault =
    market && marketAddress
      ? deriveVaultPda(marketAddress, market.baseMint)
      : null;

  const quoteVault =
    market && marketAddress
      ? deriveVaultPda(marketAddress, market.quoteMint)
      : null;

  const loadMarket = useCallback(async () => {
    if (!marketAddress) {
      setMarket(null);
      setError("Invalid market address");
      setLoading(false);
      return;
    }

    setLoading(true);
    setError(null);

    try {
      const account = await getTidebookAccounts(readProgram).market.fetchNullable(
        marketAddress,
      );
      setMarket(account);

      if (!account) {
        setError("Market account was not found on devnet");
      }
    } catch (cause) {
      setError(getErrorMessage(cause));
    } finally {
      setLoading(false);
    }
  }, [marketAddress, readProgram]);

  useEffect(() => {
    void loadMarket();
  }, [loadMarket]);

  const loadTraderBalance = useCallback(async () => {
    if (!traderBalanceAddress) {
      setTraderBalance(null);
      return;
    }

    setBalanceLoading(true);
    setBalanceError(null);
    try {
      const account = await getTidebookAccounts(
        readProgram,
      ).traderBalance.fetchNullable(traderBalanceAddress);
      setTraderBalance(account);
    } catch (cause) {
      setBalanceError(getErrorMessage(cause));
    } finally {
      setBalanceLoading(false);
    }
  }, [readProgram, traderBalanceAddress]);

  useEffect(() => {
    void loadTraderBalance();
  }, [loadTraderBalance]);

  const initializeBalance = async () => {
    if (!signedProgram || !wallet || !marketAddress || !traderBalanceAddress) {
      return;
    }

    setBalancePending("initialize");
    setBalanceError(null);
    setBalanceSignature(null);
    try {
      const signature = await signedProgram.methods
        .initializeTraderBalance()
        .accounts({
          owner: wallet.publicKey,
          market: marketAddress,
          traderBalance: traderBalanceAddress,
          systemProgram: SystemProgram.programId,
        })
        .rpc();
      setBalanceSignature(signature);
      await loadTraderBalance();
    } catch (cause) {
      setBalanceError(getErrorMessage(cause));
    } finally {
      setBalancePending(null);
    }
  };

  const transferBalance = async (action: "deposit" | "withdraw") => {
    if (
      !signedProgram ||
      !wallet ||
      !marketAddress ||
      !market ||
      !traderBalanceAddress ||
      !traderBalance
    ) {
      return;
    }

    setBalancePending(action);
    setBalanceError(null);
    setBalanceSignature(null);
    try {
      const amount = parsePositiveU64(balanceAmount, "Amount");
      const mint = balanceAsset === "base" ? market.baseMint : market.quoteMint;
      const tokenAccount = await findOwnedTokenAccount(
        connection,
        wallet.publicKey,
        mint,
        action === "deposit" ? amount : new BN(0),
      );
      const vaultAuthority = deriveVaultAuthorityPda(marketAddress);
      const marketVault = deriveVaultPda(marketAddress, mint);

      const signature =
        action === "deposit"
          ? await signedProgram.methods
              .deposit(amount)
              .accounts({
                owner: wallet.publicKey,
                market: marketAddress,
                traderBalance: traderBalanceAddress,
                depositMint: mint,
                traderTokenAccount: tokenAccount,
                vaultAuthority,
                marketVault,
                tokenProgram: TOKEN_PROGRAM_ID,
              })
              .rpc()
          : await signedProgram.methods
              .withdraw(amount)
              .accounts({
                owner: wallet.publicKey,
                market: marketAddress,
                traderBalance: traderBalanceAddress,
                withdrawalMint: mint,
                ownerTokenAccount: tokenAccount,
                vaultAuthority,
                marketVault,
                tokenProgram: TOKEN_PROGRAM_ID,
              })
              .rpc();

      setBalanceAmount("");
      setBalanceSignature(signature);
      await loadTraderBalance();
    } catch (cause) {
      setBalanceError(getErrorMessage(cause));
    } finally {
      setBalancePending(null);
    }
  };

  const placeOrder = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();

    if (
      !signedProgram ||
      !wallet ||
      !marketAddress ||
      !market ||
      !traderBalanceAddress
    ) {
      return;
    }

    setPending(true);
    setError(null);
    setResult(null);

    try {
      if (decodeMarketStatus(market.status) !== "active") {
        throw new Error("This market is paused and cannot accept new orders");
      }
      if (!traderBalance) {
        throw new Error("Initialize and fund your market balance first");
      }

      const rawPrice = parsePositiveU64(price, "Price");
      if (!rawPrice.mod(market.priceTickSize).isZero()) {
        throw new Error(
          `Price must be a multiple of ${market.priceTickSize.toString()}`,
        );
      }

      const rawQuantity = parsePositiveU64(quantity, "Quantity");
      if (!rawQuantity.mod(market.quantityLotSize).isZero()) {
        throw new Error(
          `Quantity must be a multiple of ${market.quantityLotSize.toString()}`,
        );
      }

      const orderSide = side === "bid" ? { bid: {} } : { ask: {} };
      const baseScale = new BN(10).pow(new BN(market.baseDecimals));
      const availableBalance =
        side === "bid" ? traderBalance.quoteFree : traderBalance.baseFree;
      const opposingBest = side === "bid" ? market.bestAsk : market.bestBid;
      const crossesBest = Boolean(
        opposingBest &&
          (side === "bid"
            ? rawPrice.gte(opposingBest)
            : rawPrice.lte(opposingBest)),
      );

      if (crossesBest && opposingBest) {
        // Build a bounded read-only snapshot of the FIFO path. Every matching
        // instruction still validates these relationships on-chain. If any
        // account changes before confirmation, the entire transaction rolls
        // back instead of committing only a prefix of the planned fills.
        const plan = await planBoundedMatches(
          signedProgram,
          marketAddress,
          market,
          wallet.publicKey,
          side,
          rawPrice,
          rawQuantity,
        );
        if (plan.steps.length === 0) {
          throw new Error("No crossing maker is currently available");
        }

        const filledQuantity = rawQuantity.sub(plan.remainingQuantity);
        // Quote settlement rounds once per fill at its maker price, matching
        // the program's arithmetic. Summing first and rounding later would be
        // incorrect when a taker crosses multiple price levels.
        const requiredBalance =
          side === "bid"
            ? plan.steps.reduce(
                (total, step) =>
                  total.add(
                    step.makerPrice.mul(step.fillQuantity).div(baseScale),
                  ),
                new BN(0),
              )
            : filledQuantity;
        if (availableBalance.lt(requiredBalance)) {
          throw new Error(
            `Insufficient free ${side === "bid" ? "quote" : "base"} balance`,
          );
        }

        const transaction = new Transaction();
        for (const step of plan.steps) {
          const instruction = await signedProgram.methods
            .matchLimitOrder(
              orderSide,
              rawPrice,
              step.takerQuantityBeforeFill,
            )
            .accountsPartial({
              taker: wallet.publicKey,
              market: marketAddress,
              makerOrder: step.makerOrder,
              makerPriceLevel: step.makerPriceLevel,
              nextOrder: step.nextOrder ?? signedProgram.programId,
              worseLevel: step.worseLevel ?? signedProgram.programId,
              levelRentRecipient:
                step.levelRentRecipient ?? signedProgram.programId,
              makerBalance: step.makerBalance,
              takerBalance: traderBalanceAddress,
            })
            .instruction();
          transaction.add(instruction);
        }

        // getTidebookProgram always constructs an AnchorProvider. The explicit
        // type communicates that this write path requires wallet signing, while
        // the read-only program elsewhere only exposes a generic Provider.
        const provider = signedProgram.provider as AnchorProvider;
        const signature = await provider.sendAndConfirm(transaction);

        setResult({
          kind: "matched",
          signature,
          makerCount: plan.steps.length,
          filledQuantity,
          remainingQuantity: plan.remainingQuantity,
        });
        // A capped remainder stays free: it is neither discarded nor silently
        // posted as a resting order. Keeping it visible lets the user submit
        // the next bounded batch deliberately.
        setQuantity(
          plan.remainingQuantity.isZero()
            ? ""
            : plan.remainingQuantity.toString(),
        );
        if (plan.remainingQuantity.isZero()) setPrice("");
      } else {
        const collateralAmount =
          side === "bid"
            ? rawPrice.mul(rawQuantity).div(baseScale)
            : rawQuantity;
        if (availableBalance.lt(collateralAmount)) {
          throw new Error(
            `Insufficient free ${side === "bid" ? "quote" : "base"} balance`,
          );
        }

        const order = deriveOrderPda(marketAddress, market.nextOrderId);
        const priceLevel = derivePriceLevelPda(marketAddress, side, rawPrice);
        const priceLevelState = await getTidebookAccounts(
          signedProgram,
        ).priceLevel.fetchNullable(priceLevel);
        let signature: string;

        if (priceLevelState) {
          if (!priceLevelState.lastOrder) {
            throw new Error("Existing price level has no FIFO tail");
          }

          signature = await signedProgram.methods
            .appendLimitOrder(orderSide, rawPrice, rawQuantity)
            .accounts({
              trader: wallet.publicKey,
              market: marketAddress,
              order,
              priceLevel,
              previousOrder: priceLevelState.lastOrder,
              traderBalance: traderBalanceAddress,
              systemProgram: SystemProgram.programId,
            })
            .rpc();
        } else {
          const currentBest = side === "bid" ? market.bestBid : market.bestAsk;
          const { betterLevel, worseLevel } = await findPriceLevelNeighbors(
            signedProgram,
            marketAddress,
            side,
            rawPrice,
            currentBest,
          );

          signature = await signedProgram.methods
            .insertLimitOrder(orderSide, rawPrice, rawQuantity)
            .accountsPartial({
              trader: wallet.publicKey,
              market: marketAddress,
              order,
              priceLevel,
              traderBalance: traderBalanceAddress,
              systemProgram: SystemProgram.programId,
              betterLevel: betterLevel ?? signedProgram.programId,
              worseLevel: worseLevel ?? signedProgram.programId,
            })
            .rpc();
        }

        setResult({ kind: "placed", signature, order });
        setPrice("");
        setQuantity("");
      }
      await Promise.all([loadMarket(), loadTraderBalance()]);
    } catch (cause) {
      setError(getErrorMessage(cause));
    } finally {
      setPending(false);
    }
  };

  const manageMarket = async (action: "pause" | "unpause" | "close") => {
    if (!signedProgram || !wallet || !marketAddress || !market) return;
    if (
      action === "close" &&
      !window.confirm(
        "Close this market and both empty vaults? This cannot be undone.",
      )
    ) {
      return;
    }

    setLifecyclePending(action);
    setLifecycleSignature(null);
    setError(null);

    try {
      let signature: string;

      if (action === "pause") {
        signature = await signedProgram.methods
          .pauseMarket()
          .accounts({ authority: wallet.publicKey, market: marketAddress })
          .rpc();
      } else if (action === "unpause") {
        signature = await signedProgram.methods
          .unpauseMarket()
          .accounts({ authority: wallet.publicKey, market: marketAddress })
          .rpc();
      } else {
        if (!market.openOrderCount.isZero()) {
          throw new Error("Cancel every open order before closing this market");
        }

        signature = await signedProgram.methods
          .closeMarket()
          .accounts({
            authority: wallet.publicKey,
            market: marketAddress,
            vaultAuthority: deriveVaultAuthorityPda(marketAddress),
            baseVault: deriveVaultPda(marketAddress, market.baseMint),
            quoteVault: deriveVaultPda(marketAddress, market.quoteMint),
            tokenProgram: TOKEN_PROGRAM_ID,
          })
          .rpc();
      }

      setLifecycleSignature(signature);

      if (action === "close") {
        router.push("/markets");
        router.refresh();
      } else {
        await loadMarket();
      }
    } catch (cause) {
      setError(getErrorMessage(cause));
    } finally {
      setLifecyclePending(null);
    }
  };

  const status = market ? decodeMarketStatus(market.status) : null;
  const isMarketAuthority = Boolean(
    wallet && market && wallet.publicKey.equals(market.authority),
  );
  const pricePreview = useMemo(() => {
    if (!market || !/^[0-9]+$/.test(price.trim())) return null;
    return formatAtomicAmount(new BN(price.trim(), 10), market.quoteDecimals);
  }, [market, price]);
  const quantityPreview = useMemo(() => {
    if (!market || !/^[0-9]+$/.test(quantity.trim())) return null;
    return formatAtomicAmount(
      new BN(quantity.trim(), 10),
      market.baseDecimals,
    );
  }, [market, quantity]);
  const collateralPreview = useMemo(() => {
    if (
      !market ||
      !/^[0-9]+$/.test(price.trim()) ||
      !/^[0-9]+$/.test(quantity.trim())
    ) {
      return null;
    }

    const rawPrice = new BN(price.trim(), 10);
    const rawQuantity = new BN(quantity.trim(), 10);

    if (side === "ask") {
      return `${formatAtomicAmount(rawQuantity, market.baseDecimals)} base tokens`;
    }

    const baseScale = new BN(10).pow(new BN(market.baseDecimals));
    const quoteAmount = rawPrice.mul(rawQuantity).div(baseScale);
    return `${formatAtomicAmount(quoteAmount, market.quoteDecimals)} quote tokens`;
  }, [market, price, quantity, side]);
  const crossesBest = useMemo(() => {
    if (!market || !/^[0-9]+$/.test(price.trim())) return false;
    const rawPrice = new BN(price.trim(), 10);
    const opposingBest = side === "bid" ? market.bestAsk : market.bestBid;
    if (!opposingBest) return false;
    return side === "bid"
      ? rawPrice.gte(opposingBest)
      : rawPrice.lte(opposingBest);
  }, [market, price, side]);

  return (
    <div className="app-shell">
      <AppHeader />

      <main className="route-shell">
        <div className="market-detail-back">
          <Link href="/markets">← All markets</Link>
        </div>

        {loading && <div className="access-card">Loading market…</div>}

        {!loading && market && marketAddress && status && (
          <>
            <div className="route-heading market-detail-heading">
              <div>
                <div className="eyebrow">Market details</div>
                <h1>{shortAddress(marketAddress.toBase58())}</h1>
                <p>
                  Cross the best FIFO maker or post a collateralized limit
                  order. Every completed fill settles internal balances
                  atomically at the resting maker price.
                </p>
              </div>
              <span className={`role-badge market-${status}`}>{status}</span>
            </div>

            <div className="market-detail-layout">
              <section className="admin-card market-information">
                <div className="card-label">Pair</div>
                <div className="market-detail-pair">
                  <div>
                    <span>Base mint</span>
                    <strong>{market.baseMint.toBase58()}</strong>
                  </div>
                  <div>
                    <span>Quote mint</span>
                    <strong>{market.quoteMint.toBase58()}</strong>
                  </div>
                </div>

                <dl className="market-metadata">
                  <div>
                    <dt>Authority</dt>
                    <dd>{shortAddress(market.authority.toBase58())}</dd>
                  </div>
                  <div>
                    <dt>Next order</dt>
                    <dd>#{market.nextOrderId.toString()}</dd>
                  </div>
                  <div>
                    <dt>Open orders</dt>
                    <dd>{market.openOrderCount.toString()}</dd>
                  </div>
                  <div>
                    <dt>Best bid</dt>
                    <dd>{market.bestBid?.toString() ?? "No open bids"}</dd>
                  </div>
                  <div>
                    <dt>Best ask</dt>
                    <dd>{market.bestAsk?.toString() ?? "No open asks"}</dd>
                  </div>
                  <div>
                    <dt>Base decimals</dt>
                    <dd>{market.baseDecimals}</dd>
                  </div>
                  <div>
                    <dt>Quote decimals</dt>
                    <dd>{market.quoteDecimals}</dd>
                  </div>
                  <div>
                    <dt>Price tick</dt>
                    <dd>
                      {market.priceTickSize.toString()} raw (
                      {formatAtomicAmount(
                        market.priceTickSize,
                        market.quoteDecimals,
                      )} quote)
                    </dd>
                  </div>
                  <div>
                    <dt>Quantity lot</dt>
                    <dd>
                      {market.quantityLotSize.toString()} raw (
                      {formatAtomicAmount(
                        market.quantityLotSize,
                        market.baseDecimals,
                      )} base)
                    </dd>
                  </div>
                  {baseVault && (
                    <div>
                      <dt>Base vault</dt>
                      <dd>
                        <a
                          href={accountExplorerUrl(baseVault)}
                          target="_blank"
                          rel="noreferrer"
                        >
                          {shortAddress(baseVault.toBase58())} ↗
                        </a>
                      </dd>
                    </div>
                  )}

                  {quoteVault && (
                    <div>
                      <dt>Quote vault</dt>
                      <dd>
                        <a
                          href={accountExplorerUrl(quoteVault)}
                          target="_blank"
                          rel="noreferrer"
                        >
                          {shortAddress(quoteVault.toBase58())} ↗
                        </a>
                      </dd>
                    </div>
                  )}
                </dl>

                <a
                  className="market-explorer-link"
                  href={accountExplorerUrl(marketAddress)}
                  target="_blank"
                  rel="noreferrer"
                >
                  View market account on Explorer ↗
                </a>

                {isMarketAuthority && (
                  <div className="market-card-actions">
                    {status === "active" ? (
                      <button
                        className="admin-action-button"
                        type="button"
                        disabled={lifecyclePending !== null}
                        onClick={() => void manageMarket("pause")}
                      >
                        {lifecyclePending === "pause"
                          ? "Pausing…"
                          : "Pause market"}
                      </button>
                    ) : (
                      <>
                        <button
                          className="admin-action-button"
                          type="button"
                          disabled={lifecyclePending !== null}
                          onClick={() => void manageMarket("unpause")}
                        >
                          {lifecyclePending === "unpause"
                            ? "Unpausing…"
                            : "Unpause market"}
                        </button>
                        <button
                          className="danger-button"
                          type="button"
                          disabled={
                            lifecyclePending !== null ||
                            !market.openOrderCount.isZero()
                          }
                          title={
                            market.openOrderCount.isZero()
                              ? "Close this market and both empty vaults"
                              : "Cancel every open order before closing"
                          }
                          onClick={() => void manageMarket("close")}
                        >
                          {lifecyclePending === "close"
                            ? "Closing…"
                            : "Close market"}
                        </button>
                      </>
                    )}
                  </div>
                )}
              </section>

              <section className="admin-card">
                <div className="card-label">Limit order</div>
                <h2>Place an order</h2>

                {status === "paused" && (
                  <div className="market-order-notice">
                    This market is paused. Existing orders may still be canceled,
                    but new orders are disabled.
                  </div>
                )}

                {status === "active" && !wallet && (
                  <div className="market-order-notice">
                    Connect your wallet to place an order.
                  </div>
                )}

                {status === "active" && wallet && (
                  <form className="market-create-form" onSubmit={placeOrder}>
                    {!traderBalance && !balanceLoading && (
                      <div className="market-order-notice">
                        Initialize your market balance below before placing an
                        order.
                      </div>
                    )}
                    <label>
                      Side
                      <select
                        value={side}
                        onChange={(event) =>
                          setSide(event.target.value as OrderSide)
                        }
                      >
                        <option value="bid">Bid — buy base</option>
                        <option value="ask">Ask — sell base</option>
                      </select>
                    </label>

                    <label>
                      Raw price
                      <input
                        inputMode="numeric"
                        value={price}
                        onChange={(event) => setPrice(event.target.value)}
                        placeholder="100"
                        autoComplete="off"
                      />
                      <small>
                        {pricePreview === null
                          ? `Tick: ${market.priceTickSize.toString()} raw units`
                          : `${pricePreview} quote tokens per base token`}
                      </small>
                    </label>

                    <label>
                      Raw quantity
                      <input
                        inputMode="numeric"
                        value={quantity}
                        onChange={(event) => setQuantity(event.target.value)}
                        placeholder="5"
                        autoComplete="off"
                      />
                      <small>
                        {quantityPreview === null
                          ? `Lot: ${market.quantityLotSize.toString()} raw units`
                          : `${quantityPreview} base tokens`}
                      </small>
                    </label>

                    <button
                      className="primary-button"
                      type="submit"
                      disabled={
                        !traderBalance ||
                        !price.trim() ||
                        !quantity.trim() ||
                        pending
                      }
                    >
                      {pending
                        ? "Submitting…"
                        : crossesBest
                          ? `Match best ${side === "bid" ? "ask" : "bid"}`
                          : `Place ${side}`}
                    </button>
                    {crossesBest ? (
                      <small>
                        This transaction processes up to {MAX_MATCHES_PER_TRANSACTION}{" "}
                        FIFO makers atomically. Any larger remainder stays free
                        and remains in this form.
                      </small>
                    ) : collateralPreview ? (
                      <small>This order will lock {collateralPreview}.</small>
                    ) : null}
                  </form>
                )}
              </section>
            </div>

            {wallet && (
              <section className="admin-card market-balance-card">
                <div className="admin-list-heading">
                  <div>
                    <div className="card-label">Internal balance</div>
                    <h2>Fund this market</h2>
                  </div>
                  <button
                    className="text-button"
                    type="button"
                    disabled={balanceLoading || balancePending !== null}
                    onClick={() => void loadTraderBalance()}
                  >
                    {balanceLoading ? "Refreshing…" : "Refresh"}
                  </button>
                </div>

                {!balanceLoading && !traderBalance && (
                  <div className="balance-initialize">
                    <p>
                      Create one balance ledger for this wallet and market.
                      This does not move tokens.
                    </p>
                    <button
                      className="primary-button"
                      type="button"
                      disabled={balancePending !== null}
                      onClick={() => void initializeBalance()}
                    >
                      {balancePending === "initialize"
                        ? "Initializing…"
                        : "Initialize balance"}
                    </button>
                  </div>
                )}

                {traderBalance && (
                  <>
                    <dl className="market-metadata balance-metadata">
                      <div>
                        <dt>Base free</dt>
                        <dd>{traderBalance.baseFree.toString()}</dd>
                      </div>
                      <div>
                        <dt>Base locked</dt>
                        <dd>{traderBalance.baseLocked.toString()}</dd>
                      </div>
                      <div>
                        <dt>Quote free</dt>
                        <dd>{traderBalance.quoteFree.toString()}</dd>
                      </div>
                      <div>
                        <dt>Quote locked</dt>
                        <dd>{traderBalance.quoteLocked.toString()}</dd>
                      </div>
                    </dl>

                    <form
                      className="market-create-form balance-form"
                      onSubmit={(event) => event.preventDefault()}
                    >
                      <label>
                        Asset
                        <select
                          value={balanceAsset}
                          onChange={(event) =>
                            setBalanceAsset(
                              event.target.value as "base" | "quote",
                            )
                          }
                        >
                          <option value="base">Base token</option>
                          <option value="quote">Quote token</option>
                        </select>
                      </label>
                      <label>
                        Raw amount
                        <input
                          inputMode="numeric"
                          value={balanceAmount}
                          onChange={(event) =>
                            setBalanceAmount(event.target.value)
                          }
                          placeholder="1000"
                          autoComplete="off"
                        />
                      </label>
                      <div className="balance-actions">
                        <button
                          className="primary-button"
                          type="button"
                          disabled={!balanceAmount.trim() || balancePending !== null}
                          onClick={() => void transferBalance("deposit")}
                        >
                          {balancePending === "deposit"
                            ? "Depositing…"
                            : "Deposit"}
                        </button>
                        <button
                          className="admin-action-button"
                          type="button"
                          disabled={!balanceAmount.trim() || balancePending !== null}
                          onClick={() => void transferBalance("withdraw")}
                        >
                          {balancePending === "withdraw"
                            ? "Withdrawing…"
                            : "Withdraw free balance"}
                        </button>
                      </div>
                      <small>
                        Orders move free balance to locked balance. Cancellation
                        releases it back to free; only withdrawal moves tokens
                        back to your wallet.
                      </small>
                    </form>
                  </>
                )}

                {balanceError && (
                  <div className="transaction-message transaction-error">
                    {balanceError}
                  </div>
                )}
                {balanceSignature && (
                  <div className="transaction-message transaction-success">
                    Balance updated. {" "}
                    <a
                      href={transactionExplorerUrl(balanceSignature)}
                      target="_blank"
                      rel="noreferrer"
                    >
                      View transaction ↗
                    </a>
                  </div>
                )}
              </section>
            )}
          </>
        )}

        {error && (
          <div className="transaction-message transaction-error">{error}</div>
        )}

        {result?.kind === "placed" && (
          <div className="transaction-message transaction-success">
            Order created at {shortAddress(result.order.toBase58())}.{" "}
            <a
              href={transactionExplorerUrl(result.signature)}
              target="_blank"
              rel="noreferrer"
            >
              View transaction ↗
            </a>
          </div>
        )}

        {result?.kind === "matched" && (
          <div className="transaction-message transaction-success">
            Filled {result.filledQuantity.toString()} raw base across {" "}
            {result.makerCount} maker{result.makerCount === 1 ? "" : "s"}.
            {!result.remainingQuantity.isZero() && (
              <> {result.remainingQuantity.toString()} remains unprocessed.</>
            )}{" "}
            <a
              href={transactionExplorerUrl(result.signature)}
              target="_blank"
              rel="noreferrer"
            >
              View transaction ↗
            </a>
          </div>
        )}

        {lifecycleSignature && (
          <div className="transaction-message transaction-success">
            Market lifecycle updated.{" "}
            <a
              href={transactionExplorerUrl(lifecycleSignature)}
              target="_blank"
              rel="noreferrer"
            >
              View transaction ↗
            </a>
          </div>
        )}
      </main>
    </div>
  );
}
