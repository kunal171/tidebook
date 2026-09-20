"use client";

import { useCallback, useEffect, useMemo, useState } from "react";
import Link from "next/link";
import { useConnection } from "@solana/wallet-adapter-react";
import {
  accountExplorerUrl,
  decodeMarketStatus,
  getTidebookAccounts,
  getTidebookReadProgram,
  type MarketView,
} from "../lib/tidebook";
import { AppHeader } from "./app-header";

function shortAddress(value: string) {
  return `${value.slice(0, 6)}…${value.slice(-6)}`;
}

function getErrorMessage(cause: unknown) {
  return cause instanceof Error ? cause.message : "Unable to load markets";
}

export function Markets() {
  const { connection } = useConnection();
  const [markets, setMarkets] = useState<MarketView[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const program = useMemo(
    () => getTidebookReadProgram(connection),
    [connection],
  );

  const loadMarkets = useCallback(async () => {
    setLoading(true);
    setError(null);

    try {
      const records = await getTidebookAccounts(program).market.all();
      setMarkets(
        records
          .map(({ publicKey, account }) => ({
            address: publicKey,
            ...account,
          }))
          .sort((left, right) => {
            const leftStatus = decodeMarketStatus(left.status);
            const rightStatus = decodeMarketStatus(right.status);

            if (leftStatus !== rightStatus) {
              return leftStatus === "active" ? -1 : 1;
            }

            return left.address
              .toBase58()
              .localeCompare(right.address.toBase58());
          }),
      );
    } catch (cause) {
      setError(getErrorMessage(cause));
    } finally {
      setLoading(false);
    }
  }, [program]);

  useEffect(() => {
    void loadMarkets();
  }, [loadMarkets]);

  const activeCount = markets.filter(
    (market) => decodeMarketStatus(market.status) === "active",
  ).length;

  return (
    <div className="app-shell">
      <AppHeader />

      <main className="route-shell">
        <div className="route-heading markets-heading">
          <div>
            <div className="eyebrow">Market discovery</div>
            <h1>Markets</h1>
            <p>
              Explore Tidebook markets without connecting a wallet. Active
              markets can accept new orders; paused markets remain visible for
              inspection and cancellation.
            </p>
          </div>

          <div className="market-summary" aria-label="Market summary">
            <strong>{activeCount}</strong>
            <span>Active of {markets.length}</span>
          </div>
        </div>

        <div className="market-list-heading">
          <h2>On-chain markets</h2>
          <button
            className="text-button"
            type="button"
            disabled={loading}
            onClick={() => void loadMarkets()}
          >
            {loading ? "Refreshing…" : "Refresh"}
          </button>
        </div>

        {loading && markets.length === 0 && (
          <div className="access-card">Loading markets…</div>
        )}

        {!loading && !error && markets.length === 0 && (
          <div className="access-card">No markets have been created yet.</div>
        )}

        <section className="markets-grid" aria-live="polite">
          {markets.map((market) => {
            const status = decodeMarketStatus(market.status);

            return (
              <article className="market-card" key={market.address.toBase58()}>
                <div className="market-card-heading">
                  <div>
                    <span className="card-label">Market</span>
                    <strong>{shortAddress(market.address.toBase58())}</strong>
                  </div>
                  <span className={`role-badge market-${status}`}>{status}</span>
                </div>

                <div className="market-pair">
                  <div>
                    <span>Base mint</span>
                    <strong>{shortAddress(market.baseMint.toBase58())}</strong>
                  </div>
                  <span aria-hidden="true">/</span>
                  <div>
                    <span>Quote mint</span>
                    <strong>{shortAddress(market.quoteMint.toBase58())}</strong>
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
                    <dt>Price tick</dt>
                    <dd>{market.priceTickSize.toString()}</dd>
                  </div>
                  <div>
                    <dt>Quantity lot</dt>
                    <dd>{market.quantityLotSize.toString()}</dd>
                  </div>
                  <div>
                    <dt>Best bid</dt>
                    <dd>{market.bestBid?.toString() ?? "—"}</dd>
                  </div>
                  <div>
                    <dt>Best ask</dt>
                    <dd>{market.bestAsk?.toString() ?? "—"}</dd>
                  </div>
                </dl>

                <div className="market-card-actions">
                  <Link
                    className="market-open-link"
                    href={`/markets/${market.address.toBase58()}`}
                  >
                    {status === "active" ? "Open market" : "View details"}
                  </Link>
                  <a
                    className="market-explorer-link"
                    href={accountExplorerUrl(market.address)}
                    target="_blank"
                    rel="noreferrer"
                  >
                    Explorer ↗
                  </a>
                </div>
              </article>
            );
          })}
        </section>

        {error && (
          <div className="transaction-message transaction-error">{error}</div>
        )}
      </main>
    </div>
  );
}
