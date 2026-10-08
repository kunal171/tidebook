"use client";

import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  type FormEvent,
} from "react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { AnchorProvider, BN } from "@anchor-lang/core";
import { useAnchorWallet, useConnection } from "@solana/wallet-adapter-react";
import { PublicKey, SystemProgram, Transaction } from "@solana/web3.js";
import {
  createAssociatedTokenAccountInstruction,
  getAssociatedTokenAddressSync,
} from "@solana/spl-token";
import {
  accountExplorerUrl,
  decodeMarketStatus,
  deriveMarketFeesPda,
  deriveTestFaucetAuthorityPda,
  deriveTestFaucetPda,
  deriveProtocolConfigPda,
  deriveTraderBalancePda,
  deriveVaultAuthorityPda,
  formatAtomicAmount,
  getOwnedTokenBalance,
  findOwnedTokenAccount,
  fetchLatestTrade,
  fetchPriceLevelSide,
  getTidebookAccounts,
  getTidebookProgram,
  getTidebookReadProgram,
  parseTokenAmount,
  transactionExplorerUrl,
  deriveVaultPda,
  TOKEN_PROGRAM_ID,
  type MarketAccount,
  type MarketFeesAccount,
  type LastTradeView,
  type PriceLevelView,
  type TestFaucetAccount,
  type OrderSide,
  type TraderBalanceAccount,
} from "../lib/tidebook";
import {
  MAX_MATCHES_PER_TRANSACTION,
  planBoundedMatches,
} from "../lib/matching-plan";
import { AppHeader } from "./app-header";
import { buildRestingOrderInstruction } from "../lib/resting-order";
import { useProtocolRole } from "./protocol-role-provider";
import { ServerTestFaucet } from "./server-test-faucet";

function shortAddress(value: string) {
  return `${value.slice(0, 8)}…${value.slice(-8)}`;
}

function getErrorMessage(cause: unknown) {
  return cause instanceof Error ? cause.message : "Transaction failed";
}

function parsePositiveU64(value: string, label: string) {
  const normalized = value.trim();

  if (!/^[0-9]+$/.test(normalized)) {
    throw new Error(`${label} must be a positive whole number`);
  }

  const number = new BN(normalized, 10);
  if (number.isZero()) {
    throw new Error(`${label} must be greater than zero`);
  }
  if (number.bitLength() > 64) {
    throw new Error(`${label} exceeds the u64 limit`);
  }

  return number;
}

type SubmissionResult =
  | {
      kind: "placed";
      signature: string;
      order: PublicKey;
    }
  | {
      kind: "matched";
      signature: string;
      makerCount: number;
      filledQuantity: BN;
      remainingQuantity: BN;
      postedOrder: PublicKey | null;
    };

