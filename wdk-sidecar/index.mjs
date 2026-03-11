import { createServer } from 'node:http';
import WDK from '@tetherto/wdk';
import WalletManagerBtc, { ElectrumTcp } from '@tetherto/wdk-wallet-btc';
import WalletManagerEvm from '@tetherto/wdk-wallet-evm';

const PORT = parseInt(process.env.WDK_PORT || '7321', 10);
const HOST = '127.0.0.1';
const AUTH_TOKEN = String(process.env.WDK_AUTH_TOKEN || '').trim();
const AUTH_HEADER = 'x-agenthalo-wdk-token';

const EVM_CHAINS = {
  ethereum: {
    provider: process.env.ETH_RPC || 'https://eth.drpc.org',
    label: 'Ethereum',
    symbol: 'ETH',
  },
  polygon: {
    provider: process.env.POLYGON_RPC || 'https://polygon.drpc.org',
    label: 'Polygon',
    symbol: 'MATIC',
  },
  arbitrum: {
    provider: process.env.ARBITRUM_RPC || 'https://arbitrum.drpc.org',
    label: 'Arbitrum',
    symbol: 'ETH',
  }
};

const BTC_CONFIG = {
  host: process.env.BTC_ELECTRUM_HOST || 'electrum.blockstream.info',
  port: parseInt(process.env.BTC_ELECTRUM_PORT || '50001', 10),
  network: process.env.BTC_NETWORK || 'bitcoin'
};

let initialized = false;
let wdk = null;
let btcWallet = null;
let evmWallet = null;

function json(res, status, body) {
  res.writeHead(status, { 'Content-Type': 'application/json' });
  res.end(JSON.stringify(body));
}

function parseBody(req) {
  return new Promise((resolve, reject) => {
    const chunks = [];
    req.on('data', c => chunks.push(c));
    req.on('end', () => {
      if (!chunks.length) {
        resolve({});
        return;
      }
      try {
        resolve(JSON.parse(Buffer.concat(chunks).toString('utf8')));
      } catch (_e) {
        resolve({});
      }
    });
    req.on('error', reject);
  });
}

function getRandomSeedPhrase() {
  if (WDK && typeof WDK.getRandomSeedPhrase === 'function') {
    return WDK.getRandomSeedPhrase(24);
  }
  throw new Error('WDK.getRandomSeedPhrase unavailable; refusing unsafe fallback');
}

function shortError(e) {
  return e && e.message ? String(e.message) : String(e);
}

async function maybeCall(obj, names, ...args) {
  if (!obj) return null;
  for (const name of names) {
    const fn = obj[name];
    if (typeof fn === 'function') {
      try {
        return await fn.apply(obj, args);
      } catch (_e) {
      }
    }
  }
  return null;
}

async function extractAddress(wallet, chain) {
  if (!wallet) return null;
  const direct = await maybeCall(wallet, ['getAddress', 'address', 'receiveAddress']);
  if (typeof direct === 'string' && direct) return direct;

  const account = await maybeCall(wallet, ['getAccount', 'account'], 0);
  if (account) {
    const fromAccount = await maybeCall(account, ['getAddress', 'address', 'receiveAddress']);
    if (typeof fromAccount === 'string' && fromAccount) return fromAccount;
  }

  return null;
}

async function extractBalance(wallet) {
  if (!wallet) return { balance: '0', decimals: 0 };
  const b = await maybeCall(wallet, ['getBalance', 'balance']);
  if (b == null) return { balance: '0', decimals: 0 };
  if (typeof b === 'string' || typeof b === 'number' || typeof b === 'bigint') {
    return { balance: String(b), decimals: 0 };
  }
  if (typeof b === 'object') {
    return {
      balance: String(b.balance ?? b.value ?? b.amount ?? '0'),
      decimals: Number(b.decimals ?? 0)
    };
  }
  return { balance: '0', decimals: 0 };
}

async function initWallets(seed) {
  wdk = new WDK(seed);

  const btcClient = new ElectrumTcp({
    host: BTC_CONFIG.host,
    port: BTC_CONFIG.port
  });
  btcWallet = new WalletManagerBtc(seed, {
    client: btcClient,
    network: BTC_CONFIG.network
  });

  evmWallet = new WalletManagerEvm(seed, {
    provider: EVM_CHAINS.ethereum.provider
  });
}

