const {newAccountWithLamports} = require("@solana/spl-token-swap/dist/util/new-account-with-lamports");
const {Connection, PublicKey, Account, Keypair} = require("@solana/web3.js");
const {TokenSwap, CurveType} = require("@solana/spl-token-swap");
const {Token, TOKEN_PROGRAM_ID} = require('@solana/spl-token')

// ----------------------------------------------------------------------------- constants

const TOKEN_SWAP_PROGRAM_ID = new PublicKey('AStiSsWN3KVdCKnhifLYcw4M4Ey8LMUA9p2Ka6bsGtf8'); //no need to change in the lib, we can just change here
// const TOKEN_SWAP_PROGRAM_ID = new PublicKey('B2pRB7qNuBm4Lzovj8UFUgi9voBbnwwMkFGUtb6RhXK6');
// (!)This seems to be used throughout the code as a PRODUCTION ON/OFF SWITCH
const SWAP_PROGRAM_OWNER_FEE_ADDRESS = "AFe99p6byLxYfEV9E1nNumSeKdtgXm2HL5Gy5dN6icj9";

// Pool fees
const TRADING_FEE_NUMERATOR = 25; //0.25% to LPs, modelled on uniswap
const TRADING_FEE_DENOMINATOR = 10000;
const OWNER_TRADING_FEE_NUMERATOR = 5; //0.05% to exchange, modelled on uniswap
const OWNER_TRADING_FEE_DENOMINATOR = 10000;
const OWNER_WITHDRAW_FEE_NUMERATOR = 0;
const OWNER_WITHDRAW_FEE_DENOMINATOR = 0;
const HOST_FEE_NUMERATOR = 20; //ui host gets 1/5 of the fees of the protocol
const HOST_FEE_DENOMINATOR = 100;

// const CURVE_TYPE = CurveType.ConstantProduct;
const CURVE_TYPE = CurveType.ConstantPrice; //todo note constant price won't work without fixing this first - https://github.com/solana-labs/solana-program-library/issues/2234

// Initial amount in each swap token (1m each)
const currentSwapTokenA = 1000000;
const currentSwapTokenB = 1000000;

// Swap instruction constants
// Because there is no withdraw fee in the production version, these numbers
// need to get slightly tweaked in the two cases.
const SWAP_AMOUNT_IN = 100000;
const SWAP_AMOUNT_OUT = SWAP_PROGRAM_OWNER_FEE_ADDRESS ? 90661 : 90674;
// Pool token amount to withdraw / deposit (10m or 1% of 1bn)
const POOL_TOKEN_AMOUNT = 10000000;

// ----------------------------------------------------------------------------- globals state

const url = "https://api.devnet.solana.com"
let connection;

// --------------------------------------- exchange
let tokenSwap; // the object for the specific pool we've created
let authority; // authority over that pool
let nonce; // nonce used to generate the authority public key
// owner of the user accounts
let owner = Keypair.fromSecretKey(Uint8Array.from([208,175,150,242,88,34,108,88,177,16,168,75,115,181,199,242,120,4,78,75,19,227,13,215,184,108,226,53,111,149,179,84,137,121,79,1,160,223,124,241,202,203,220,237,50,242,57,158,226,207,203,188,43,28,70,110,214,234,251,15,249,157,62,80]));
// Token pool
let mintPool; //mint account for the pool token
let tokenAccountPool; //the initial supply of tokens for an exchange is stored there
let feeAccount;
// Tokens swapped
let mintA;
let mintB;
let tokenAccountA; //exchange's token A
let tokenAccountB; //exchange's otken B

// --------------------------------------- user
let userAccountA; //user's token A
let userAccountB; //user's token B
let userAccountPool; //user's account for LP tokens

// --------------------------------------- host
let hostPoolAccount; //where the UI host's fees will accrue

// ----------------------------------------------------------------------------- connection
async function getConnection() {
  if (connection) return connection;
  connection = new Connection(url, 'recent');
  const version = await connection.getVersion();
  console.log('Connection to cluster established:', url, version);
  return connection;
}

