"use client";

import { useMemo } from "react";
import { ConnectionProvider, WalletProvider } from "@solana/wallet-adapter-react";
import { WalletModalProvider } from "@solana/wallet-adapter-react-ui";
import { RPC_ENDPOINT } from "../lib/solana";
import { ProtocolRoleProvider } from "./protocol-role-provider";

export function SolanaProvider({ children }: { children: React.ReactNode }) {
  const wallets = useMemo(() => [], []);

  return (
    <ConnectionProvider endpoint={RPC_ENDPOINT}>
      <WalletProvider wallets={wallets} autoConnect>
        <WalletModalProvider>
          <ProtocolRoleProvider>{children}</ProtocolRoleProvider>
        </WalletModalProvider>
      </WalletProvider>
    </ConnectionProvider>
  );
}
