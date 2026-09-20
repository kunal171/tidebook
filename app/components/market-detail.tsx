"use client";

import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  type FormEvent,
} from "react";
import Link from "next/link";
import { BN } from "@anchor-lang/core";
import { useAnchorWallet, useConnection } from "@solana/wallet-adapter-react";
import { PublicKey, SystemProgram } from "@solana/web3.js";
import {
  accountExplorerUrl,
  decodeMarketStatus,
  deriveOrderPda,
  formatAtomicAmount,
  getTidebookAccounts,
  getTidebookProgram,
  getTidebookReadProgram,
  transactionExplorerUrl,
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
  const { connection } = useConnection();
  const wallet = useAnchorWallet();
  const [market, setMarket] = useState<MarketAccount | null>(null);
  const [loading, setLoading] = useState(true);
  const [side, setSide] = useState<OrderSide>("bid");
  const [price, setPrice] = useState("");
  const [quantity, setQuantity] = useState("");
  const [pending, setPending] = useState(false);
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

      const signature = await signedProgram.methods
        .placeLimitOrder(orderSide, rawPrice, rawQuantity)
        .accounts({
          trader: wallet.publicKey,
          market: marketAddress,
          order,
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

  const status = market ? decodeMarketStatus(market.status) : null;
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
                  Submit raw integer limit orders to this research market. No
                  assets are locked or transferred at the current milestone.
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
                </dl>

                <a
                  className="market-explorer-link"
                  href={accountExplorerUrl(marketAddress)}
                  target="_blank"
                  rel="noreferrer"
                >
                  View market account on Explorer ↗
                </a>
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
      </main>
    </div>
  );
}