// ----------------------------------------------------------------------------- create swap

// so just to be clear - this is creating a new traing pool of tokens A vs B
// this is NOT creating a new exchange per se - the exchange is already deployed on-chain and we're interacting with it
// which leads to the realization that in this model each pool can have custom fees
// that are regulated by the exchange program (which runs validate_fees() to ensure they're ok)
async function createSwap() {
  const payer = owner; //the two are the same in our case
  const tokenSwapAccount = new Keypair(); //new Account() = old version

  [authority, nonce] = await PublicKey.findProgramAddress(
    [tokenSwapAccount.publicKey.toBuffer()], //seeds for the PDA
    TOKEN_SWAP_PROGRAM_ID, //owner of the PDA
  );

  // --------------------------------------- pool tokens

  console.log('creating pool mint');
  mintPool = await Token.createMint(
    connection,
    payer,
    authority,
    null,
    2,
    TOKEN_PROGRAM_ID,
  );

  console.log('creating pool & fee accounts');
  tokenAccountPool = await mintPool.createAccount(owner.publicKey); //creates an associated account
  feeAccount = await mintPool.createAccount(new PublicKey(SWAP_PROGRAM_OWNER_FEE_ADDRESS));

  // --------------------------------------- exchange A token

  console.log('creating token A');
  mintA = await Token.createMint(
    connection,
    payer,
    owner.publicKey,
    null,
    2,
    TOKEN_PROGRAM_ID,
  );

  console.log('creating EXCHANGE token A account');
  tokenAccountA = await mintA.createAccount(authority);
  //in this case we're simply minting the tokens into Token account A, in real life we'd of course send them
  await mintA.mintTo(tokenAccountA, owner, [], currentSwapTokenA);

  // --------------------------------------- exchange B token

  console.log('creating token B');
  mintB = await Token.createMint(
    connection,
    payer,
    owner.publicKey,
    null,
    2,
    TOKEN_PROGRAM_ID,
  );

  console.log('creating EXCHANGE token B account');
  tokenAccountB = await mintB.createAccount(authority);
  await mintB.mintTo(tokenAccountB, owner, [], currentSwapTokenB);

  // --------------------------------------- user A token
  console.log('Creating USER token a account');
  userAccountA = await mintA.createAccount(owner.publicKey);
  await mintA.mintTo(userAccountA, owner, [], SWAP_AMOUNT_IN*20); //enough to play with

  // --------------------------------------- user B token
  console.log('Creating USER token b account');
  userAccountB = await mintB.createAccount(owner.publicKey);
  await mintB.mintTo(userAccountB, owner, [], SWAP_AMOUNT_IN*20); //enough to play with

  // --------------------------------------- user pool token account
  console.log('Creating USER pool token account');
  userAccountPool = await mintPool.createAccount(owner.publicKey);

  // --------------------------------------- host
  console.log('Creating UI host account to accrue fees');
  hostPoolAccount = SWAP_PROGRAM_OWNER_FEE_ADDRESS
    ? await mintPool.createAccount(owner.publicKey)
    : null;

  // --------------------------------------- the swap itself

  console.log('creating token swap');
  tokenSwap = await TokenSwap.createTokenSwap(
    connection,
    payer,
    tokenSwapAccount,
    authority, //authority over the exchange itself and all the related accounts
    tokenAccountA,
    tokenAccountB,
    mintPool.publicKey,
    mintA.publicKey,
    mintB.publicKey,
    feeAccount,
    tokenAccountPool,
    TOKEN_SWAP_PROGRAM_ID,
    TOKEN_PROGRAM_ID,
    nonce,
    TRADING_FEE_NUMERATOR,
    TRADING_FEE_DENOMINATOR,
    OWNER_TRADING_FEE_NUMERATOR,
    OWNER_TRADING_FEE_DENOMINATOR,
    OWNER_WITHDRAW_FEE_NUMERATOR,
    OWNER_WITHDRAW_FEE_DENOMINATOR,
    HOST_FEE_NUMERATOR,
    HOST_FEE_DENOMINATOR,
    CURVE_TYPE,
  );

  console.log('loading token swap');
  const fetchedTokenSwap = await TokenSwap.loadTokenSwap(
    connection,
    tokenSwapAccount.publicKey,
    TOKEN_SWAP_PROGRAM_ID,
    payer,
  );
  console.log(fetchedTokenSwap)
}

