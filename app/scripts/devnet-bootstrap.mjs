import { readFile } from "node:fs/promises";
import anchor from "@anchor-lang/core";
import web3 from "@solana/web3.js";

const { AnchorProvider, BN, Program, Wallet } = anchor;
const {
  Connection,
  Keypair,
  PublicKey,
  SystemProgram,
} = web3;
const BPF_LOADER_UPGRADEABLE_PROGRAM_ID = new PublicKey(
  "BPFLoaderUpgradeab1e11111111111111111111111",
);

const RPC_URL = "https://api.devnet.solana.com";
const BASE_MINT = new PublicKey(
  "FggaZG7eJ5tpr2vmpNMbMCHkWkcw9n2Nhmf54g3wQwGk",
);
const QUOTE_MINT = new PublicKey(
  "8KRa3QwidV2yZspR3wr4p3eNzq2KtejMbU1tZUgNLdtt",
);
const PRICE_TICK_SIZE = new BN("10000");
const QUANTITY_LOT_SIZE = new BN("100000");
const ORDER_PRICE = new BN("67250120000");
const ORDER_QUANTITY = new BN("100000");

const walletPath = process.env.SOLANA_WALLET;
if (!walletPath) {
  throw new Error("Set SOLANA_WALLET to the deployer keypair path");
}

const idl = JSON.parse(
  await readFile(new URL("../idl/tidebook.json", import.meta.url), "utf8"),
);
const secret = JSON.parse(await readFile(walletPath, "utf8"));
const payer = Keypair.fromSecretKey(Uint8Array.from(secret));
const wallet = new Wallet(payer);
const connection = new Connection(RPC_URL, "confirmed");
const provider = new AnchorProvider(connection, wallet, {
  commitment: "confirmed",
  preflightCommitment: "confirmed",
});
const program = new Program(idl, provider);
const programId = new PublicKey(idl.address);

const derivePda = (seeds) =>
  PublicKey.findProgramAddressSync(seeds, programId)[0];
const protocolConfig = derivePda([Buffer.from("protocol_config")]);
const deployerAdmin = derivePda([
  Buffer.from("admin"),
  payer.publicKey.toBuffer(),
]);
const market = derivePda([
  Buffer.from("market"),
  BASE_MINT.toBuffer(),
  QUOTE_MINT.toBuffer(),
]);
const programData = PublicKey.findProgramAddressSync(
  [programId.toBuffer()],
  BPF_LOADER_UPGRADEABLE_PROGRAM_ID,
)[0];

let initializeProtocolSignature = null;
if (!(await connection.getAccountInfo(protocolConfig, "confirmed"))) {
  initializeProtocolSignature = await program.methods
    .initializeProtocol()
    .accounts({
      deployer: payer.publicKey,
      protocolConfig,
      deployerAdmin,
      program: programId,
      programData,
      systemProgram: SystemProgram.programId,
    })
    .rpc();
}

let initializeMarketSignature = null;
if (!(await connection.getAccountInfo(market, "confirmed"))) {
  initializeMarketSignature = await program.methods
    .initializeMarket(PRICE_TICK_SIZE, QUANTITY_LOT_SIZE)
    .accounts({
      authority: payer.publicKey,
      adminRecord: deployerAdmin,
      market,
      baseMint: BASE_MINT,
      quoteMint: QUOTE_MINT,
      systemProgram: SystemProgram.programId,
    })
    .rpc();
}

const marketState = await program.account.market.fetch(market);
const orderId = marketState.nextOrderId;
const order = derivePda([
  Buffer.from("order"),
  market.toBuffer(),
  orderId.toArrayLike(Buffer, "le", 8),
]);
const vaultAuthority = derivePda([
  Buffer.from("vault-authority"),
  market.toBuffer(),
]);
const collateralMint = marketState.quoteMint;
const marketVault = derivePda([
  Buffer.from("vault"),
  market.toBuffer(),
  collateralMint.toBuffer(),
]);
const tokenAccounts = await connection.getParsedTokenAccountsByOwner(
  payer.publicKey,
  { mint: collateralMint },
  "confirmed",
);
const traderCollateral = tokenAccounts.value.find(({ account }) => {
  if (!("parsed" in account.data)) return false;

  const amount = account.data.parsed?.info?.tokenAmount?.amount;
  return typeof amount === "string" && new BN(amount).gtn(0);
})?.pubkey;

if (!traderCollateral) {
  throw new Error(
    "No funded quote token account found for " + collateralMint.toBase58(),
  );
}

const priceLevelFor = (price) =>
  derivePda([
    Buffer.from("price_level"),
    market.toBuffer(),
    Buffer.from("bid"),
    price.toArrayLike(Buffer, "le", 8),
  ]);
const orderAccountsFor = (price) => ({
  trader: payer.publicKey,
  market,
  order,
  priceLevel: priceLevelFor(price),
  collateralMint,
  traderCollateral,
  vaultAuthority,
  marketVault,
  tokenProgram: new PublicKey("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"),
  systemProgram: SystemProgram.programId,
  betterLevel: programId,
  worseLevel: programId,
});

async function expectRejected(label, operation) {
  try {
    const signature = await operation();
    throw new Error(label + " unexpectedly succeeded: " + signature);
  } catch (cause) {
    const message = cause instanceof Error ? cause.message : String(cause);
    if (message.startsWith(label + " unexpectedly succeeded")) throw cause;
    return message.split("\n")[0];
  }
}

const offTickPrice = ORDER_PRICE.addn(1);
const offTickError = await expectRejected("off-tick order", () =>
  program.methods
    .insertLimitOrder({ bid: {} }, offTickPrice, ORDER_QUANTITY)
    .accounts(orderAccountsFor(offTickPrice))
    .rpc(),
);
const offLotError = await expectRejected("off-lot order", () =>
  program.methods
    .insertLimitOrder({ bid: {} }, ORDER_PRICE, ORDER_QUANTITY.addn(1))
    .accounts(orderAccountsFor(ORDER_PRICE))
    .rpc(),
);

const placeOrderSignature = await program.methods
  .insertLimitOrder({ bid: {} }, ORDER_PRICE, ORDER_QUANTITY)
  .accounts(orderAccountsFor(ORDER_PRICE))
  .rpc();
const cancelOrderSignature = await program.methods
  .cancelLimitOrder(orderId)
  .accounts({
    owner: payer.publicKey,
    market,
    order,
    collateralMint,
    ownerCollateral: traderCollateral,
    vaultAuthority,
    marketVault,
    tokenProgram: new PublicKey("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"),
  })
  .rpc();

const orderState = await program.account.order.fetch(order);

console.log(
  JSON.stringify(
    {
      program: programId.toBase58(),
      programData: programData.toBase58(),
      deployer: payer.publicKey.toBase58(),
      protocolConfig: protocolConfig.toBase58(),
      deployerAdmin: deployerAdmin.toBase58(),
      initializeProtocolSignature,
      market: market.toBase58(),
      initializeMarketSignature,
      priceTickSize: PRICE_TICK_SIZE.toString(),
      quantityLotSize: QUANTITY_LOT_SIZE.toString(),
      order: order.toBase58(),
      orderId: orderId.toString(),
      placeOrderSignature,
      cancelOrderSignature,
      finalOrderStatus: Object.keys(orderState.status)[0],
      offTickError,
      offLotError,
    },
    null,
    2,
  ),
);
