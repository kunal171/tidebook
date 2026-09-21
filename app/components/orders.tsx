"use client";

import { useCallback, useEffect, useMemo, useState } from "react";
import { useAnchorWallet, useConnection } from "@solana/wallet-adapter-react";

import {
  decodeOrderSide,
  decodeOrderStatus,
  deriveVaultAuthorityPda,
  deriveVaultPda,
  findOwnedTokenAccount,
  getTidebookAccounts,
  getTidebookProgram,
  TOKEN_PROGRAM_ID,
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
      const market = await getTidebookAccounts(program).market.fetchNullable(
        order.market,
      );
      if (!market) {
        throw new Error("The order's market account was not found");
      }

      const side = decodeOrderSide(order.side);
      const collateralMint = side === "bid" ? market.quoteMint : market.baseMint;
      const ownerCollateral = await findOwnedTokenAccount(
        connection,
        wallet.publicKey,
        collateralMint,
      );
      const vaultAuthority = deriveVaultAuthorityPda(order.market);
      const marketVault = deriveVaultPda(order.market, collateralMint);

      const transaction = await program.methods
        .cancelLimitOrder(order.orderId)
        .accounts({
          owner: wallet.publicKey,
          market: order.market,
          order: order.address,
          collateralMint,
          ownerCollateral,
          vaultAuthority,
          marketVault,
          tokenProgram: TOKEN_PROGRAM_ID,
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
            Orders owned by the connected wallet. Open orders can be
            canceled even while their market is paused.
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
                          {shortAddress(order.market.toBase58())}
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
            Order canceled.{" "}
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