// ----------------------------------------------------------------------------- swap

async function swap() {
  //create a temporary Keypair that will hold authority over given amount of tokens
  const userTransferAuthority = new Account();
  //approve it to spend a pre-determined amount
  await mintA.approve(
    userAccountA,
    userTransferAuthority.publicKey,
    owner,
    [],
    SWAP_AMOUNT_IN,
  );

  console.log('Swapping');
  await tokenSwap.swap(
    userAccountA,
    tokenAccountA,
    tokenAccountB,
    userAccountB,
    hostPoolAccount, //Host account to gather fees
    userTransferAuthority, //Account delegated to transfer user's tokens
    SWAP_AMOUNT_IN, //Amount to transfer from source account
    SWAP_AMOUNT_OUT, //Minimum amount of tokens the user will receive
  );
}

// ----------------------------------------------------------------------------- deposit both

// what's interesting is that in this calculatin we start with what % of the pool we want to represent as a user and then convert to A/B tokens
async function depositAllTokenTypes() {
  //     mintAuthority: null | PublicKey;
  //     supply: u64;
  //     decimals: number;
  //     isInitialized: boolean;
  //     freezeAuthority: null | PublicKey;
  const poolMintInfo = await mintPool.getMintInfo();
  const supply = poolMintInfo.supply.toNumber(); //total pool token in existence

  //     address: PublicKey;
  //     mint: PublicKey;
  //     owner: PublicKey;
  //     amount: u64;
  //     delegate: null | PublicKey;
  //     delegatedAmount: u64;
  //     isInitialized: boolean;
  //     isFrozen: boolean;
  //     isNative: boolean;
  //     rentExemptReserve: null | u64;
  //     closeAuthority: null | PublicKey;
  const swapTokenA = await mintA.getAccountInfo(tokenAccountA);
  const tokenA = Math.ceil(
    (swapTokenA.amount.toNumber() * POOL_TOKEN_AMOUNT) / supply, //how much pool we want to withdraw / total pool * A token
  );

  const swapTokenB = await mintB.getAccountInfo(tokenAccountB);
  const tokenB = Math.ceil(
    (swapTokenB.amount.toNumber() * POOL_TOKEN_AMOUNT) / supply, //how much pool we want to withdraw / total pool * B token
  );

  //transfer authority to move A and B tokens to exchange
  const userTransferAuthority = new Account();
  await mintA.approve(
    userAccountA,
    userTransferAuthority.publicKey,
    owner,
    [],
    tokenA*2, //amount of token A //had to make it *2 for constant price curve to work
  );
  await mintB.approve(
    userAccountB,
    userTransferAuthority.publicKey,
    owner,
    [],
    tokenB*2, //amount of token B //had to make it *2 for constant price curve to work
  );

  console.log('Depositing into swap');
  await tokenSwap.depositAllTokenTypes(
    userAccountA,
    userAccountB,
    userAccountPool,
    userTransferAuthority,
    POOL_TOKEN_AMOUNT,
    tokenA*2, //had to make it *2 for constant price curve to work
    tokenB*2, //had to make it *2 for constant price curve to work
  );

}

// ----------------------------------------------------------------------------- withdraw both