export function MarketDetail({ address }: { address: string }) {
  const router = useRouter();
  const { connection } = useConnection();
  const wallet = useAnchorWallet();
  const { role } = useProtocolRole();
  const [market, setMarket] = useState<MarketAccount | null>(null);
  const [marketFees, setMarketFees] = useState<MarketFeesAccount | null>(null);
  const [bidLevels, setBidLevels] = useState<PriceLevelView[]>([]);
  const [askLevels, setAskLevels] = useState<PriceLevelView[]>([]);
  const [lastTrade, setLastTrade] = useState<LastTradeView | null>(null);
  const [bookError, setBookError] = useState<string | null>(null);
  const [testFaucet, setTestFaucet] = useState<TestFaucetAccount | null>(null);
  const [serverFaucetPair, setServerFaucetPair] = useState<boolean | null>(
    null,
  );
  const [walletBaseBalance, setWalletBaseBalance] = useState(() => new BN(0));
  const [walletQuoteBalance, setWalletQuoteBalance] = useState(() => new BN(0));
  const [faucetPending, setFaucetPending] = useState<
    "initialize" | "claim" | null
  >(null);
  const [faucetError, setFaucetError] = useState<string | null>(null);
  const [faucetSignature, setFaucetSignature] = useState<string | null>(null);
  const [baseClaimAmount, setBaseClaimAmount] = useState("10");
  const [quoteClaimAmount, setQuoteClaimAmount] = useState("10000");
  const [loading, setLoading] = useState(true);
  const [side, setSide] = useState<OrderSide>("bid");
  const [price, setPrice] = useState("");
  const [quantity, setQuantity] = useState("");
  const [pending, setPending] = useState(false);
  const [traderBalance, setTraderBalance] =
    useState<TraderBalanceAccount | null>(null);
  const [balanceLoading, setBalanceLoading] = useState(false);
  const [balanceAsset, setBalanceAsset] = useState<"base" | "quote">("base");
  const [balanceAmount, setBalanceAmount] = useState("");
  const [balancePending, setBalancePending] = useState<
    "initialize" | "deposit" | "withdraw" | null
  >(null);
  const [balanceError, setBalanceError] = useState<string | null>(null);
  const [balanceSignature, setBalanceSignature] = useState<string | null>(null);
  const [lifecyclePending, setLifecyclePending] = useState<
    "pause" | "unpause" | "close" | null
  >(null);
  const [lifecycleSignature, setLifecycleSignature] = useState<string | null>(
    null,
  );
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<SubmissionResult | null>(null);
  const [feeDestination, setFeeDestination] = useState("");
  const [feeAmount, setFeeAmount] = useState("");
  const [feePending, setFeePending] = useState(false);
  const [feeError, setFeeError] = useState<string | null>(null);
  const [feeSignature, setFeeSignature] = useState<string | null>(null);
  const marketAddress = useMemo(() => {
    try {
      return new PublicKey(address);
    } catch {
      return null;
    }
  }, [address]);

  const readProgram = useMemo(
    () => getTidebookReadProgram(connection),
    [connection],
  );
  const signedProgram = useMemo(
    () => (wallet ? getTidebookProgram(connection, wallet) : null),
    [connection, wallet],
  );
  const traderBalanceAddress = useMemo(
    () =>
      marketAddress && wallet
        ? deriveTraderBalancePda(marketAddress, wallet.publicKey)
        : null,
    [marketAddress, wallet],
  );
  // Vault addresses are deterministic children of the market and mint, so the
  // UI can link to them without storing extra addresses in the Market account.
  const baseVault =
    market && marketAddress
      ? deriveVaultPda(marketAddress, market.baseMint)
      : null;

  const quoteVault =
    market && marketAddress
      ? deriveVaultPda(marketAddress, market.quoteMint)
      : null;

  const loadMarket = useCallback(async () => {
    if (!marketAddress) {
      setMarket(null);
      setError("Invalid market address");
      setLoading(false);
      return;
    }

    setLoading(true);
    setError(null);

    try {
      const accounts = getTidebookAccounts(readProgram);
      const [account, fees, faucet] = await Promise.all([
        accounts.market.fetchNullable(marketAddress),
        accounts.marketFees.fetchNullable(deriveMarketFeesPda(marketAddress)),
        accounts.testFaucet.fetchNullable(deriveTestFaucetPda(marketAddress)),
      ]);
      setMarket(account);
      setMarketFees(fees);
      setTestFaucet(faucet);

      if (!account) {
        setBidLevels([]);
        setAskLevels([]);
        setLastTrade(null);
        setError("Market account was not found on devnet");
        return;
      }

      // The book is canonical on-chain state. Trade history currently lives in
      // FillEvent logs, so failure to fetch recent history must not hide the
      // usable market or its order book.
      setBookError(null);
      try {
        const [bids, asks] = await Promise.all([
          fetchPriceLevelSide(
            readProgram,
            marketAddress,
            "bid",
            account.bestBid,
          ),
          fetchPriceLevelSide(
            readProgram,
            marketAddress,
            "ask",
            account.bestAsk,
          ),
        ]);
        setBidLevels(bids);
        setAskLevels(asks);
      } catch (cause) {
        setBidLevels([]);
        setAskLevels([]);
        setBookError(getErrorMessage(cause));
      }

      const recentTrade = await fetchLatestTrade(
        connection,
        readProgram,
        marketAddress,
      ).catch(() => null);
      setLastTrade(recentTrade);
    } catch (cause) {
      setError(getErrorMessage(cause));
    } finally {
      setLoading(false);
    }
  }, [connection, marketAddress, readProgram]);

  useEffect(() => {
    void loadMarket();
  }, [loadMarket]);

  const loadBalances = useCallback(async () => {
    if (!traderBalanceAddress || !wallet || !market) {
      setTraderBalance(null);
      setWalletBaseBalance(new BN(0));
      setWalletQuoteBalance(new BN(0));
      return;
    }

    setBalanceLoading(true);
    setBalanceError(null);
    try {
      const [account, baseBalance, quoteBalance] = await Promise.all([
        getTidebookAccounts(readProgram).traderBalance.fetchNullable(
          traderBalanceAddress,
        ),
        getOwnedTokenBalance(connection, wallet.publicKey, market.baseMint),
        getOwnedTokenBalance(connection, wallet.publicKey, market.quoteMint),
      ]);
      setTraderBalance(account);
      setWalletBaseBalance(baseBalance);
      setWalletQuoteBalance(quoteBalance);
    } catch (cause) {
      setBalanceError(getErrorMessage(cause));
    } finally {
      setBalanceLoading(false);
    }
  }, [connection, market, readProgram, traderBalanceAddress, wallet]);

  useEffect(() => {
    void loadBalances();
  }, [loadBalances]);

  const initializeBalance = async () => {
    if (!signedProgram || !wallet || !marketAddress || !traderBalanceAddress) {
      return;
    }

    setBalancePending("initialize");
    setBalanceError(null);
    setBalanceSignature(null);
    try {
      const signature = await signedProgram.methods
        .initializeTraderBalance()
        .accounts({
          owner: wallet.publicKey,
          market: marketAddress,
          traderBalance: traderBalanceAddress,
          systemProgram: SystemProgram.programId,
        })
        .rpc();
      setBalanceSignature(signature);
      await loadBalances();
    } catch (cause) {
      setBalanceError(getErrorMessage(cause));
    } finally {
      setBalancePending(null);
    }
  };

  const transferBalance = async (action: "deposit" | "withdraw") => {
    if (
      !signedProgram ||
      !wallet ||
      !marketAddress ||
      !market ||
      !traderBalanceAddress ||
      !traderBalance
    ) {
      return;
    }

    setBalancePending(action);
    setBalanceError(null);
    setBalanceSignature(null);
    try {
      const amount = parseTokenAmount(
        balanceAmount,
        balanceAsset === "base" ? market.baseDecimals : market.quoteDecimals,
        "Amount",
      );
      const freeBalance =
        balanceAsset === "base"
          ? traderBalance.baseFree
          : traderBalance.quoteFree;
      if (action === "withdraw" && amount.gt(freeBalance)) {
        throw new Error(
          "Withdrawal exceeds the selected asset's free internal balance",
        );
      }
      const mint = balanceAsset === "base" ? market.baseMint : market.quoteMint;
      const tokenAccount = await findOwnedTokenAccount(
        connection,
        wallet.publicKey,
        mint,
        action === "deposit" ? amount : new BN(0),
      );
      const vaultAuthority = deriveVaultAuthorityPda(marketAddress);
      const marketVault = deriveVaultPda(marketAddress, mint);

      const signature =
        action === "deposit"
          ? await signedProgram.methods
              .deposit(amount)
              .accounts({
                owner: wallet.publicKey,
                market: marketAddress,
                traderBalance: traderBalanceAddress,
                depositMint: mint,
                traderTokenAccount: tokenAccount,
                vaultAuthority,
                marketVault,
                tokenProgram: TOKEN_PROGRAM_ID,
              })
              .rpc()
          : await signedProgram.methods
              .withdraw(amount)
              .accounts({
                owner: wallet.publicKey,
                market: marketAddress,
                traderBalance: traderBalanceAddress,
                withdrawalMint: mint,
                ownerTokenAccount: tokenAccount,
                vaultAuthority,
                marketVault,
                tokenProgram: TOKEN_PROGRAM_ID,
              })
              .rpc();

      setBalanceAmount("");
      setBalanceSignature(signature);
      await loadBalances();
    } catch (cause) {
      setBalanceError(getErrorMessage(cause));
    } finally {
      setBalancePending(null);
    }
  };

  const initializeTestFaucet = async () => {
    if (
      !signedProgram ||
      !wallet ||
      !marketAddress ||
      !market ||
      role !== "super-admin"
    ) {
      return;
    }

    setFaucetPending("initialize");
    setFaucetError(null);
    setFaucetSignature(null);
    try {
      const baseAmount = parseTokenAmount(
        baseClaimAmount,
        market.baseDecimals,
        "Base claim amount",
      );
      const quoteAmount = parseTokenAmount(
        quoteClaimAmount,
        market.quoteDecimals,
        "Quote claim amount",
      );
      const testFaucetAddress = deriveTestFaucetPda(marketAddress);
      const signature = await signedProgram.methods
        .initializeTestFaucet(baseAmount, quoteAmount)
        .accounts({
          superAdmin: wallet.publicKey,
          protocolConfig: deriveProtocolConfigPda(),
          market: marketAddress,
          testFaucet: testFaucetAddress,
          faucetAuthority: deriveTestFaucetAuthorityPda(testFaucetAddress),
          baseMint: market.baseMint,
          quoteMint: market.quoteMint,
          tokenProgram: TOKEN_PROGRAM_ID,
          systemProgram: SystemProgram.programId,
        })
        .rpc();

      setFaucetSignature(signature);
      await loadMarket();
    } catch (cause) {
      setFaucetError(getErrorMessage(cause));
    } finally {
      setFaucetPending(null);
    }
  };

  const claimTestTokens = async () => {
    if (!signedProgram || !wallet || !marketAddress || !market || !testFaucet) {
      return;
    }

    setFaucetPending("claim");
    setFaucetError(null);
    setFaucetSignature(null);
    try {
      const claimantBaseAccount = getAssociatedTokenAddressSync(
        market.baseMint,
        wallet.publicKey,
      );
      const claimantQuoteAccount = getAssociatedTokenAddressSync(
        market.quoteMint,
        wallet.publicKey,
      );
      const [baseAccountInfo, quoteAccountInfo] =
        await connection.getMultipleAccountsInfo(
          [claimantBaseAccount, claimantQuoteAccount],
          "confirmed",
        );

      const transaction = new Transaction();
      if (!baseAccountInfo) {
        transaction.add(
          createAssociatedTokenAccountInstruction(
            wallet.publicKey,
            claimantBaseAccount,
            wallet.publicKey,
            market.baseMint,
          ),
        );
      }
      if (!quoteAccountInfo) {
        transaction.add(
          createAssociatedTokenAccountInstruction(
            wallet.publicKey,
            claimantQuoteAccount,
            wallet.publicKey,
            market.quoteMint,
          ),
        );
      }

      const testFaucetAddress = deriveTestFaucetPda(marketAddress);
      transaction.add(
        await signedProgram.methods
          .claimTestTokens()
          .accounts({
            claimant: wallet.publicKey,
            market: marketAddress,
            testFaucet: testFaucetAddress,
            faucetAuthority: deriveTestFaucetAuthorityPda(testFaucetAddress),
            baseMint: market.baseMint,
            quoteMint: market.quoteMint,
            claimantBaseAccount,
            claimantQuoteAccount,
            tokenProgram: TOKEN_PROGRAM_ID,
          })
          .instruction(),
      );

      const provider = signedProgram.provider as AnchorProvider;
      const signature = await provider.sendAndConfirm(transaction);
      setFaucetSignature(signature);
      await loadBalances();
    } catch (cause) {
      setFaucetError(getErrorMessage(cause));
    } finally {
      setFaucetPending(null);
    }
  };

  const placeOrder = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();

    if (
      !signedProgram ||
      !wallet ||
      !marketAddress ||
      !market ||
      !traderBalanceAddress
    ) {
      return;
    }

    setPending(true);
    setError(null);
    setResult(null);

    try {
      if (decodeMarketStatus(market.status) !== "active") {
        throw new Error("This market is paused and cannot accept new orders");
      }
      if (!traderBalance) {
        throw new Error("Initialize and fund your market balance first");
      }

      const rawPrice = parseTokenAmount(price, market.quoteDecimals, "Price");
      if (!rawPrice.mod(market.priceTickSize).isZero()) {
        throw new Error(
          `Price must align to the ${formatAtomicAmount(market.priceTickSize, market.quoteDecimals)} tick`,
        );
      }

      const rawQuantity = parseTokenAmount(
        quantity,
        market.baseDecimals,
        "Quantity",
      );
      if (!rawQuantity.mod(market.quantityLotSize).isZero()) {
        throw new Error(
          `Quantity must align to the ${formatAtomicAmount(market.quantityLotSize, market.baseDecimals)} lot`,
        );
      }

      const orderSide = side === "bid" ? { bid: {} } : { ask: {} };
      const baseScale = new BN(10).pow(new BN(market.baseDecimals));
      const availableBalance =
        side === "bid" ? traderBalance.quoteFree : traderBalance.baseFree;
      const opposingBest = side === "bid" ? market.bestAsk : market.bestBid;
      const crossesBest = Boolean(
        opposingBest &&
        (side === "bid"
          ? rawPrice.gte(opposingBest)
          : rawPrice.lte(opposingBest)),
      );

      if (crossesBest && opposingBest) {
        const protocolConfig = await getTidebookAccounts(
          readProgram,
        ).protocolConfig.fetchNullable(deriveProtocolConfigPda());
        if (!protocolConfig) {
          throw new Error("Protocol configuration was not found");
        }
        const marketFeesAddress = deriveMarketFeesPda(marketAddress);
        const feeDenominator = new BN(10_000);
        const feeRoundingOffset = feeDenominator.subn(1);
        const takerFee = (grossQuote: BN) =>
          protocolConfig.takerFeeBps === 0
            ? new BN(0)
            : grossQuote
                .muln(protocolConfig.takerFeeBps)
                .add(feeRoundingOffset)
                .div(feeDenominator);

        // Build a bounded read-only snapshot of the FIFO path. Every matching
        // instruction still validates these relationships on-chain. If any
        // account changes before confirmation, the entire transaction rolls
        // back instead of committing only a prefix of the planned fills.
        const plan = await planBoundedMatches(
          signedProgram,
          marketAddress,
          market,
          wallet.publicKey,
          side,
          rawPrice,
          rawQuantity,
        );
        if (plan.steps.length === 0) {
          throw new Error("No crossing maker is currently available");
        }

        const filledQuantity = rawQuantity.sub(plan.remainingQuantity);
        const safeToPostRemainder =
          !plan.remainingQuantity.isZero() &&
          (plan.stopReason === "book-exhausted" ||
            plan.stopReason === "non-crossing");

        // A bid remainder must still produce at least one quote atom at the
        // limit price. If it rounds to zero, leave it free instead of adding an
        // instruction that the program would reject as a zero-notional order.
        const remainderCollateral =
          side === "bid"
            ? rawPrice.mul(plan.remainingQuantity).div(baseScale)
            : plan.remainingQuantity;
        const shouldPostRemainder =
          safeToPostRemainder && !remainderCollateral.isZero();

        // Quote settlement rounds once per fill at its maker price, matching
        // the program's arithmetic. Summing first and rounding later would be
        // incorrect when a taker crosses multiple price levels.
        const matchedCollateral =
          side === "bid"
            ? plan.steps.reduce((total, step) => {
                const grossQuote = step.makerPrice
                  .mul(step.fillQuantity)
                  .div(baseScale);
                return total.add(grossQuote).add(takerFee(grossQuote));
              }, new BN(0))
            : filledQuantity;
        const requiredBalance = shouldPostRemainder
          ? matchedCollateral.add(remainderCollateral)
          : matchedCollateral;
        if (availableBalance.lt(requiredBalance)) {
          throw new Error(
            `Insufficient free ${side === "bid" ? "quote" : "base"} balance`,
          );
        }

        const transaction = new Transaction();
        for (const step of plan.steps) {
          const instruction = await signedProgram.methods
            .matchLimitOrder(orderSide, rawPrice, step.takerQuantityBeforeFill)
            .accountsPartial({
              taker: wallet.publicKey,
              market: marketAddress,
              protocolConfig: deriveProtocolConfigPda(),
              marketFees: marketFeesAddress,
              makerOrder: step.makerOrder,
              makerPriceLevel: step.makerPriceLevel,
              nextOrder: step.nextOrder ?? signedProgram.programId,
              worseLevel: step.worseLevel ?? signedProgram.programId,
              levelRentRecipient:
                step.levelRentRecipient ?? signedProgram.programId,
              makerBalance: step.makerBalance,
              takerBalance: traderBalanceAddress,
            })
            .instruction();
          transaction.add(instruction);
        }

        // Matching changes only the opposing side and never increments the
        // market order id. A safe remainder can therefore reuse the pre-match
        // same-side insertion snapshot and execute after every fill atomically.
        let postedOrder: PublicKey | null = null;
        if (shouldPostRemainder) {
          const placement = await buildRestingOrderInstruction(
            signedProgram,
            wallet.publicKey,
            marketAddress,
            market,
            traderBalanceAddress,
            side,
            rawPrice,
            plan.remainingQuantity,
          );
          transaction.add(placement.instruction);
          postedOrder = placement.order;
        }

        // getTidebookProgram always constructs an AnchorProvider. The explicit
        // type communicates that this write path requires wallet signing, while
        // the read-only program elsewhere only exposes a generic Provider.
        const provider = signedProgram.provider as AnchorProvider;
        const signature = await provider.sendAndConfirm(transaction);

        setResult({
          kind: "matched",
          signature,
          makerCount: plan.steps.length,
          filledQuantity,
          remainingQuantity: plan.remainingQuantity,
          postedOrder,
        });

        if (plan.remainingQuantity.isZero() || postedOrder) {
          setPrice("");
          setQuantity("");
        } else {
          // A match-cap or zero-notional remainder stays free and visible so
          // the trader can explicitly submit the next bounded attempt.
          setQuantity(
            formatAtomicAmount(plan.remainingQuantity, market.baseDecimals),
          );
        }
      } else {
        const collateralAmount =
          side === "bid"
            ? rawPrice.mul(rawQuantity).div(baseScale)
            : rawQuantity;
        if (availableBalance.lt(collateralAmount)) {
          throw new Error(
            `Insufficient free ${side === "bid" ? "quote" : "base"} balance`,
          );
        }

        // Build the placement without submitting it so this same path can also
        // be appended after matching instructions for a safe taker remainder.
        const placement = await buildRestingOrderInstruction(
          signedProgram,
          wallet.publicKey,
          marketAddress,
          market,
          traderBalanceAddress,
          side,
          rawPrice,
          rawQuantity,
        );

        const provider = signedProgram.provider as AnchorProvider;
        const transaction = new Transaction().add(placement.instruction);
        const signature = await provider.sendAndConfirm(transaction);

        setResult({
          kind: "placed",
          signature,
          order: placement.order,
        });
        setPrice("");
        setQuantity("");
      }
      await Promise.all([loadMarket(), loadBalances()]);
    } catch (cause) {
      setError(getErrorMessage(cause));
    } finally {
      setPending(false);
    }
  };

  const withdrawProtocolFees = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (
      !signedProgram ||
      !wallet ||
      !marketAddress ||
      !market ||
      !marketFees ||
      role !== "super-admin"
    ) {
      return;
    }

    setFeePending(true);
    setFeeError(null);
    setFeeSignature(null);
    try {
      const amount = parsePositiveU64(feeAmount, "Fee amount");
      if (amount.gt(marketFees.accruedQuoteFees)) {
        throw new Error("Amount exceeds accrued protocol fees");
      }
      const destination = new PublicKey(feeDestination.trim());
      const signature = await signedProgram.methods
        .withdrawProtocolFees(amount)
        .accounts({
          superAdmin: wallet.publicKey,
          protocolConfig: deriveProtocolConfigPda(),
          market: marketAddress,
          marketFees: deriveMarketFeesPda(marketAddress),
          quoteMint: market.quoteMint,
          destinationQuoteAccount: destination,
          vaultAuthority: deriveVaultAuthorityPda(marketAddress),
          quoteVault: deriveVaultPda(marketAddress, market.quoteMint),
          tokenProgram: TOKEN_PROGRAM_ID,
        })
        .rpc();
      setFeeAmount("");
      setFeeSignature(signature);
      await loadMarket();
    } catch (cause) {
      setFeeError(getErrorMessage(cause));
    } finally {
      setFeePending(false);
    }
  };

  const manageMarket = async (action: "pause" | "unpause" | "close") => {
    if (!signedProgram || !wallet || !marketAddress || !market) return;
    if (
      action === "close" &&
      !window.confirm(
        "Close this market, its test faucet, and both empty vaults? This cannot be undone.",
      )
    ) {
      return;
    }

    setLifecyclePending(action);
    setLifecycleSignature(null);
    setError(null);

    try {
      let signature: string;

      if (action === "pause") {
        signature = await signedProgram.methods
          .pauseMarket()
          .accounts({ authority: wallet.publicKey, market: marketAddress })
          .rpc();
      } else if (action === "unpause") {
        signature = await signedProgram.methods
          .unpauseMarket()
          .accounts({ authority: wallet.publicKey, market: marketAddress })
          .rpc();
      } else {
        if (!market.openOrderCount.isZero()) {
          throw new Error("Cancel every open order before closing this market");
        }
        if (marketFees && !marketFees.accruedQuoteFees.isZero()) {
          throw new Error("Withdraw all accrued protocol fees before closing");
        }

        const transaction = new Transaction();
        const testFaucetAddress = deriveTestFaucetPda(marketAddress);
        if (testFaucet) {
          transaction.add(
            await signedProgram.methods
              .closeTestFaucet()
              .accounts({
                authority: wallet.publicKey,
                market: marketAddress,
                testFaucet: testFaucetAddress,
              })
              .instruction(),
          );
        }
        transaction.add(
          await signedProgram.methods
            .closeMarket()
            .accountsPartial({
              authority: wallet.publicKey,
              market: marketAddress,
              marketFees: deriveMarketFeesPda(marketAddress),
              testFaucet: testFaucetAddress,
              vaultAuthority: deriveVaultAuthorityPda(marketAddress),
              baseVault: deriveVaultPda(marketAddress, market.baseMint),
              quoteVault: deriveVaultPda(marketAddress, market.quoteMint),
              tokenProgram: TOKEN_PROGRAM_ID,
            })
            .instruction(),
        );
        const provider = signedProgram.provider as AnchorProvider;
        signature = await provider.sendAndConfirm(transaction);
      }

      setLifecycleSignature(signature);

      if (action === "close") {
        router.push("/markets");
        router.refresh();
      } else {
        await loadMarket();
      }
    } catch (cause) {
      setError(getErrorMessage(cause));
    } finally {
      setLifecyclePending(null);
    }
  };

  const status = market ? decodeMarketStatus(market.status) : null;
  const isMarketAuthority = Boolean(
    wallet && market && wallet.publicKey.equals(market.authority),
  );
  const rawPricePreview = useMemo(() => {
    if (!market || !price.trim()) return null;
    try {
      return parseTokenAmount(price, market.quoteDecimals, "Price");
    } catch {
      return null;
    }
  }, [market, price]);
  const rawQuantityPreview = useMemo(() => {
    if (!market || !quantity.trim()) return null;
    try {
      return parseTokenAmount(quantity, market.baseDecimals, "Quantity");
    } catch {
      return null;
    }
  }, [market, quantity]);
  const collateralPreview = useMemo(() => {
    if (!market || !rawPricePreview || !rawQuantityPreview) return null;

    if (side === "ask") {
      return `${formatAtomicAmount(rawQuantityPreview, market.baseDecimals)} base tokens`;
    }

    const baseScale = new BN(10).pow(new BN(market.baseDecimals));
    const quoteAmount = rawPricePreview.mul(rawQuantityPreview).div(baseScale);
    return `${formatAtomicAmount(quoteAmount, market.quoteDecimals)} quote tokens`;
  }, [market, rawPricePreview, rawQuantityPreview, side]);
  const crossesBest = useMemo(() => {
    if (!market || !rawPricePreview) return false;
    const opposingBest = side === "bid" ? market.bestAsk : market.bestBid;
    if (!opposingBest) return false;
    return side === "bid"
      ? rawPricePreview.gte(opposingBest)
      : rawPricePreview.lte(opposingBest);
  }, [market, rawPricePreview, side]);
  const rawBalanceAmount = useMemo(() => {
    if (!market || !balanceAmount.trim()) return null;
    try {
      return parseTokenAmount(
        balanceAmount,
        balanceAsset === "base" ? market.baseDecimals : market.quoteDecimals,
        "Amount",
      );
    } catch {
      return null;
    }
  }, [balanceAmount, balanceAsset, market]);
  const selectedWalletBalance =
    balanceAsset === "base" ? walletBaseBalance : walletQuoteBalance;
  const selectedFreeBalance = traderBalance
    ? balanceAsset === "base"
      ? traderBalance.baseFree
      : traderBalance.quoteFree
    : new BN(0);
  const depositExceedsWallet = Boolean(
    rawBalanceAmount && rawBalanceAmount.gt(selectedWalletBalance),
  );
  const withdrawalExceedsFree = Boolean(
    rawBalanceAmount && rawBalanceAmount.gt(selectedFreeBalance),
  );
  const submitLabel = pending
    ? "Submitting…"
    : side === "bid"
      ? "Buy base token"
      : "Sell base token";

  return (
    <div className="app-shell">
      <AppHeader />

      <main className="route-shell">
        <div className="market-detail-back">
          <Link href="/markets">← All markets</Link>
        </div>

        {loading && <div className="access-card">Loading market…</div>}

        {!loading && market && marketAddress && status && (
          <>
            <div className="route-heading market-detail-heading">
              <div>
                <div className="eyebrow">Market details</div>
                <h1>{shortAddress(marketAddress.toBase58())}</h1>
                <p>
                  Cross the best FIFO maker or post a collateralized limit
                  order. Every completed fill settles internal balances
                  atomically at the resting maker price.
                </p>
              </div>
              <span className={`role-badge market-${status}`}>{status}</span>
            </div>

            <div className="market-detail-layout">
              <section className="admin-card market-information">
                <div className="card-label">Pair</div>
                <div className="market-detail-pair">
                  <div>
                    <span>Base mint</span>
                    <strong>{market.baseMint.toBase58()}</strong>
                  </div>
                  <div>
                    <span>Quote mint</span>
                    <strong>{market.quoteMint.toBase58()}</strong>
                  </div>
                </div>

                <dl className="market-metadata">
                  <div>
                    <dt>Last traded price</dt>
                    <dd>
                      {lastTrade
                        ? (
                            <a
                              href={transactionExplorerUrl(
                                lastTrade.signature,
                              )}
                              target="_blank"
                              rel="noreferrer"
                            >
                              {formatAtomicAmount(
                                lastTrade.price,
                                market.quoteDecimals,
                              )}{" "}
                              ↗
                            </a>
                          )
                        : "No recent trade found"}
                    </dd>
                  </div>
                  <div>
                    <dt>Authority</dt>
                    <dd>{shortAddress(market.authority.toBase58())}</dd>
                  </div>
                  <div>
                    <dt>Next order</dt>
                    <dd>#{market.nextOrderId.toString()}</dd>
                  </div>
                  <div>
                    <dt>Open orders</dt>
                    <dd>{market.openOrderCount.toString()}</dd>
                  </div>
                  <div>
                    <dt>Best bid</dt>
                    <dd>
                      {market.bestBid
                        ? formatAtomicAmount(
                            market.bestBid,
                            market.quoteDecimals,
                          )
                        : "No open bids"}
                    </dd>
                  </div>
                  <div>
                    <dt>Best ask</dt>
                    <dd>
                      {market.bestAsk
                        ? formatAtomicAmount(
                            market.bestAsk,
                            market.quoteDecimals,
                          )
                        : "No open asks"}
                    </dd>
                  </div>
                  <div>
                    <dt>Base decimals</dt>
                    <dd>{market.baseDecimals}</dd>
                  </div>
                  <div>
                    <dt>Quote decimals</dt>
                    <dd>{market.quoteDecimals}</dd>
                  </div>
                  <div>
                    <dt>Price tick</dt>
                    <dd>
                      {formatAtomicAmount(
                        market.priceTickSize,
                        market.quoteDecimals,
                      )}{" "}
                      quote
                    </dd>
                  </div>
                  <div>
                    <dt>Quantity lot</dt>
                    <dd>
                      {formatAtomicAmount(
                        market.quantityLotSize,
                        market.baseDecimals,
                      )}{" "}
                      base
                    </dd>
                  </div>
                  {baseVault && (
                    <div>
                      <dt>Base vault</dt>
                      <dd>
                        <a
                          href={accountExplorerUrl(baseVault)}
                          target="_blank"
                          rel="noreferrer"
                        >
                          {shortAddress(baseVault.toBase58())} ↗
                        </a>
                      </dd>
                    </div>
                  )}

                  {quoteVault && (
                    <div>
                      <dt>Quote vault</dt>
                      <dd>
                        <a
                          href={accountExplorerUrl(quoteVault)}
                          target="_blank"
                          rel="noreferrer"
                        >
                          {shortAddress(quoteVault.toBase58())} ↗
                        </a>
                      </dd>
                    </div>
                  )}
                  {marketFees && (
                    <div>
                      <dt>Protocol fees</dt>
                      <dd>
                        {marketFees.accruedQuoteFees.toString()} raw quote
                      </dd>
                    </div>
                  )}
                </dl>

                <a
                  className="market-explorer-link"
                  href={accountExplorerUrl(marketAddress)}
                  target="_blank"
                  rel="noreferrer"
                >
                  View market account on Explorer ↗
                </a>

                {isMarketAuthority && (
                  <div className="market-card-actions">
                    {status === "active" ? (
                      <button
                        className="admin-action-button"
                        type="button"
                        disabled={lifecyclePending !== null}
                        onClick={() => void manageMarket("pause")}
                      >
                        {lifecyclePending === "pause"
                          ? "Pausing…"
                          : "Pause market"}
                      </button>
                    ) : (
                      <>
                        <button
                          className="admin-action-button"
                          type="button"
                          disabled={lifecyclePending !== null}
                          onClick={() => void manageMarket("unpause")}
                        >
                          {lifecyclePending === "unpause"
                            ? "Unpausing…"
                            : "Unpause market"}
                        </button>
                        <button
                          className="danger-button"
                          type="button"
                          disabled={
                            lifecyclePending !== null ||
                            !market.openOrderCount.isZero()
                          }
                          title={
                            market.openOrderCount.isZero()
                              ? "Close this market and both empty vaults"
                              : "Cancel every open order before closing"
                          }
                          onClick={() => void manageMarket("close")}
                        >
                          {lifecyclePending === "close"
                            ? "Closing…"
                            : "Close market"}
                        </button>
                      </>
                    )}
                  </div>
                )}
              </section>

              <section className="admin-card">
                <div className="card-label">Limit order</div>
                <h2>Place an order</h2>

                {status === "paused" && (
                  <div className="market-order-notice">
                    This market is paused. Existing orders may still be
                    canceled, but new orders are disabled.
                  </div>
                )}

                {status === "active" && !wallet && (
                  <div className="market-order-notice">
                    Connect your wallet to place an order.
                  </div>
                )}

                {status === "active" && wallet && (
                  <form className="market-create-form" onSubmit={placeOrder}>
                    {!traderBalance && !balanceLoading && (
                      <div className="market-order-notice">
                        Initialize your market balance below before placing an
                        order.
                      </div>
                    )}
                    <label>
                      Side
                      <select
                        value={side}
                        onChange={(event) =>
                          setSide(event.target.value as OrderSide)
                        }
                      >
                        <option value="bid">Buy base token</option>
                        <option value="ask">Sell base token</option>
                      </select>
                    </label>

                    <label>
                      Price per base token
                      <input
                        inputMode="decimal"
                        value={price}
                        onChange={(event) => setPrice(event.target.value)}
                        placeholder="150.00"
                        autoComplete="off"
                      />
                      <small>
                        Minimum price increment:{" "}
                        {formatAtomicAmount(
                          market.priceTickSize,
                          market.quoteDecimals,
                        )}{" "}
                        quote
                      </small>
                    </label>

                    <label>
                      Base quantity
                      <input
                        inputMode="decimal"
                        value={quantity}
                        onChange={(event) => setQuantity(event.target.value)}
                        placeholder="0.01"
                        autoComplete="off"
                      />
                      <small>
                        Minimum quantity increment:{" "}
                        {formatAtomicAmount(
                          market.quantityLotSize,
                          market.baseDecimals,
                        )}{" "}
                        base
                      </small>
                    </label>

                    <button
                      className="primary-button"
                      type="submit"
                      disabled={
                        !traderBalance ||
                        !rawPricePreview ||
                        !rawQuantityPreview ||
                        !rawPricePreview.mod(market.priceTickSize).isZero() ||
                        !rawQuantityPreview
                          .mod(market.quantityLotSize)
                          .isZero() ||
                        pending
                      }
                    >
                      {submitLabel}
                    </button>
                    {crossesBest ? (
                      <small>
                        Your limit price crosses an existing{" "}
                        {side === "bid" ? "sell" : "buy"} order, so it will
                        trade automatically. This transaction processes up to{" "}
                        {MAX_MATCHES_PER_TRANSACTION} FIFO makers atomically. A
                        safe remainder rests at your limit price; a cap-blocked
                        remainder stays free for retry.
                      </small>
                    ) : collateralPreview ? (
                      <small>This order will lock {collateralPreview}.</small>
                    ) : null}
                  </form>
                )}
              </section>
            </div>

            <section className="admin-card order-book-card">
              <div className="order-book-heading">
                <div>
                  <div className="card-label">Live on-chain depth</div>
                  <h2>Order book</h2>
                </div>
                <button
                  className="text-button"
                  type="button"
                  disabled={loading}
                  onClick={() => void loadMarket()}
                >
                  Refresh
                </button>
              </div>

              {bookError && (
                <div className="transaction-message transaction-error">
                  Could not load the order book: {bookError}
                </div>
              )}

              <div className="order-book-grid">
                <div className="order-book-side">
                  <h3>Buy orders</h3>
                  <div className="order-book-columns" aria-hidden="true">
                    <span>Price</span>
                    <span>Base quantity</span>
                    <span>Orders</span>
                  </div>
                  {bidLevels.length === 0 ? (
                    <p className="order-book-empty">No open buy orders</p>
                  ) : (
                    bidLevels.map((level) => (
                      <div
                        className="order-book-row order-book-bid"
                        key={level.address.toBase58()}
                      >
                        <span>
                          {formatAtomicAmount(
                            level.price,
                            market.quoteDecimals,
                          )}
                        </span>
                        <span>
                          {formatAtomicAmount(
                            level.totalRemainingQuantity,
                            market.baseDecimals,
                          )}
                        </span>
                        <span>{level.orderCount.toString()}</span>
                      </div>
                    ))
                  )}
                </div>

                <div className="order-book-side">
                  <h3>Sell orders</h3>
                  <div className="order-book-columns" aria-hidden="true">
                    <span>Price</span>
                    <span>Base quantity</span>
                    <span>Orders</span>
                  </div>
                  {askLevels.length === 0 ? (
                    <p className="order-book-empty">No open sell orders</p>
                  ) : (
                    askLevels.map((level) => (
                      <div
                        className="order-book-row order-book-ask"
                        key={level.address.toBase58()}
                      >
                        <span>
                          {formatAtomicAmount(
                            level.price,
                            market.quoteDecimals,
                          )}
                        </span>
                        <span>
                          {formatAtomicAmount(
                            level.totalRemainingQuantity,
                            market.baseDecimals,
                          )}
                        </span>
                        <span>{level.orderCount.toString()}</span>
                      </div>
                    ))
                  )}
                </div>
              </div>

              <p className="order-book-note">
                Levels are ordered by the program&apos;s linked price index. Last
                traded price is recovered from recent successful FillEvent
                logs; a production indexer will provide complete trade history
                and candles.
              </p>
            </section>

            <ServerTestFaucet
              baseMint={market.baseMint}
              quoteMint={market.quoteMint}
              baseDecimals={market.baseDecimals}
              quoteDecimals={market.quoteDecimals}
              onClaimed={loadBalances}
              onPairDetected={setServerFaucetPair}
            />

            {serverFaucetPair !== true && wallet && testFaucet && (
              <section className="admin-card market-balance-card">
                <div className="card-label">Devnet test faucet</div>
                <h2>Get test assets</h2>
                <p>
                  These tokens have no value. Each claim mints{" "}
                  {formatAtomicAmount(
                    testFaucet.baseClaimAmount,
                    market.baseDecimals,
                  )}{" "}
                  base and{" "}
                  {formatAtomicAmount(
                    testFaucet.quoteClaimAmount,
                    market.quoteDecimals,
                  )}{" "}
                  quote tokens to your wallet.
                </p>
                <button
                  className="primary-button"
                  type="button"
                  disabled={faucetPending !== null}
                  onClick={() => void claimTestTokens()}
                >
                  {faucetPending === "claim" ? "Minting…" : "Claim test tokens"}
                </button>
                {faucetError && (
                  <div className="transaction-message transaction-error">
                    {faucetError}
                  </div>
                )}
                {faucetSignature && (
                  <div className="transaction-message transaction-success">
                    Test assets minted.{" "}
                    <a
                      href={transactionExplorerUrl(faucetSignature)}
                      target="_blank"
                      rel="noreferrer"
                    >
                      View transaction ↗
                    </a>
                  </div>
                )}
              </section>
            )}

            {serverFaucetPair === false &&
              role === "super-admin" &&
              !testFaucet && (
                <section className="admin-card market-balance-card">
                  <div className="card-label">Devnet test faucet</div>
                  <h2>Enable public test minting</h2>
                  <p>
                    This permanently transfers both mint authorities to
                    Tidebook. Enable it only for fresh, zero-supply tokens with
                    no economic value.
                  </p>
                  <form
                    className="market-create-form balance-form"
                    onSubmit={(event) => event.preventDefault()}
                  >
                    <label>
                      Base tokens per claim
                      <input
                        inputMode="decimal"
                        value={baseClaimAmount}
                        onChange={(event) =>
                          setBaseClaimAmount(event.target.value)
                        }
                        placeholder="10"
                        autoComplete="off"
                      />
                    </label>
                    <label>
                      Quote tokens per claim
                      <input
                        inputMode="decimal"
                        value={quoteClaimAmount}
                        onChange={(event) =>
                          setQuoteClaimAmount(event.target.value)
                        }
                        placeholder="10000"
                        autoComplete="off"
                      />
                    </label>
                    <button
                      className="primary-button"
                      type="button"
                      disabled={
                        faucetPending !== null ||
                        !baseClaimAmount.trim() ||
                        !quoteClaimAmount.trim()
                      }
                      onClick={() => void initializeTestFaucet()}
                    >
                      {faucetPending === "initialize"
                        ? "Enabling…"
                        : "Enable public faucet"}
                    </button>
                  </form>
                  {faucetError && (
                    <div className="transaction-message transaction-error">
                      {faucetError}
                    </div>
                  )}
                </section>
              )}

            {wallet && (
              <section className="admin-card market-balance-card">
                <div className="admin-list-heading">
                  <div>
                    <div className="card-label">
                      Wallet and internal balances
                    </div>
                    <h2>Fund this market</h2>
                  </div>
                  <button
                    className="text-button"
                    type="button"
                    disabled={balanceLoading || balancePending !== null}
                    onClick={() => void loadBalances()}
                  >
                    {balanceLoading ? "Refreshing…" : "Refresh"}
                  </button>
                </div>

                <dl className="market-metadata balance-metadata">
                  <div>
                    <dt>Base in wallet</dt>
                    <dd>
                      {formatAtomicAmount(
                        walletBaseBalance,
                        market.baseDecimals,
                      )}
                    </dd>
                  </div>
                  <div>
                    <dt>Quote in wallet</dt>
                    <dd>
                      {formatAtomicAmount(
                        walletQuoteBalance,
                        market.quoteDecimals,
                      )}
                    </dd>
                  </div>
                </dl>

                {!balanceLoading && !traderBalance && (
                  <div className="balance-initialize">
                    <p>
                      Create one balance ledger for this wallet and market. This
                      does not move tokens.
                    </p>
                    <button
                      className="primary-button"
                      type="button"
                      disabled={balancePending !== null}
                      onClick={() => void initializeBalance()}
                    >
                      {balancePending === "initialize"
                        ? "Initializing…"
                        : "Initialize balance"}
                    </button>
                  </div>
                )}

                {traderBalance && (
                  <>
                    <dl className="market-metadata balance-metadata">
                      <div>
                        <dt>Base free</dt>
                        <dd>
                          {formatAtomicAmount(
                            traderBalance.baseFree,
                            market.baseDecimals,
                          )}
                        </dd>
                      </div>
                      <div>
                        <dt>Base locked</dt>
                        <dd>
                          {formatAtomicAmount(
                            traderBalance.baseLocked,
                            market.baseDecimals,
                          )}
                        </dd>
                      </div>
                      <div>
                        <dt>Quote free</dt>
                        <dd>
                          {formatAtomicAmount(
                            traderBalance.quoteFree,
                            market.quoteDecimals,
                          )}
                        </dd>
                      </div>
                      <div>
                        <dt>Quote locked</dt>
                        <dd>
                          {formatAtomicAmount(
                            traderBalance.quoteLocked,
                            market.quoteDecimals,
                          )}
                        </dd>
                      </div>
                    </dl>

                    <form
                      className="market-create-form balance-form"
                      onSubmit={(event) => event.preventDefault()}
                    >
                      <label>
                        Asset
                        <select
                          value={balanceAsset}
                          onChange={(event) =>
                            setBalanceAsset(
                              event.target.value as "base" | "quote",
                            )
                          }
                        >
                          <option value="base">Base token</option>
                          <option value="quote">Quote token</option>
                        </select>
                      </label>
                      <label>
                        Amount
                        <input
                          inputMode="decimal"
                          value={balanceAmount}
                          onChange={(event) =>
                            setBalanceAmount(event.target.value)
                          }
                          placeholder={
                            balanceAsset === "base" ? "1.0" : "100.0"
                          }
                          autoComplete="off"
                        />
                      </label>
                      <div className="balance-actions">
                        <button
                          className="primary-button"
                          type="button"
                          disabled={
                            !rawBalanceAmount ||
                            depositExceedsWallet ||
                            balancePending !== null
                          }
                          onClick={() => void transferBalance("deposit")}
                        >
                          {balancePending === "deposit"
                            ? "Depositing…"
                            : "Deposit"}
                        </button>
                        <button
                          className="admin-action-button"
                          type="button"
                          disabled={
                            !rawBalanceAmount ||
                            withdrawalExceedsFree ||
                            balancePending !== null
                          }
                          onClick={() => void transferBalance("withdraw")}
                        >
                          {balancePending === "withdraw"
                            ? "Withdrawing…"
                            : "Withdraw free balance"}
                        </button>
                      </div>
                      {depositExceedsWallet && (
                        <small className="form-error">
                          Deposit exceeds the selected asset balance in your
                          wallet.
                        </small>
                      )}
                      {withdrawalExceedsFree && (
                        <small className="form-error">
                          Withdrawal exceeds the selected asset&apos;s free
                          internal balance.
                        </small>
                      )}
                      <small>
                        Orders move free balance to locked balance. Cancellation
                        releases it back to free; only withdrawal moves tokens
                        back to your wallet.
                      </small>
                    </form>
                  </>
                )}

                {balanceError && (
                  <div className="transaction-message transaction-error">
                    {balanceError}
                  </div>
                )}
                {balanceSignature && (
                  <div className="transaction-message transaction-success">
                    Balance updated.{" "}
                    <a
                      href={transactionExplorerUrl(balanceSignature)}
                      target="_blank"
                      rel="noreferrer"
                    >
                      View transaction ↗
                    </a>
                  </div>
                )}
              </section>
            )}

            {role === "super-admin" && marketFees && (
              <section className="admin-card market-balance-card">
                <div className="card-label">Protocol treasury</div>
                <h2>Withdraw accrued fees</h2>
                <p>
                  Available: {marketFees.accruedQuoteFees.toString()} raw quote
                  atoms. The destination may be any token account for this
                  market&apos;s quote mint.
                </p>
                <form className="admin-form" onSubmit={withdrawProtocolFees}>
                  <label>
                    Destination quote-token account
                    <input
                      value={feeDestination}
                      onChange={(event) =>
                        setFeeDestination(event.target.value)
                      }
                      placeholder="Treasury token account"
                      autoComplete="off"
                    />
                  </label>
                  <label>
                    Raw quote amount
                    <input
                      inputMode="numeric"
                      value={feeAmount}
                      onChange={(event) => setFeeAmount(event.target.value)}
                      placeholder="0"
                      autoComplete="off"
                    />
                  </label>
                  <button
                    className="primary-button"
                    type="submit"
                    disabled={
                      feePending ||
                      !feeDestination.trim() ||
                      !feeAmount.trim() ||
                      marketFees.accruedQuoteFees.isZero()
                    }
                  >
                    {feePending ? "Withdrawing…" : "Withdraw fees"}
                  </button>
                </form>
                {feeError && (
                  <div className="transaction-message transaction-error">
                    {feeError}
                  </div>
                )}
                {feeSignature && (
                  <div className="transaction-message transaction-success">
                    Fee withdrawal confirmed.{" "}
                    <a
                      href={transactionExplorerUrl(feeSignature)}
                      target="_blank"
                      rel="noreferrer"
                    >
                      View on Explorer ↗
                    </a>
                  </div>
                )}
              </section>
            )}
          </>
        )}

        {error && (
          <div className="transaction-message transaction-error">{error}</div>
        )}

        {result?.kind === "placed" && (
          <div className="transaction-message transaction-success">
            Order created at {shortAddress(result.order.toBase58())}.{" "}
            <a
              href={transactionExplorerUrl(result.signature)}
              target="_blank"
              rel="noreferrer"
            >
              View transaction ↗
            </a>
          </div>
        )}

        {result?.kind === "matched" && (
          <div className="transaction-message transaction-success">
            Filled {result.filledQuantity.toString()} raw base across{" "}
            {result.makerCount} maker{result.makerCount === 1 ? "" : "s"}.
            {result.postedOrder ? (
              <>
                {" "}
                Posted {result.remainingQuantity.toString()} as resting order{" "}
                {shortAddress(result.postedOrder.toBase58())}.
              </>
            ) : (
              !result.remainingQuantity.isZero() && (
                <> {result.remainingQuantity.toString()} remains unprocessed.</>
              )
            )}{" "}
            <a
              href={transactionExplorerUrl(result.signature)}
              target="_blank"
              rel="noreferrer"
            >
              View transaction ↗
            </a>
          </div>
        )}

        {lifecycleSignature && (
          <div className="transaction-message transaction-success">
            Market lifecycle updated.{" "}
            <a
              href={transactionExplorerUrl(lifecycleSignature)}
              target="_blank"
              rel="noreferrer"
            >
              View transaction ↗
            </a>
          </div>
        )}
      </main>
    </div>
  );
}
