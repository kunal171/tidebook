import { readFile } from "node:fs/promises";
import anchor from "@anchor-lang/core";
import web3 from "@solana/web3.js";

const { AnchorProvider, BN, Program, Wallet } = anchor;
const { Connection, Keypair, PublicKey, SystemProgram, Transaction } = web3;

const RPC_URL =
  process.env.SOLANA_RPC_URL ?? "https://api.devnet.solana.com";
const TOKEN_PROGRAM_ID = new PublicKey(
  "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
);
const PRICE = new BN("100000000");
const TICK_SIZE = new BN("10000");
const LOT_SIZE = new BN("100000");
const MAKER_QUANTITY = new BN("100000");
const TAKER_QUANTITY = new BN("200000");
const SECOND_FILL_QUANTITY = new BN("100000");
const MAKER_BASE_DEPOSIT = new BN("100000");
const TAKER_QUOTE_DEPOSIT = new BN("1000000");

function required(name) {
  const value = process.env[name];
  if (!value) throw new Error(`Set ${name}`);
  return value;
}

async function loadKeypair(path) {
  const secret = JSON.parse(await readFile(path, "utf8"));
  return Keypair.fromSecretKey(Uint8Array.from(secret));
}

function derivePda(programId, seeds) {
  return PublicKey.findProgramAddressSync(seeds, programId)[0];
}

function u64Seed(value) {
  return value.toArrayLike(Buffer, "le", 8);
}

function assertBn(actual, expected, label) {
  if (!actual.eq(expected)) {
    throw new Error(
      `${label}: expected ${expected.toString()}, received ${actual.toString()}`,
    );
  }
}

function programFor(idl, connection, keypair) {
  const provider = new AnchorProvider(connection, new Wallet(keypair), {
    commitment: "confirmed",
    preflightCommitment: "confirmed",
  });
  return new Program(idl, provider);
}

const deployer = await loadKeypair(required("SOLANA_WALLET"));
const secondMaker = await loadKeypair(required("MAKER_TWO_WALLET"));
const taker = await loadKeypair(required("TAKER_WALLET"));
const baseMint = new PublicKey(required("BASE_MINT"));
const quoteMint = new PublicKey(required("QUOTE_MINT"));
const firstMakerBaseToken = new PublicKey(required("MAKER_ONE_BASE_TOKEN"));
const secondMakerBaseToken = new PublicKey(required("MAKER_TWO_BASE_TOKEN"));
const takerQuoteToken = new PublicKey(required("TAKER_QUOTE_TOKEN"));

const idl = JSON.parse(
  await readFile(new URL("../idl/tidebook.json", import.meta.url), "utf8"),
);
const connection = new Connection(RPC_URL, "confirmed");
const deployerProgram = programFor(idl, connection, deployer);
const secondMakerProgram = programFor(idl, connection, secondMaker);
const takerProgram = programFor(idl, connection, taker);
const programId = new PublicKey(idl.address);

const market = derivePda(programId, [
  Buffer.from("market"),
  baseMint.toBuffer(),
  quoteMint.toBuffer(),
]);
const adminRecord = derivePda(programId, [
  Buffer.from("admin"),
  deployer.publicKey.toBuffer(),
]);
const vaultAuthority = derivePda(programId, [
  Buffer.from("vault-authority"),
  market.toBuffer(),
]);
const baseVault = derivePda(programId, [
  Buffer.from("vault"),
  market.toBuffer(),
  baseMint.toBuffer(),
]);
const quoteVault = derivePda(programId, [
  Buffer.from("vault"),
  market.toBuffer(),
  quoteMint.toBuffer(),
]);
const firstMakerBalance = derivePda(programId, [
  Buffer.from("trader_balance"),
  market.toBuffer(),
  deployer.publicKey.toBuffer(),
]);
const secondMakerBalance = derivePda(programId, [
  Buffer.from("trader_balance"),
  market.toBuffer(),
  secondMaker.publicKey.toBuffer(),
]);
const takerBalance = derivePda(programId, [
  Buffer.from("trader_balance"),
  market.toBuffer(),
  taker.publicKey.toBuffer(),
]);