async function withdrawAllTokenTypes() {
  const poolMintInfo = await mintPool.getMintInfo();
  const supply = poolMintInfo.supply.toNumber();
  let swapTokenA = await mintA.getAccountInfo(tokenAccountA);
  let swapTokenB = await mintB.getAccountInfo(tokenAccountB);

  //calculate withdrawal fees and subtract them from the amount before calculating respective A and B tokens
  let feeAmount = 0;
  if (OWNER_WITHDRAW_FEE_NUMERATOR !== 0) {
    feeAmount = Math.floor(
      (POOL_TOKEN_AMOUNT * OWNER_WITHDRAW_FEE_NUMERATOR) /
        OWNER_WITHDRAW_FEE_DENOMINATOR,
    );
  }
  const poolTokenAmount = POOL_TOKEN_AMOUNT - feeAmount;

  const tokenA = Math.floor(
    (swapTokenA.amount.toNumber() * poolTokenAmount) / supply,
  );
  const tokenB = Math.floor(
    (swapTokenB.amount.toNumber() * poolTokenAmount) / supply,
  );

  const userTransferAuthority = new Account();
  await mintPool.approve(
    userAccountPool,
    userTransferAuthority.publicKey,
    owner,
    [],
    POOL_TOKEN_AMOUNT,
  );

  console.log('Withdrawing pool tokens for A and B tokens');
  await tokenSwap.withdrawAllTokenTypes(
    userAccountA,
    userAccountB,
    userAccountPool,
    userTransferAuthority,
    POOL_TOKEN_AMOUNT,
    tokenA/2, //had to make it /2 for constant price curve to work
    tokenB/2, //had to make it /2 for constant price curve to work
  );
}

// ----------------------------------------------------------------------------- deposit one side

async function depositSingleTokenTypeExactAmountIn() {
  // Pool token amount to deposit on one side
  const depositAmount = 10000;

  const poolMintInfo = await mintPool.getMintInfo();
  const supply = poolMintInfo.supply.toNumber();

  const swapTokenA = await mintA.getAccountInfo(tokenAccountA);
  const poolTokenA = tradingTokensToPoolTokens( //gives us the number of pool tokens we're requesting
    depositAmount, //how much of token A we want to deposit
    swapTokenA.amount.toNumber(), //how much of token A there is in exchange
    supply,
  );
  const swapTokenB = await mintB.getAccountInfo(tokenAccountB);
  const poolTokenB = tradingTokensToPoolTokens(
    depositAmount,
    swapTokenB.amount.toNumber(),
    supply,
  );

  //create an authority and let it control both A and B
  const userTransferAuthority = new Account();
  await mintA.approve(
    userAccountA,
    userTransferAuthority.publicKey,
    owner,
    [],
    depositAmount,
  );
  await mintB.approve(
    userAccountB,
    userTransferAuthority.publicKey,
    owner,
    [],
    depositAmount,
  );

  console.log('Depositing token A into swap');
  await tokenSwap.depositSingleTokenTypeExactAmountIn(
    userAccountA,
    userAccountPool,
    userTransferAuthority,
    depositAmount,
    poolTokenA/2, //had to make it /2 for constant price curve to work
  );

  // console.log('Depositing token B into swap');
  // await tokenSwap.depositSingleTokenTypeExactAmountIn(
  //   userAccountB,
  //   userAccountPool,
  //   userTransferAuthority,
  //   depositAmount,
  //   poolTokenB/2, //had to make it /2 for constant price curve to work
  // );

}

// ----------------------------------------------------------------------------- withdraw one side

