"use client";

import { useMemo, useState } from "react";
import { BN } from "@anchor-lang/core";
import { useAnchorWallet, useConnection } from "@solana/wallet-adapter-react";
import { PublicKey, SystemProgram } from "@solana/web3.js";
import {
  deriveAdminRecordPda,
  deriveMarketPda,
  getTidebookProgram,
  transactionExplorerUrl,
} from "../lib/tidebook";
import { AppHeader } from "./app-header";
import { useProtocolRole } from "./protocol-role-provider";

function getErrorMessage(cause: unknown) {
  return cause instanceof Error ? cause.message : "Market creation failed";
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

export function CreateMarket() {
  const { connection } = useConnection();
  const wallet = useAnchorWallet();
  const { role, loading: roleLoading, error: roleError } = useProtocolRole();
  const [baseMint, setBaseMint] = useState("");
  const [quoteMint, setQuoteMint] = useState("");
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<{ signature: string; market: PublicKey } | null>(null);
  const [priceTickSize, setPriceTickSize] = useState("");
  const [quantityLotSize, setQuantityLotSize] = useState("");

  const program = useMemo(
    () => (wallet ? getTidebookProgram(connection, wallet) : null),
    [connection, wallet],
  );

  const createMarket = async (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!program || !wallet) return;

    setPending(true);
    setError(null);
    setResult(null);

    try {
      const base = new PublicKey(baseMint.trim());
      const quote = new PublicKey(quoteMint.trim());

      if (base.equals(quote)) {
        throw new Error("Base and quote mints must be different");
      }
      const tickSize = parsePositiveU64(priceTickSize, "Price tick size");
      const lotSize = parsePositiveU64(quantityLotSize, "Quantity lot size");

      const market = deriveMarketPda(base, quote);
      const signature = await program.methods
        .initializeMarket(tickSize, lotSize)
        .accounts({
          authority: wallet.publicKey,
          adminRecord: deriveAdminRecordPda(wallet.publicKey),
          market,
          baseMint: base,
          quoteMint: quote,
          systemProgram: SystemProgram.programId,
        })
        .rpc();

      setResult({ signature, market });
    } catch (cause) {
      setError(getErrorMessage(cause));
    } finally {
      setPending(false);
    }
  };

  const isAdmin = role === "super-admin" || role === "admin";

  return (
    <div className="app-shell">
      <AppHeader />
      <main className="route-shell route-shell-narrow">
        <div className="route-heading">
          <div className="eyebrow">Canonical market</div>
          <h1>Create a market</h1>
          <p>The connected wallet must have an active on-chain administrator record.</p>
        </div>

        {!wallet && <div className="access-card">Connect your wallet to continue.</div>}
        {wallet && roleLoading && <div className="access-card">Checking administrator role…</div>}
        {wallet && !roleLoading && !roleError && !isAdmin && (
          <div className="access-card access-denied">
            This route requires an active administrator wallet.
          </div>
        )}

        {wallet && !roleLoading && isAdmin && (
          <form className="admin-card market-create-form" onSubmit={(event) => void createMarket(event)}>
            <label>
              Base mint
              <input
                value={baseMint}
                onChange={(event) => setBaseMint(event.target.value)}
                placeholder="SPL Token mint address"
                autoComplete="off"
              />
            </label>
            <label>
              Quote mint
              <input
                value={quoteMint}
                onChange={(event) => setQuoteMint(event.target.value)}
                placeholder="SPL Token mint address"
                autoComplete="off"
              />
            </label>
            <label>
              Price tick size
              <input
                inputMode="numeric"
                value={priceTickSize}
                onChange={(event) => setPriceTickSize(event.target.value)}
                placeholder="10000"
                autoComplete="off"
                required
              />
            </label>

            <label>
              Quantity lot size
              <input
                inputMode="numeric"
                value={quantityLotSize}
                onChange={(event) => setQuantityLotSize(event.target.value)}
                placeholder="100000"
                autoComplete="off"
                required
              />
            </label>
            <button
              className="primary-button"
              type="submit"
              disabled={
                !baseMint.trim() ||
                !quoteMint.trim() ||
                !priceTickSize.trim() ||
                !quantityLotSize.trim() ||
                pending
              }
            >
              {pending ? "Creating market…" : "Create market"}
            </button>
          </form>
        )}

        {(roleError || error) && (
          <div className="transaction-message transaction-error">
            {roleError || error}
          </div>
        )}
        {result && (
          <div className="transaction-message transaction-success">
            <strong>Market created:</strong> {result.market.toBase58()}{" "}
            <a href={transactionExplorerUrl(result.signature)} target="_blank" rel="noreferrer">
              View transaction ↗
            </a>
          </div>
        )}
      </main>
    </div>
  );
}