const initializeMarketSignature = await deployerProgram.methods
  .initializeMarket(TICK_SIZE, LOT_SIZE)
  .accounts({
    authority: deployer.publicKey,
    adminRecord,
    market,
    baseMint,
    quoteMint,
    vaultAuthority,
    baseVault,
    quoteVault,
    tokenProgram: TOKEN_PROGRAM_ID,
    systemProgram: SystemProgram.programId,
  })
  .rpc();

async function initializeBalance(program, owner, traderBalance) {
  return program.methods
    .initializeTraderBalance()
    .accounts({
      owner,
      market,
      traderBalance,
      systemProgram: SystemProgram.programId,
    })
    .rpc();
}

const initializeFirstMakerBalanceSignature = await initializeBalance(
  deployerProgram,
  deployer.publicKey,
  firstMakerBalance,
);
const initializeSecondMakerBalanceSignature = await initializeBalance(
  secondMakerProgram,
  secondMaker.publicKey,
  secondMakerBalance,
);
const initializeTakerBalanceSignature = await initializeBalance(
  takerProgram,
  taker.publicKey,
  takerBalance,
);

async function deposit(
  program,
  owner,
  traderBalance,
  amount,
  mint,
  traderTokenAccount,
  marketVault,
) {
  return program.methods
    .deposit(amount)
    .accounts({
      owner,
      market,
      traderBalance,
      depositMint: mint,
      traderTokenAccount,
      vaultAuthority,
      marketVault,
      tokenProgram: TOKEN_PROGRAM_ID,
    })
    .rpc();
}

const firstMakerDepositSignature = await deposit(
  deployerProgram,
  deployer.publicKey,
  firstMakerBalance,
  MAKER_BASE_DEPOSIT,
  baseMint,
  firstMakerBaseToken,
  baseVault,
);
const secondMakerDepositSignature = await deposit(
  secondMakerProgram,
  secondMaker.publicKey,
  secondMakerBalance,
  MAKER_BASE_DEPOSIT,
  baseMint,
  secondMakerBaseToken,
  baseVault,
);
const takerDepositSignature = await deposit(
  takerProgram,
  taker.publicKey,
  takerBalance,
  TAKER_QUOTE_DEPOSIT,
  quoteMint,
  takerQuoteToken,
  quoteVault,
);

const initialMarket = await deployerProgram.account.market.fetch(market);
const firstOrder = derivePda(programId, [
  Buffer.from("order"),
  market.toBuffer(),
  u64Seed(initialMarket.nextOrderId),
]);
const priceLevel = derivePda(programId, [
  Buffer.from("price_level"),
  market.toBuffer(),
  Buffer.from("ask"),
  u64Seed(PRICE),
]);
const firstPlacementSignature = await deployerProgram.methods
  .insertLimitOrder({ ask: {} }, PRICE, MAKER_QUANTITY)
  .accountsPartial({
    trader: deployer.publicKey,
    market,
    order: firstOrder,
    priceLevel,
    traderBalance: firstMakerBalance,
    systemProgram: SystemProgram.programId,
    betterLevel: programId,
    worseLevel: programId,
  })
  .rpc();

const marketAfterFirst = await secondMakerProgram.account.market.fetch(market);
const secondOrder = derivePda(programId, [
  Buffer.from("order"),
  market.toBuffer(),
  u64Seed(marketAfterFirst.nextOrderId),
]);
const secondPlacementSignature = await secondMakerProgram.methods
  .appendLimitOrder({ ask: {} }, PRICE, MAKER_QUANTITY)
  .accounts({
    trader: secondMaker.publicKey,
    market,
    order: secondOrder,
    priceLevel,
    previousOrder: firstOrder,
    traderBalance: secondMakerBalance,
    systemProgram: SystemProgram.programId,
  })
  .rpc();

const firstMatchInstruction = await takerProgram.methods
  .matchLimitOrder({ bid: {} }, PRICE, TAKER_QUANTITY)
  .accountsPartial({
    taker: taker.publicKey,
    market,
    makerOrder: firstOrder,
    makerPriceLevel: priceLevel,
    nextOrder: secondOrder,
    worseLevel: programId,
    levelRentRecipient: programId,
    makerBalance: firstMakerBalance,
    takerBalance,
  })
  .instruction();