async function withdrawSingleTokenTypeExactAmountOut() {
  // Pool token amount to withdraw on one side
  const withdrawAmount = 5000;
  const roundingAmount = 1.001; // make math a little easier

  const poolMintInfo = await mintPool.getMintInfo();
  const supply = poolMintInfo.supply.toNumber();

  const swapTokenA = await mintA.getAccountInfo(tokenAccountA);
  const swapTokenAPost = swapTokenA.amount.toNumber() - withdrawAmount;
  const poolTokenA = tradingTokensToPoolTokens(
    withdrawAmount,
    swapTokenAPost,
    supply,
  );
  let adjustedPoolTokenA = poolTokenA * roundingAmount;
  if (OWNER_WITHDRAW_FEE_NUMERATOR !== 0) {
    adjustedPoolTokenA *=
      1 + OWNER_WITHDRAW_FEE_NUMERATOR / OWNER_WITHDRAW_FEE_DENOMINATOR;
  }

  const swapTokenB = await mintB.getAccountInfo(tokenAccountB);
  const swapTokenBPost = swapTokenB.amount.toNumber() - withdrawAmount;
  const poolTokenB = tradingTokensToPoolTokens(
    withdrawAmount,
    swapTokenBPost,
    supply,
  );
  let adjustedPoolTokenB = poolTokenB * roundingAmount;
  if (OWNER_WITHDRAW_FEE_NUMERATOR !== 0) {
    adjustedPoolTokenB *=
      1 + OWNER_WITHDRAW_FEE_NUMERATOR / OWNER_WITHDRAW_FEE_DENOMINATOR;
  }

  const userTransferAuthority = new Account();
  await mintPool.approve(
    userAccountPool,
    userTransferAuthority.publicKey,
    owner,
    [],
    adjustedPoolTokenA + adjustedPoolTokenB,
  );

  console.log('Withdrawing token A only');
  await tokenSwap.withdrawSingleTokenTypeExactAmountOut(
    userAccountA,
    userAccountPool,
    userTransferAuthority,
    withdrawAmount,
    adjustedPoolTokenA*2, //had to make it *2 for constant price curve to work
  );

  console.log('Withdrawing token B only');
  await tokenSwap.withdrawSingleTokenTypeExactAmountOut(
    userAccountB,
    userAccountPool,
    userTransferAuthority,
    withdrawAmount,
    adjustedPoolTokenB*2, //had to make it *2 for constant price curve to work
  );
}

// ----------------------------------------------------------------------------- helpers

function tradingTokensToPoolTokens(
  sourceAmount,
  swapSourceAmount,
  poolAmount,
) {
  const tradingFee =
    (sourceAmount / 2) * (TRADING_FEE_NUMERATOR / TRADING_FEE_DENOMINATOR);
  const sourceAmountPostFee = sourceAmount - tradingFee;
  const root = Math.sqrt(sourceAmountPostFee / swapSourceAmount + 1);
  return Math.floor(poolAmount * (root - 1));
}

async function printAllBalances() {
  // user
  let userABalance = await connection.getTokenAccountBalance(userAccountA);
  let userBBalance = await connection.getTokenAccountBalance(userAccountB);
  let userPoolBalance = await connection.getTokenAccountBalance(userAccountPool);
  // exchange
  let exchangeABalance = await connection.getTokenAccountBalance(tokenAccountA);
  let exchangeBBalance = await connection.getTokenAccountBalance(tokenAccountB);
  let tokenAccountPoolBalance = await connection.getTokenAccountBalance(tokenAccountPool);
  let feeAccountBalance = await connection.getTokenAccountBalance(feeAccount);
  // host
  let hostPoolAccountBalance = await connection.getTokenAccountBalance(hostPoolAccount);

  console.log('// -----------------------------------------------------------------------------')
  console.log('User token A: ', userABalance.value.uiAmount)
  console.log('User token B: ', userBBalance.value.uiAmount)
  console.log('User pool token account: ', userPoolBalance.value.uiAmount)
  console.log('Exchange token A: ', exchangeABalance.value.uiAmount)
  console.log('Exchange token B: ', exchangeBBalance.value.uiAmount)
  console.log('Exchange pool token account: ', tokenAccountPoolBalance.value.uiAmount)
  console.log('Exchange fee account: ', feeAccountBalance.value.uiAmount)
  console.log('Host fee account: ', hostPoolAccountBalance.value.uiAmount)
  console.log('// -----------------------------------------------------------------------------')
}

// ----------------------------------------------------------------------------- play

async function play() {
  await getConnection();
  await createSwap();
  await printAllBalances();
  // await swap();
  // await printAllBalances();
  // await depositAllTokenTypes();
  // await printAllBalances();
  // await withdrawAllTokenTypes();
  // await printAllBalances();
  await depositSingleTokenTypeExactAmountIn();
  await printAllBalances();
  // await withdrawSingleTokenTypeExactAmountOut();
  // await printAllBalances();
}

play()