import { BN, Program } from "@anchor-lang/core";
import { PublicKey } from "@solana/web3.js";

import {
  decodeOrderSide,
  decodeOrderStatus,
  derivePriceLevelPda,
  deriveTraderBalancePda,
  getTidebookAccounts,
  type MarketAccount,
  type OrderSide,
} from "./tidebook";

// Keep the batch deliberately small. Each match step introduces writable
// maker, order, and price-level accounts, so an unbounded plan would eventually
// exceed Solana's transaction account or compute limits.
export const MAX_MATCHES_PER_TRANSACTION = 3;

export interface MatchStep {
  makerOrder: PublicKey;
  makerPriceLevel: PublicKey;
  makerBalance: PublicKey;
  // Settlement uses the resting maker's price. Keep it in the plan so the
  // caller can calculate the exact quote amount across multiple price levels.
  makerPrice: BN;

  nextOrder: PublicKey | null;
  worseLevel: PublicKey | null;
  levelRentRecipient: PublicKey | null;

  takerQuantityBeforeFill: BN;
  fillQuantity: BN;
}

export function pricesCross(
  takerSide: OrderSide,
  takerLimitPrice: BN,
  makerPrice: BN,
) {
  return takerSide === "bid"
    ? takerLimitPrice.gte(makerPrice)
    : takerLimitPrice.lte(makerPrice);
}

export type MatchPlanStopReason =
  | "filled"
  | "book-exhausted"
  | "non-crossing"
  | "match-cap";

/**
 * A read-only snapshot used to construct one atomic matching transaction.
 *
 * `book-exhausted` and `non-crossing` prove that an unfilled remainder may be
 * posted at the taker's limit price. `match-cap` does not: crossing liquidity
 * still exists and must be processed by another bounded transaction first.
 * Every relationship in this plan remains untrusted until the program
 * revalidates it on-chain.
 */
export interface BoundedMatchPlan {
  steps: MatchStep[];
  remainingQuantity: BN;
  stopReason: MatchPlanStopReason;
}

