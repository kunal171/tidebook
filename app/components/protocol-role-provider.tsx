"use client";

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import { useAnchorWallet, useConnection } from "@solana/wallet-adapter-react";
import {
  decodeAdminStatus,
  deriveAdminRecordPda,
  deriveProtocolConfigPda,
  getTidebookAccounts,
  getTidebookProgram,
  type ProtocolRole,
} from "../lib/tidebook";

interface ProtocolRoleContextValue {
  role: ProtocolRole;
  loading: boolean;
  error: string | null;
  refresh: () => Promise<void>;
}

const ProtocolRoleContext = createContext<ProtocolRoleContextValue | null>(null);

export function ProtocolRoleProvider({ children }: { children: ReactNode }) {
  const { connection } = useConnection();
  const wallet = useAnchorWallet();
  const [role, setRole] = useState<ProtocolRole>("disconnected");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (!wallet) {
      setRole("disconnected");
      setError(null);
      return;
    }

    setLoading(true);
    setError(null);

    try {
      const program = getTidebookProgram(connection, wallet);
      const accounts = getTidebookAccounts(program);
      const protocolConfig = await accounts.protocolConfig.fetchNullable(
        deriveProtocolConfigPda(),
      );

      if (!protocolConfig) {
        setRole("uninitialized");
        return;
      }

      const adminRecord = await accounts.adminRecord.fetchNullable(
        deriveAdminRecordPda(wallet.publicKey),
      );

      if (!adminRecord) {
        setRole("user");
        return;
      }

      if (decodeAdminStatus(adminRecord.status) === "disabled") {
        setRole("disabled");
        return;
      }

      setRole(
        protocolConfig.superAdmin.equals(wallet.publicKey)
          ? "super-admin"
          : "admin",
      );
    } catch (cause) {
      setRole("user");
      const message = cause instanceof Error ? cause.message : "Unknown RPC error";
      setError(`Unable to read protocol role: ${message}`);
    } finally {
      setLoading(false);
    }
  }, [connection, wallet]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const value = useMemo(
    () => ({ role, loading, error, refresh }),
    [role, loading, error, refresh],
  );

  return (
    <ProtocolRoleContext.Provider value={value}>
      {children}
    </ProtocolRoleContext.Provider>
  );
}

export function useProtocolRole() {
  const context = useContext(ProtocolRoleContext);

  if (!context) {
    throw new Error("useProtocolRole must be used inside ProtocolRoleProvider");
  }

  return context;
}