const secondMatchInstruction = await takerProgram.methods
  .matchLimitOrder({ bid: {} }, PRICE, SECOND_FILL_QUANTITY)
  .accountsPartial({
    taker: taker.publicKey,
    market,
    makerOrder: secondOrder,
    makerPriceLevel: priceLevel,
    nextOrder: programId,
    worseLevel: programId,
    levelRentRecipient: deployer.publicKey,
    makerBalance: secondMakerBalance,
    takerBalance,
  })
  .instruction();
const matchTransaction = new Transaction().add(
  firstMatchInstruction,
  secondMatchInstruction,
);
const matchSignature =
  await takerProgram.provider.sendAndConfirm(matchTransaction);

const [
  finalMarket,
  firstOrderState,
  secondOrderState,
  firstMakerLedger,
  secondMakerLedger,
  takerLedger,
] = await Promise.all([
  takerProgram.account.market.fetch(market),
  takerProgram.account.order.fetch(firstOrder),
  takerProgram.account.order.fetch(secondOrder),
  takerProgram.account.traderBalance.fetch(firstMakerBalance),
  takerProgram.account.traderBalance.fetch(secondMakerBalance),
  takerProgram.account.traderBalance.fetch(takerBalance),
]);

if (!("filled" in firstOrderState.status)) {
  throw new Error("First FIFO maker was not filled");
}
if (!("filled" in secondOrderState.status)) {
  throw new Error("Second FIFO maker was not filled");
}
assertBn(firstOrderState.remainingQuantity, new BN(0), "first remaining");
assertBn(secondOrderState.remainingQuantity, new BN(0), "second remaining");
if (finalMarket.bestAsk !== null) {
  throw new Error("Market best ask was not cleared");
}
if (await connection.getAccountInfo(priceLevel, "confirmed")) {
  throw new Error("Empty price level was not closed");
}
assertBn(finalMarket.openOrderCount, new BN(0), "market open order count");
assertBn(firstMakerLedger.baseLocked, new BN(0), "first maker base locked");
assertBn(firstMakerLedger.quoteFree, new BN("100000"), "first maker quote free");
assertBn(secondMakerLedger.baseLocked, new BN(0), "second maker base locked");
assertBn(
  secondMakerLedger.quoteFree,
  new BN("100000"),
  "second maker quote free",
);
assertBn(takerLedger.baseFree, TAKER_QUANTITY, "taker base free");
assertBn(takerLedger.quoteFree, new BN("800000"), "taker quote free");

console.log(
  JSON.stringify(
    {
      program: programId.toBase58(),
      market: market.toBase58(),
      baseMint: baseMint.toBase58(),
      quoteMint: quoteMint.toBase58(),
      firstMaker: deployer.publicKey.toBase58(),
      secondMaker: secondMaker.publicKey.toBase58(),
      taker: taker.publicKey.toBase58(),
      firstOrder: firstOrder.toBase58(),
      secondOrder: secondOrder.toBase58(),
      priceLevel: priceLevel.toBase58(),
      signatures: {
        initializeMarket: initializeMarketSignature,
        initializeFirstMakerBalance: initializeFirstMakerBalanceSignature,
        initializeSecondMakerBalance: initializeSecondMakerBalanceSignature,
        initializeTakerBalance: initializeTakerBalanceSignature,
        firstMakerDeposit: firstMakerDepositSignature,
        secondMakerDeposit: secondMakerDepositSignature,
        takerDeposit: takerDepositSignature,
        firstPlacement: firstPlacementSignature,
        secondPlacement: secondPlacementSignature,
        atomicMatch: matchSignature,
      },
      finalState: {
        firstOrder: "filled",
        secondOrder: "filled",
        secondOrderRemaining: secondOrderState.remainingQuantity.toString(),
        marketOpenOrderCount: finalMarket.openOrderCount.toString(),
        takerBaseFree: takerLedger.baseFree.toString(),
        takerQuoteFree: takerLedger.quoteFree.toString(),
      },
    },
    null,
    2,
  ),
);