export async function planBoundedMatches(
  program: Program,
  marketAddress: PublicKey,
  market: MarketAccount,
  taker: PublicKey,
  takerSide: OrderSide,
  limitPrice: BN,
  quantity: BN,
): Promise<BoundedMatchPlan> {
  if (quantity.isZero()) {
    throw new Error("Matching quantity must be greater than zero");
  }

  // If traversal naturally runs out of opposing levels, the book is
  // exhausted. More specific exit paths replace this default below.
  let stopReason: MatchPlanStopReason = "book-exhausted";
  const accounts = getTidebookAccounts(program);
  const makerSide: OrderSide = takerSide === "bid" ? "ask" : "bid";
  const steps: MatchStep[] = [];

  const visitedLevels = new Set<string>();
  const visitedOrders = new Set<string>();

  let remainingQuantity = quantity;
  let currentPrice =
    takerSide === "bid" ? market.bestAsk : market.bestBid;

  // When moving to a worse level, its stored better link still points to the
  // level that an earlier instruction in this transaction will remove.
  let expectedBetterPrice: BN | null = null;

  while (
    currentPrice &&
    !remainingQuantity.isZero() &&
    steps.length < MAX_MATCHES_PER_TRANSACTION
  ) {
    if (!pricesCross(takerSide, limitPrice, currentPrice)) {
      stopReason = "non-crossing";
      break;
    }

    const levelAddress = derivePriceLevelPda(
      marketAddress,
      makerSide,
      currentPrice,
    );
    const levelKey = levelAddress.toBase58();

    if (visitedLevels.has(levelKey)) {
      throw new Error("Price-level index contains a cycle");
    }
    visitedLevels.add(levelKey);

    const level = await accounts.priceLevel.fetchNullable(levelAddress);
    if (!level) {
      throw new Error(`Missing price level ${levelKey}`);
    }

    const levelSide: OrderSide =
      "bid" in level.side ? "bid" : "ask";

    if (!level.market.equals(marketAddress) || levelSide !== makerSide) {
      throw new Error("Price level crosses a market or side boundary");
    }

    if (!level.price.eq(currentPrice)) {
      throw new Error("Price-level PDA and stored price disagree");
    }

    const betterLinkMatches = expectedBetterPrice
      ? level.betterPrice?.eq(expectedBetterPrice) === true
      : level.betterPrice === null;

    if (!betterLinkMatches) {
      throw new Error("Price level has a broken better-price link");
    }

    if (expectedBetterPrice) {
      const correctlyOrdered =
        makerSide === "bid"
          ? expectedBetterPrice.gt(level.price)
          : expectedBetterPrice.lt(level.price);

      if (!correctlyOrdered) {
        throw new Error("Price levels are not strictly ordered");
      }
    }

    if (
      !level.firstOrder ||
      !level.lastOrder ||
      level.orderCount.isZero() ||
      level.totalRemainingQuantity.isZero()
    ) {
      throw new Error("Active price level has invalid queue state");
    }

    let currentOrder: PublicKey | null = level.firstOrder;
    let expectedPrevious: PublicKey | null = null;
    let localOrderCount = level.orderCount;
    let localLevelQuantity = level.totalRemainingQuantity;
    let advanceToWorseLevel = false;

    while (
      currentOrder &&
      !remainingQuantity.isZero() &&
      steps.length < MAX_MATCHES_PER_TRANSACTION
    ) {
      const orderKey = currentOrder.toBase58();

      if (visitedOrders.has(orderKey)) {
        throw new Error("FIFO order queue contains a cycle");
      }
      visitedOrders.add(orderKey);

      const maker = await accounts.order.fetchNullable(currentOrder);
      if (!maker) {
        throw new Error(`Missing maker order ${orderKey}`);
      }

      const previousLinkMatches = expectedPrevious
        ? maker.previousOrder?.equals(expectedPrevious) === true
        : maker.previousOrder === null;

      if (
        !maker.market.equals(marketAddress) ||
        !maker.priceLevel.equals(levelAddress) ||
        decodeOrderSide(maker.side) !== makerSide ||
        !maker.price.eq(level.price) ||
        decodeOrderStatus(maker.status) !== "open" ||
        !previousLinkMatches ||
        maker.remainingQuantity.isZero()
      ) {
        throw new Error("Maker order violates FIFO or market invariants");
      }

      if (maker.owner.equals(taker)) {
        // Price-time priority does not permit silently skipping a self-order.
        throw new Error("Self-trading blocks the current matching path");
      }

      const fillQuantity = BN.min(
        remainingQuantity,
        maker.remainingQuantity,
      );
      const makerFullyFilled = fillQuantity.eq(
        maker.remainingQuantity,
      );

      if (localLevelQuantity.lt(fillQuantity)) {
        throw new Error("Price-level aggregate is smaller than the fill");
      }

      const nextLevelQuantity =
        localLevelQuantity.sub(fillQuantity);
      const nextOrderCount = makerFullyFilled
        ? localOrderCount.subn(1)
        : localOrderCount;
      const removesPriceLevel =
        makerFullyFilled && nextOrderCount.isZero();

      if (makerFullyFilled && removesPriceLevel) {
        if (
          maker.nextOrder !== null ||
          !level.lastOrder.equals(currentOrder) ||
          !nextLevelQuantity.isZero()
        ) {
          throw new Error("Final maker does not match level endpoints");
        }
      } else if (makerFullyFilled && !maker.nextOrder) {
        throw new Error("Filled FIFO maker has no successor");
      }

      steps.push({
        makerOrder: currentOrder,
        makerPriceLevel: levelAddress,
        makerBalance: deriveTraderBalancePda(
          marketAddress,
          maker.owner,
        ),
        makerPrice: maker.price,

        nextOrder:
          makerFullyFilled && !removesPriceLevel
            ? maker.nextOrder
            : null,

        worseLevel:
          removesPriceLevel && level.worsePrice
            ? derivePriceLevelPda(
                marketAddress,
                makerSide,
                level.worsePrice,
              )
            : null,

        levelRentRecipient: removesPriceLevel
          ? level.rentPayer
          : null,

        // Each instruction receives the taker quantity remaining before its
        // own fill, not the original submitted quantity.
        takerQuantityBeforeFill: remainingQuantity,
        fillQuantity,
      });

      remainingQuantity = remainingQuantity.sub(fillQuantity);
      localLevelQuantity = nextLevelQuantity;
      localOrderCount = nextOrderCount;

      if (remainingQuantity.isZero()) {
        stopReason = "filled";
        break;
      }

      if (steps.length === MAX_MATCHES_PER_TRANSACTION) {
        // Reaching the cap is not automatically unsafe for remainder posting.
        // The capped step may also have consumed the final crossing maker. We
        // can classify that boundary from the current maker and level links
        // without fetching a fourth order.
        if (maker.nextOrder) {
          // Another FIFO order exists at the same crossing price.
          stopReason = "match-cap";
        } else if (!level.worsePrice) {
          // The final opposing price level was completely consumed.
          stopReason = "book-exhausted";
        } else if (pricesCross(takerSide, limitPrice, level.worsePrice)) {
          // The next price level still crosses, but the transaction is full.
          stopReason = "match-cap";
        } else {
          stopReason = "non-crossing";
        }

        break;
      }

      if (!makerFullyFilled) {
        // A partial maker consumes all remaining taker quantity.
        throw new Error("Partial maker left an unexpected taker remainder");
      }

      if (maker.nextOrder) {
        // Validate the pre-transaction reciprocal link on the next loop. The
        // preceding instruction will clear it when promoting this order.
        expectedPrevious = currentOrder;
        currentOrder = maker.nextOrder;
      } else {
        expectedBetterPrice = level.price;
        currentPrice = level.worsePrice;
        advanceToWorseLevel = true;
        break;
      }
    }

    if (
      remainingQuantity.isZero() ||
      steps.length === MAX_MATCHES_PER_TRANSACTION
    ) {
      break;
    }

    if (!advanceToWorseLevel) {
      break;
    }
  }

  return {
    steps,
    remainingQuantity,
    stopReason,
  };
}