function isSupportedChain(chain) {
  return chain === 'bitcoin' || chain === 'ethereum' || chain === 'polygon' || chain === 'arbitrum';
}

function isHex40(value) {
  return typeof value === 'string' && /^[0-9a-fA-F]{40}$/.test(value);
}

function isLikelyBitcoinAddress(value) {
  if (typeof value !== 'string') return false;
  const addr = value.trim();
  if (!addr) return false;
  const bech32 = addr.startsWith('bc1') || addr.startsWith('tb1');
  const legacy = /^[123mn2][A-Za-z0-9]{25,89}$/.test(addr);
  return bech32 || legacy;
}

function isPositiveIntegerString(value) {
  if (typeof value !== 'string') return false;
  if (!/^[0-9]{1,80}$/.test(value.trim())) return false;
  try {
    return BigInt(value.trim()) > 0n;
  } catch (_e) {
    return false;
  }
}

function validateTransferInput(chain, to, amount) {
  if (!isSupportedChain(chain)) return `unsupported chain: ${chain}`;
  if (!isPositiveIntegerString(amount)) return 'amount must be a positive integer string';
  if (chain === 'bitcoin' && !isLikelyBitcoinAddress(to)) {
    return 'invalid bitcoin address';
  }
  if (chain !== 'bitcoin') {
    if (!String(to).startsWith('0x') || !isHex40(String(to).slice(2))) {
      return `invalid ${chain} address`;
    }
  }
  return null;
}

function isAuthorized(req) {
  if (!AUTH_TOKEN) return false;
  const token = String(req.headers[AUTH_HEADER] || '').trim();
  return token === AUTH_TOKEN;
}

async function handleInit(req, res) {
  const body = await parseBody(req);
  const requested = String(body.seed || '').trim();
  const generate = !!body.generate;
  if (initialized) {
    json(res, 200, { ok: true, initialized: true, message: 'already initialized' });
    return;
  }

  let seedPhrase = null;
  if (generate) {
    seedPhrase = getRandomSeedPhrase();
  } else if (requested) {
    seedPhrase = requested;
  } else {
    json(res, 400, { error: 'provide seed or set generate=true' });
    return;
  }

  try {
    await initWallets(seedPhrase);
    initialized = true;
    const payload = { ok: true, initialized: true };
    if (generate) payload.seed = seedPhrase;
    json(res, 200, payload);
  } catch (e) {
    initialized = false;
    wdk = null;
    btcWallet = null;
    evmWallet = null;
    json(res, 500, { error: `WDK init failed: ${shortError(e)}` });
  }
}

async function requireInit(res) {
  if (!initialized) {
    json(res, 400, { error: 'wallet not initialized' });
    return false;
  }
  return true;
}

async function handleAccounts(res) {
  if (!(await requireInit(res))) return;
  const btcAddress = await extractAddress(btcWallet, 'bitcoin');
  const ethAddress = await extractAddress(evmWallet, 'ethereum');
  const errors = [];
  if (!btcAddress) errors.push('bitcoin address unavailable');
  if (!ethAddress) errors.push('evm address unavailable');
  json(res, 200, {
    accounts: [
      { chain: 'bitcoin', label: 'Bitcoin', symbol: 'BTC', address: btcAddress || '' },
      { chain: 'ethereum', label: 'Ethereum', symbol: 'ETH', address: ethAddress || '' },
      { chain: 'polygon', label: 'Polygon', symbol: 'MATIC', address: ethAddress || '' },
      { chain: 'arbitrum', label: 'Arbitrum', symbol: 'ETH', address: ethAddress || '' }
    ],
    address_errors: errors
  });
}

