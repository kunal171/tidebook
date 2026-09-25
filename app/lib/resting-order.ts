import { BN, Program } from "@anchor-lang/core";
import {
  PublicKey,
  SystemProgram,
  TransactionInstruction,
} from "@solana/web3.js";

import {
  deriveOrderPda,
  derivePriceLevelPda,
  findPriceLevelNeighbors,
  getTidebookAccounts,
  type MarketAccount,
  type OrderSide,
} from "./tidebook";

export interface RestingOrderInstruction {
  instruction: TransactionInstruction;
  order: PublicKey;
}

/**
 * Builds, but does not submit, an instruction that places a resting order.
 *
 * Matching only modifies the opposing side of the book and does not increment
 * `nextOrderId`. Therefore, this instruction can safely follow bounded matching
 * instructions in the same atomic transaction. The program still validates all
 * supplied price-level and FIFO relationships on-chain.
 */
export async function buildRestingOrderInstruction(
  program: Program,
  trader: PublicKey,
  marketAddress: PublicKey,
  market: MarketAccount,
  traderBalance: PublicKey,
  side: OrderSide,
  price: BN,
  quantity: BN,
): Promise<RestingOrderInstruction> {
  const orderSide = side === "bid" ? { bid: {} } : { ask: {} };
  const order = deriveOrderPda(marketAddress, market.nextOrderId);
  const priceLevel = derivePriceLevelPda(marketAddress, side, price);

  const priceLevelState = await getTidebookAccounts(
    program,
  ).priceLevel.fetchNullable(priceLevel);

  if (priceLevelState) {
    if (!priceLevelState.lastOrder) {
      throw new Error("Existing price level has no FIFO tail");
    }

    const instruction = await program.methods
      .appendLimitOrder(orderSide, price, quantity)
      .accounts({
        trader,
        market: marketAddress,
        order,
        priceLevel,
        previousOrder: priceLevelState.lastOrder,
        traderBalance,
        systemProgram: SystemProgram.programId,
      })
      .instruction();

    return { instruction, order };
  }

  const currentBest =
    side === "bid" ? market.bestBid : market.bestAsk;

  const { betterLevel, worseLevel } = await findPriceLevelNeighbors(
    program,
    marketAddress,
    side,
    price,
    currentBest,
  );

  const instruction = await program.methods
    .insertLimitOrder(orderSide, price, quantity)
    .accountsPartial({
      trader,
      market: marketAddress,
      order,
      priceLevel,
      traderBalance,
      systemProgram: SystemProgram.programId,
      betterLevel: betterLevel ?? program.programId,
      worseLevel: worseLevel ?? program.programId,
    })
    .instruction();

  return { instruction, order };
}