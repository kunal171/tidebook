import { clusterApiUrl, PublicKey } from "@solana/web3.js";

export const NETWORK = "devnet" as const;
export const RPC_ENDPOINT =
  process.env.NEXT_PUBLIC_SOLANA_RPC_URL ?? clusterApiUrl(NETWORK);

export const PROGRAM_ID = new PublicKey(
  "BPdNF5CnV8z1EkHo7tcueR6wXmzZV2j6j4wsUTirPgWL",
);

export const PROGRAM_EXPLORER_URL = `https://explorer.solana.com/address/${PROGRAM_ID.toBase58()}?cluster=${NETWORK}`;
