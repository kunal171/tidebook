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
import { BN } from "@anchor-lang/core";
import { useAnchorWallet, useConnection } from "@solana/wallet-adapter-react";
import { PublicKey, SystemProgram } from "@solana/web3.js";
import {
  accountExplorerUrl,
  decodeMarketStatus,
  deriveOrderPda,
  deriveVaultAuthorityPda,
  formatAtomicAmount,
  findOwnedTokenAccount,
  getTidebookAccounts,
  getTidebookProgram,
  getTidebookReadProgram,
  transactionExplorerUrl,
  deriveVaultPda,
  TOKEN_PROGRAM_ID,
  type MarketAccount,
  type OrderSide,
} from "../lib/tidebook";
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
  const [lifecyclePending, setLifecyclePending] = useState<
    "pause" | "unpause" | "close" | null
  >(null);
  const [lifecycleSignature, setLifecycleSignature] = useState<string | null>(
    null,
  );
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<{
    signature: string;
    order: PublicKey;
  } | null>(null);
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

  const placeOrder = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();

    if (!signedProgram || !wallet || !marketAddress || !market) return;

    setPending(true);
    setError(null);
    setResult(null);

    try {
      if (decodeMarketStatus(market.status) !== "active") {
        throw new Error("This market is paused and cannot accept new orders");
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

      const order = deriveOrderPda(marketAddress, market.nextOrderId);
      const orderSide = side === "bid" ? { bid: {} } : { ask: {} };
      const collateralMint = side === "bid" ? market.quoteMint : market.baseMint;
      const baseScale = new BN(10).pow(new BN(market.baseDecimals));
      const collateralAmount =
        side === "bid"
          ? rawPrice.mul(rawQuantity).div(baseScale)
          : rawQuantity;
      const traderCollateral = await findOwnedTokenAccount(
        connection,
        wallet.publicKey,
        collateralMint,
        collateralAmount,
      );
      const vaultAuthority = deriveVaultAuthorityPda(marketAddress);
      const marketVault = deriveVaultPda(marketAddress, collateralMint);

      const signature = await signedProgram.methods
        .placeLimitOrder(orderSide, rawPrice, rawQuantity)
        .accounts({
          trader: wallet.publicKey,
          market: marketAddress,
          order,
          collateralMint,
          traderCollateral,
          vaultAuthority,
          marketVault,
          tokenProgram: TOKEN_PROGRAM_ID,
          systemProgram: SystemProgram.programId,
        })
        .rpc();

      setResult({ signature, order });
      setPrice("");
      setQuantity("");
      await loadMarket();
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
                  Submit collateralized limit orders. Bids lock quote tokens;
                  asks lock base tokens until cancellation or future matching.
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
                    <dd>{market.bestBid?.toString() ?? "Not maintained"}</dd>
                  </div>
                  <div>
                    <dt>Best ask</dt>
                    <dd>{market.bestAsk?.toString() ?? "Not maintained"}</dd>
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
                      disabled={!price.trim() || !quantity.trim() || pending}
                    >
                      {pending ? "Placing order…" : `Place ${side}`}
                    </button>
                    {collateralPreview && (
                      <small>This order will lock {collateralPreview}.</small>
                    )}
                  </form>
                )}
              </section>
            </div>
          </>
        )}

        {error && (
          <div className="transaction-message transaction-error">{error}</div>
        )}

        {result && (
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
