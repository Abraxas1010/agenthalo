import { createServer } from 'node:http';
import WDK from '@tetherto/wdk';
import WalletManagerBtc, { ElectrumTcp } from '@tetherto/wdk-wallet-btc';
import WalletManagerEvm from '@tetherto/wdk-wallet-evm';

const PORT = parseInt(process.env.WDK_PORT || '7321', 10);
const HOST = '127.0.0.1';

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
let seedPhrase = null;
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
  const fallback = 'abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about';
  return fallback;
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

  if (chain === 'ethereum') return '0x0000000000000000000000000000000000000000';
  return '';
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

async function handleInit(req, res) {
  const body = await parseBody(req);
  const requested = String(body.seed || '').trim();
  const generate = !!body.generate;
  if (initialized) {
    json(res, 200, { ok: true, initialized: true, message: 'already initialized' });
    return;
  }

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
    seedPhrase = null;
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
  json(res, 200, {
    accounts: [
      { chain: 'bitcoin', label: 'Bitcoin', symbol: 'BTC', address: btcAddress || '' },
      { chain: 'ethereum', label: 'Ethereum', symbol: 'ETH', address: ethAddress || '' },
      { chain: 'polygon', label: 'Polygon', symbol: 'MATIC', address: ethAddress || '' },
      { chain: 'arbitrum', label: 'Arbitrum', symbol: 'ETH', address: ethAddress || '' }
    ]
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
  json(res, 200, {
    ok: true,
    quote: {
      chain: body.chain || 'unknown',
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
  if (!chain || !to || !amount) {
    json(res, 400, { error: 'chain, to, and amount are required' });
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
  seedPhrase = null;
  wdk = null;
  btcWallet = null;
  evmWallet = null;
}

const server = createServer(async (req, res) => {
  try {
    if (req.socket.remoteAddress !== '127.0.0.1' && req.socket.remoteAddress !== '::1') {
      json(res, 403, { error: 'forbidden' });
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
