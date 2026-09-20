"use client";

import { useEffect, useState } from "react";
import Link from "next/link";
import { WalletMultiButton } from "@solana/wallet-adapter-react-ui";
import { NETWORK } from "../lib/solana";
import { useProtocolRole } from "./protocol-role-provider";

const roleLabels = {
  disconnected: null,
  uninitialized: "Setup required",
  "super-admin": "Super admin",
  admin: "Admin",
  disabled: "Admin disabled",
  user: null,
};

function HydrationSafeWalletButton() {
  const [mounted, setMounted] = useState(false);

  useEffect(() => {
    setMounted(true);
  }, []);

  if (!mounted) {
    return (
      <button
        className="wallet-adapter-button wallet-adapter-button-trigger"
        type="button"
        disabled
        aria-label="Loading wallet"
      >
        Select Wallet
      </button>
    );
  }

  return <WalletMultiButton />;
}

export function AppHeader() {
  const { role, loading } = useProtocolRole();
  const roleLabel = loading ? "Checking role" : roleLabels[role];
  const isSuperAdmin = !loading && role === "super-admin";
  const canCreateMarket = !loading && (role === "super-admin" || role === "admin");
  const canViewOrders = !loading && role !== "disconnected";

  return (
    <header className="topbar">
      <Link className="brand" href="/" aria-label="Tidebook home">
        <span className="brand-mark" aria-hidden="true">
          <i />
          <i />
          <i />
        </span>
        <span>Tidebook</span>
      </Link>

      <nav className="topbar-nav" aria-label="Primary navigation">
        <Link href="/markets">Markets</Link>
        {canViewOrders && <Link href="/orders">Orders</Link>}
        {isSuperAdmin && <Link href="/admin">Manage admins</Link>}
        {canCreateMarket && <Link href="/markets/new">Create market</Link>}
      </nav>

      <div className="topbar-actions">
        {roleLabel && <span className={`role-badge role-${role}`}>{roleLabel}</span>}
        <span className="network-pill">
          <span className="network-dot" />
          {NETWORK}
        </span>
        <HydrationSafeWalletButton />
      </div>
    </header>
  );
}
