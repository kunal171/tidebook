"use client";

import { useCallback, useEffect, useState } from "react";
import { useConnection, useWallet } from "@solana/wallet-adapter-react";
import { WalletMultiButton } from "@solana/wallet-adapter-react-ui";
import {
  NETWORK,
  PROGRAM_EXPLORER_URL,
  PROGRAM_ID,
} from "../lib/solana";

type ProgramStatus = "checking" | "deployed" | "not-deployed" | "unavailable";

const shortAddress = (address: string) =>
  `${address.slice(0, 5)}…${address.slice(-5)}`;

export function Dashboard() {
  const { connection } = useConnection();
  const { connected, publicKey } = useWallet();
  const [programStatus, setProgramStatus] =
    useState<ProgramStatus>("checking");

  const refreshProgramStatus = useCallback(async () => {
    setProgramStatus("checking");

    try {
      const account = await connection.getAccountInfo(PROGRAM_ID, "confirmed");
      setProgramStatus(account?.executable ? "deployed" : "not-deployed");
    } catch {
      setProgramStatus("unavailable");
    }
  }, [connection]);

  useEffect(() => {
    void refreshProgramStatus();
  }, [refreshProgramStatus]);

  const statusLabel = {
    checking: "Checking devnet",
    deployed: "Program live",
    "not-deployed": "Awaiting deployment",
    unavailable: "RPC unavailable",
  }[programStatus];

  return (
    <div className="app-shell">
      <header className="topbar">
        <a className="brand" href="#top" aria-label="Tidebook home">
          <span className="brand-mark" aria-hidden="true">
            <i />
            <i />
            <i />
          </span>
          <span>Tidebook</span>
        </a>

        <div className="topbar-actions">
          <span className="network-pill">
            <span className="network-dot" />
            {NETWORK}
          </span>
          <WalletMultiButton />
        </div>
      </header>

      <main id="top">
        <section className="hero">
          <div className="eyebrow">Solana order-book research</div>
          <h1>
            The order book,
            <span> one invariant at a time.</span>
          </h1>
          <p>
            Tidebook pairs a tested Anchor program with a transparent interface.
            Every backend milestone earns its place in the UI.
          </p>

          <div className="hero-meta">
            <a href={PROGRAM_EXPLORER_URL} target="_blank" rel="noreferrer">
              View program ↗
            </a>
            <span>{shortAddress(PROGRAM_ID.toBase58())}</span>
          </div>
        </section>

        <section className="status-grid" aria-label="Project status">
          <article className="status-card status-card-primary">
            <div className="card-label">Devnet program</div>
            <div className="status-line">
              <span className={`status-indicator status-${programStatus}`} />
              <strong>{statusLabel}</strong>
            </div>
            <button
              className="text-button"
              type="button"
              onClick={() => void refreshProgramStatus()}
              disabled={programStatus === "checking"}
            >
              Refresh status
            </button>
          </article>

          <article className="status-card">
            <div className="card-label">Current milestone</div>
            <strong>Validated markets</strong>
            <p>Distinct SPL mints, authority controls, and limit-order PDAs.</p>
          </article>

          <article className="status-card">
            <div className="card-label">Test coverage</div>
            <strong>7 passing flows</strong>
            <p>LiteSVM verifies market lifecycle and order behavior.</p>
          </article>
        </section>

        <section className="workspace">
          <div className="workspace-copy">
            <div className="eyebrow">Market workspace</div>
            <h2>Start with a trusted pair.</h2>
            <p>
              The program is live on devnet. Wiring this form to the generated
              Anchor IDL is the next client milestone; both addresses will need
              to be initialized SPL Token mints.
            </p>
          </div>

          <div className="market-panel">
            <label>
              Base mint
              <input placeholder="Base mint address" disabled />
            </label>
            <span className="pair-divider">/</span>
            <label>
              Quote mint
              <input placeholder="Quote mint address" disabled />
            </label>
            <button className="primary-button" type="button" disabled>
              {connected ? "Market creation coming next" : "Connect wallet to begin"}
            </button>
            {publicKey && (
              <span className="wallet-note">
                Connected as {shortAddress(publicKey.toBase58())}
              </span>
            )}
          </div>
        </section>
      </main>

      <footer>
        <span>Tidebook · research build</span>
        <span>Anchor + LiteSVM + React</span>
      </footer>
    </div>
  );
}
