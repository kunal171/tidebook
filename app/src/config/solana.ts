import { clusterApiUrl, PublicKey } from "@solana/web3.js";

export const NETWORK = "devnet" as const;
export const RPC_ENDPOINT =
  import.meta.env.VITE_SOLANA_RPC_URL ?? clusterApiUrl(NETWORK);

export const PROGRAM_ID = new PublicKey(
  "Honq7kkNfptR6XF5H4zn2jWqSmNRsteCpGwB8iG393cR",
);

export const PROGRAM_EXPLORER_URL = `https://explorer.solana.com/address/${PROGRAM_ID.toBase58()}?cluster=${NETWORK}`;