async function handleBalances(res) {
  if (!(await requireInit(res))) return;
  const btc = await extractBalance(btcWallet);
  const evm = await extractBalance(evmWallet);
  json(res, 200, {
    balances: [
      { chain: 'bitcoin', symbol: 'BTC', balance: btc.balance, decimals: btc.decimals },
      { chain: 'ethereum', symbol: 'ETH', balance: evm.balance, decimals: evm.decimals },
      { chain: 'polygon', symbol: 'MATIC', balance: evm.balance, decimals: evm.decimals },
      { chain: 'arbitrum', symbol: 'ETH', balance: evm.balance, decimals: evm.decimals }
    ]
  });
}

async function handleQuote(req, res) {
  if (!(await requireInit(res))) return;
  const body = await parseBody(req);
  const chain = String(body.chain || '').trim().toLowerCase();
  const to = String(body.to || '').trim();
  const amount = String(body.amount || '').trim();
  const validationError = validateTransferInput(chain, to, amount);
  if (validationError) {
    json(res, 400, { error: validationError });
    return;
  }
  json(res, 200, {
    ok: true,
    quote: {
      chain,
      fee: '0',
      note: 'fee estimation passthrough is pending WDK per-chain quote adapters'
    }
  });
}

async function handleSend(req, res) {
  if (!(await requireInit(res))) return;
  const body = await parseBody(req);
  const chain = String(body.chain || '').trim().toLowerCase();
  const to = String(body.to || '').trim();
  const amount = String(body.amount || '').trim();
  const validationError = validateTransferInput(chain, to, amount);
  if (validationError) {
    json(res, 400, { error: validationError });
    return;
  }

  const tx = await maybeCall(
    chain === 'bitcoin' ? btcWallet : evmWallet,
    ['sendTransaction', 'send'],
    { to, amount }
  );
  if (tx) {
    json(res, 200, { ok: true, hash: String(tx.hash || tx.txid || tx.transactionHash || '') });
    return;
  }
  json(res, 501, { ok: false, error: 'send not implemented by active WDK adapters yet' });
}

async function handleFees(res) {
  if (!(await requireInit(res))) return;
  json(res, 200, {
    ok: true,
    fees: {
      bitcoin: { fast: null, normal: null, slow: null },
      ethereum: { gasPriceWei: null },
      polygon: { gasPriceWei: null },
      arbitrum: { gasPriceWei: null }
    }
  });
}

function destroy() {
  initialized = false;
  wdk = null;
  btcWallet = null;
  evmWallet = null;
}

const server = createServer(async (req, res) => {
  try {
    const remote = req.socket.remoteAddress;
    if (remote !== '127.0.0.1' && remote !== '::1' && remote !== '::ffff:127.0.0.1') {
      json(res, 403, { error: 'forbidden' });
      return;
    }
    if (!isAuthorized(req)) {
      json(res, 401, { error: 'unauthorized sidecar request' });
      return;
    }

    const method = req.method || 'GET';
    const url = req.url || '/';

    if (method === 'GET' && url === '/status') {
      json(res, 200, {
        ok: true,
        initialized,
        chains: ['bitcoin', 'ethereum', 'polygon', 'arbitrum']
      });
      return;
    }
    if (method === 'POST' && url === '/init') {
      await handleInit(req, res);
      return;
    }
    if (method === 'GET' && url === '/accounts') {
      await handleAccounts(res);
      return;
    }
    if (method === 'GET' && url === '/balances') {
      await handleBalances(res);
      return;
    }
    if (method === 'POST' && url === '/quote') {
      await handleQuote(req, res);
      return;
    }
    if (method === 'POST' && url === '/send') {
      await handleSend(req, res);
      return;
    }
    if (method === 'GET' && url === '/fees') {
      await handleFees(res);
      return;
    }
    if (method === 'POST' && url === '/destroy') {
      destroy();
      json(res, 200, { ok: true, initialized: false });
      return;
    }

    json(res, 404, { error: 'not found' });
  } catch (e) {
    json(res, 500, { error: shortError(e) });
  }
});

process.on('SIGTERM', () => {
  destroy();
  server.close(() => process.exit(0));
});
process.on('SIGINT', () => {
  destroy();
  server.close(() => process.exit(0));
});

server.listen(PORT, HOST, () => {
  console.log(`[agenthalo-wdk] listening on http://${HOST}:${PORT}`);
});
