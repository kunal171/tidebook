"use client";

import { useCallback, useEffect, useState } from "react";
import { BN } from "@anchor-lang/core";
import { useAnchorWallet } from "@solana/wallet-adapter-react";
import { PublicKey } from "@solana/web3.js";

import { formatAtomicAmount, transactionExplorerUrl } from "../lib/tidebook";

type FaucetConfiguration = {
  enabled: boolean;
  baseMint?: string;
  quoteMint?: string;
  baseClaimAtoms?: string;
  quoteClaimAtoms?: string;
  windowSeconds?: number;
};

type ServerTestFaucetProps = {
  baseMint: PublicKey;
  quoteMint: PublicKey;
  baseDecimals: number;
  quoteDecimals: number;
  onClaimed: () => Promise<void>;
  onPairDetected: (configured: boolean) => void;
};

function errorMessage(cause: unknown) {
  return cause instanceof Error ? cause.message : "Faucet request failed";
}

export function ServerTestFaucet({
  baseMint,
  quoteMint,
  baseDecimals,
  quoteDecimals,
  onClaimed,
  onPairDetected,
}: ServerTestFaucetProps) {
  const wallet = useAnchorWallet();
  const [configuration, setConfiguration] =
    useState<FaucetConfiguration | null>(null);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [signature, setSignature] = useState<string | null>(null);

  useEffect(() => {
    let active = true;

    void fetch("/api/faucet", { cache: "no-store" })
      .then(async (response) => {
        if (!response.ok) throw new Error("Faucet configuration unavailable");
        return (await response.json()) as FaucetConfiguration;
      })
      .then((value) => {
        if (active) {
          setConfiguration(value);
          onPairDetected(
            value.baseMint === baseMint.toBase58() &&
              value.quoteMint === quoteMint.toBase58(),
          );
        }
      })
      .catch((cause) => {
        if (active) {
          setError(errorMessage(cause));
          onPairDetected(false);
        }
      });

    return () => {
      active = false;
    };
  }, [baseMint, onPairDetected, quoteMint]);

  const claim = useCallback(async () => {
    if (!wallet) return;

    setPending(true);
    setError(null);
    setSignature(null);
    try {
      const response = await fetch("/api/faucet", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ wallet: wallet.publicKey.toBase58() }),
      });
      const result = (await response.json()) as {
        error?: string;
        signature?: string;
        retryAfter?: number;
      };

      if (!response.ok || !result.signature) {
        const retry = result.retryAfter
          ? ` Try again in ${Math.ceil(result.retryAfter / 60)} minute(s).`
          : "";
        throw new Error(`${result.error ?? "Faucet request failed"}${retry}`);
      }

      setSignature(result.signature);
      await onClaimed();
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setPending(false);
    }
  }, [onClaimed, wallet]);

  const configuredForMarket =
    configuration?.baseMint === baseMint.toBase58() &&
    configuration.quoteMint === quoteMint.toBase58();

  if (
    !configuredForMarket ||
    !configuration.baseClaimAtoms ||
    !configuration.quoteClaimAtoms
  ) {
    return null;
  }

  const enabled = configuration.enabled;

  return (
    <section className="admin-card market-balance-card">
      <div className="card-label">Rate-limited devnet faucet</div>
      <h2>Get test assets</h2>
      <p>
        Each request mints{" "}
        {formatAtomicAmount(new BN(configuration.baseClaimAtoms), baseDecimals)}{" "}
        base and{" "}
        {formatAtomicAmount(
          new BN(configuration.quoteClaimAtoms),
          quoteDecimals,
        )}{" "}
        quote tokens. These devnet assets have no monetary value.
      </p>
      {!enabled && (
        <div className="transaction-message transaction-error">
          Faucet temporarily unavailable: durable rate limiting is not
          configured.
        </div>
      )}
      <button
        className="primary-button"
        type="button"
        disabled={!enabled || !wallet || pending}
        onClick={() => void claim()}
      >
        {pending
          ? "Minting…"
          : wallet
            ? "Claim test tokens"
            : "Connect wallet to claim"}
      </button>
      {error && (
        <div className="transaction-message transaction-error">{error}</div>
      )}
      {signature && (
        <div className="transaction-message transaction-success">
          Test assets minted.{" "}
          <a
            href={transactionExplorerUrl(signature)}
            target="_blank"
            rel="noreferrer"
          >
            View transaction ↗
          </a>
        </div>
      )}
    </section>
  );
}
