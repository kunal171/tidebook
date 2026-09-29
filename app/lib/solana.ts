import { clusterApiUrl, PublicKey } from "@solana/web3.js";

export const NETWORK = "devnet" as const;
export const RPC_ENDPOINT =
  process.env.NEXT_PUBLIC_SOLANA_RPC_URL ?? clusterApiUrl(NETWORK);

const configuredProgramId = process.env.NEXT_PUBLIC_TIDEBOOK_PROGRAM_ID;

if (!configuredProgramId) {
  throw new Error("NEXT_PUBLIC_TIDEBOOK_PROGRAM_ID is not configured");
}

export const PROGRAM_ID = new PublicKey(configuredProgramId);

export const PROGRAM_EXPLORER_URL = `https://explorer.solana.com/address/${PROGRAM_ID.toBase58()}?cluster=${NETWORK}`;
