"use client";

import { useCallback, useEffect, useMemo, useState } from "react";
import Link from "next/link";
import { useAnchorWallet, useConnection } from "@solana/wallet-adapter-react";

import {
  decodeOrderSide,
  decodeOrderStatus,
  derivePriceLevelPda,
  deriveTraderBalancePda,
  getTidebookAccounts,
  getTidebookProgram,
  transactionExplorerUrl,
  type OrderView,
} from "../lib/tidebook";
import { AppHeader } from "./app-header";

function shortAddress(value: string) {
  return `${value.slice(0, 6)}…${value.slice(-6)}`;
}

function getErrorMessage(cause: unknown) {
  return cause instanceof Error ? cause.message : "Transaction failed";
}

export function Orders() {
  const { connection } = useConnection();
  const wallet = useAnchorWallet();

  const [orders, setOrders] = useState<OrderView[]>([]);
  const [loading, setLoading] = useState(false);
  const [pendingOrder, setPendingOrder] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [signature, setSignature] = useState<string | null>(null);

  const program = useMemo(
    () => (wallet ? getTidebookProgram(connection, wallet) : null),
    [connection, wallet],
  );

  const loadOrders = useCallback(async () => {
    if (!program || !wallet) {
      setOrders([]);
      return;
    }

    setLoading(true);
    setError(null);

    try {
      const records = await getTidebookAccounts(program).order.all([
        {
          memcmp: {
            offset: 8,
            bytes: wallet.publicKey.toBase58(),
          },
        },
      ]);

      setOrders(
        records
          .map(({ publicKey, account }) => ({
            address: publicKey,
            ...account,
          }))
          .sort((left, right) =>
            right.orderId.cmp(left.orderId),
          ),
      );
    } catch (cause) {
      setError(getErrorMessage(cause));
    } finally {
      setLoading(false);
    }
  }, [program, wallet]);

  useEffect(() => {
    void loadOrders();
  }, [loadOrders]);

  const cancelOrder = async (order: OrderView) => {
    if (!program || !wallet) return;

    const address = order.address.toBase58();

    setPendingOrder(address);
    setError(null);
    setSignature(null);

    try {
      const accounts = getTidebookAccounts(program);
      const priceLevel = await accounts.priceLevel.fetchNullable(
        order.priceLevel,
      );
      if (!priceLevel) {
        throw new Error("The order's price-level account was not found");
      }

      const side = decodeOrderSide(order.side);
      const removesPriceLevel = priceLevel.orderCount.eqn(1);
      const betterLevel =
        removesPriceLevel && priceLevel.betterPrice
          ? derivePriceLevelPda(
              order.market,
              side,
              priceLevel.betterPrice,
            )
          : null;
      const worseLevel =
        removesPriceLevel && priceLevel.worsePrice
          ? derivePriceLevelPda(
              order.market,
              side,
              priceLevel.worsePrice,
            )
          : null;
      const levelRentRecipient = removesPriceLevel
        ? priceLevel.rentPayer
        : null;
      const traderBalance = deriveTraderBalancePda(
        order.market,
        wallet.publicKey,
      );

      const transaction = await program.methods
        .cancelLimitOrder(order.orderId)
        .accountsPartial({
          owner: wallet.publicKey,
          market: order.market,
          order: order.address,
          priceLevel: order.priceLevel,
          previousOrder: order.previousOrder ?? program.programId,
          nextOrder: order.nextOrder ?? program.programId,
          betterLevel: betterLevel ?? program.programId,
          worseLevel: worseLevel ?? program.programId,
          levelRentRecipient: levelRentRecipient ?? program.programId,
          traderBalance,
        })
        .rpc();

      setSignature(transaction);
      await loadOrders();
    } catch (cause) {
      setError(getErrorMessage(cause));
    } finally {
      setPendingOrder(null);
    }
  };

  return (
    <div className="app-shell">
      <AppHeader />

      <main className="route-shell">
        <div className="route-heading">
          <div className="eyebrow">Trader workspace</div>
          <h1>My orders</h1>
          <p>
            Orders owned by the connected wallet. Filled and canceled records
            remain visible as history; only open orders can be canceled.
          </p>
        </div>

        {!wallet && (
          <div className="access-card">
            Connect your wallet to view your orders.
          </div>
        )}

        {wallet && (
          <section className="admin-card">
            <div className="admin-list-heading">
              <h2>Orders</h2>

              <button
                className="text-button"
                type="button"
                disabled={loading}
                onClick={() => void loadOrders()}
              >
                {loading ? "Refreshing…" : "Refresh"}
              </button>
            </div>

            {!loading && orders.length === 0 && (
              <p>No orders found for this wallet.</p>
            )}

            <div className="orders-list">
              {orders.map((order) => {
                const status = decodeOrderStatus(order.status);
                const side = decodeOrderSide(order.side);
                const address = order.address.toBase58();

                return (
                  <article className="order-row" key={address}>
                    <div className="order-details">
                      <div>
                        <span>Order</span>
                        <strong>#{order.orderId.toString()}</strong>
                      </div>

                      <div>
                        <span>Market</span>
                        <strong>
                          <Link href={`/markets/${order.market.toBase58()}`}>
                            {shortAddress(order.market.toBase58())}
                          </Link>
                        </strong>
                      </div>

                      <div>
                        <span>Side</span>
                        <strong>{side}</strong>
                      </div>

                      <div>
                        <span>Price</span>
                        <strong>{order.price.toString()}</strong>
                      </div>

                      <div>
                        <span>Quantity</span>
                        <strong>{order.quantity.toString()}</strong>
                      </div>

                      <div>
                        <span>Remaining</span>
                        <strong>
                          {order.remainingQuantity.toString()}
                        </strong>
                      </div>

                      <div>
                        <span>Filled</span>
                        <strong>
                          {order.quantity
                            .sub(order.remainingQuantity)
                            .toString()}
                        </strong>
                      </div>

                      <div>
                        <span>Locked collateral</span>
                        <strong>
                          {order.lockedCollateral.toString()} raw{" "}
                          {side === "bid" ? "quote" : "base"}
                        </strong>
                      </div>
                    </div>

                    <div className="order-actions">
                      <span className={`role-badge order-${status}`}>
                        {status}
                      </span>

                      {status === "open" && (
                        <button
                          className="danger-button"
                          type="button"
                          disabled={pendingOrder !== null}
                          onClick={() => void cancelOrder(order)}
                        >
                          {pendingOrder === address
                            ? "Canceling…"
                            : "Cancel"}
                        </button>
                      )}
                    </div>
                  </article>
                );
              })}
            </div>
          </section>
        )}

        {error && (
          <div className="transaction-message transaction-error">
            {error}
          </div>
        )}

        {signature && (
          <div className="transaction-message transaction-success">
            Order canceled and collateral released to your free balance.{" "}
            <a
              href={transactionExplorerUrl(signature)}
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
