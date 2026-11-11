// Small side-by-side benchmark through @arkiv-network/sdk 1.0.1.
// Run from a checkout of arkiv-sdk-js: copy into its test/ directory (it resolves the SDK from there)
// and `bun run sdk-side-by-side-bench.ts`. See docs/benchmarks/2026-11-17-sdk-side-by-side.md.
// Env: RPC_URL, PRIVATE_KEY, LABEL, ENTITIES (2000), BATCH (50), WAVE (1), READS (300), CONC (16).
import { createPublicClient, createWalletClient, ExpirationTime } from "@arkiv-network/sdk"
import { localhost } from "@arkiv-network/sdk/chains"
import { and, desc, eq, gte, lt } from "@arkiv-network/sdk/query"
import { bytes, defineSchema, str, u64 } from "@arkiv-network/sdk/schema"
import { defineChain, type Hex, http } from "viem"
import { privateKeyToAccount } from "viem/accounts"

const url = process.env.RPC_URL!
const N = Number(process.env.ENTITIES ?? 2000)
const BATCH = Number(process.env.BATCH ?? 50)
const READS = Number(process.env.READS ?? 300)
const CONC = Number(process.env.CONC ?? 16)
const label = process.env.LABEL ?? url

const id = await createPublicClient({ chain: localhost, transport: http(url) }).getChainId()
const chain = defineChain({
  id,
  name: "bench",
  nativeCurrency: { decimals: 18, name: "Ether", symbol: "ETH" },
  rpcUrls: { default: { http: [url] } },
})
// viem polls for receipts every 4 s by default, which would hide the node's own latency.
const pub = createPublicClient({ chain, transport: http(), pollingInterval: 50 })
const wallet = createWalletClient({
  chain,
  transport: http(),
  pollingInterval: 50,
  account: privateKeyToAccount(process.env.PRIVATE_KEY as Hex),
})

const Item = defineSchema({
  run: str().indexed(),
  category: str().indexed(),
  score: u64().indexed(),
  title: str().indexed(),
  body: bytes().optional(),
})
const run = `bench-${Date.now()}`
const body = new Uint8Array(256).fill(7)
const pct = (xs: number[], p: number) => {
  const s = [...xs].sort((a, b) => a - b)
  return s[Math.min(s.length - 1, Math.floor((p / 100) * s.length))] ?? 0
}
const ms = (x: number) => x.toFixed(1)
const out: Record<string, unknown> = { label, entities: N, batch: BATCH }

// ---- writes: N entities in batches of BATCH, WAVE transactions in flight at a time ----
const WAVE = Number(process.env.WAVE ?? 1)
// SAME_LIFETIME=1 gives every entity ExpirationTime.fromHours(2), as applications do. Without it each
// entity gets its own expiry height, which is what the first run needed: the node then admitted
// at most 16 entities expiring at one block and failed the rest.
const SAME_LIFETIME = process.env.SAME_LIFETIME === "1"
const LIFETIME_BLOCKS = Number(process.env.LIFETIME_BLOCKS ?? 30000)
const keys: Hex[] = []
const txMs: number[] = []
let gas = 0n
let nonce = await pub.getTransactionCount({ address: wallet.account.address, blockTag: "pending" })
const first = await pub.getBlockNumber()
const t0 = performance.now()
for (let done = 0; done < N; ) {
  const wave: Promise<void>[] = []
  for (let w = 0; w < WAVE && done < N; w++) {
    const size = Math.min(BATCH, N - done)
    const start = done
    done += size
    const t = performance.now()
    wave.push(
      wallet
        .executeBatch(
          {
            creates: Array.from({ length: size }, (_, i) => {
              const n = start + i
              return {
                schema: Item,
                values: {
                  run,
                  category: `cat-${n % 10}`,
                  score: u64(BigInt(n)),
                  title: `item ${String(n).padStart(6, "0")}`,
                  body,
                },
                expiration: SAME_LIFETIME ? ExpirationTime.fromHours(2) : ExpirationTime.fromBlocks(LIFETIME_BLOCKS + n),
              }
            }),
          },
          { nonce: nonce++ },
        )
        .then(async (res) => {
          const receipt = await pub.waitForTransactionReceipt({ hash: res.txHash, pollingInterval: 50 })
          if (receipt.status !== "success") throw new Error(`batch at ${start} reverted`)
          gas += receipt.gasUsed
          txMs.push(performance.now() - t)
          keys.push(...res.createdEntities)
        }),
    )
  }
  await Promise.all(wave)
}
const writeS = (performance.now() - t0) / 1000
const blocks = Number((await pub.getBlockNumber()) - first)
out.write = {
  wave: WAVE,
  seconds: +writeS.toFixed(1),
  blocks,
  entitiesPerSecond: +(N / writeS).toFixed(1),
  entitiesPerBlock: +(N / Math.max(1, blocks)).toFixed(1),
  txLatencyMsP50: +ms(pct(txMs, 50)),
  txLatencyMsP95: +ms(pct(txMs, 95)),
  gasPerEntity: Number(gas / BigInt(N)),
}

// ---- reads ----
const byRun = eq("run", str(run))
const reads: Record<string, (i: number) => Promise<number>> = {
  getEntity: async (i) => ((await pub.getEntity(keys[(i * 7919) % keys.length]!, Item)) ? 1 : 0),
  "eq, limit 100": async (i) =>
    (await pub.select("*").from(Item).where(byRun, eq("category", str(`cat-${i % 10}`))).limit(100).fetch())
      .entities.length,
  "range, limit 100": async (i) => {
    const lo = BigInt((i * 37) % Math.max(1, N - 100))
    return (
      await pub.select("*").from(Item).where(and(byRun, gte("score", u64(lo)), lt("score", u64(lo + 100n)))).limit(100).fetch()
    ).entities.length
  },
  "count all": async () => Number(await pub.count().from(Item).where(byRun).fetch()),
  "order by score desc, limit 20": async () =>
    (await pub.select("*").from(Item).where(byRun).orderBy(desc("score")).limit(20).fetch()).entities.length,
  "offset N-50, limit 50": async () =>
    (await pub.select("*").from(Item).where(byRun).limit(50).offset(N - 50).fetch()).entities.length,
}
const readOut: Record<string, unknown> = {}
for (const [name, fn] of Object.entries(reads)) {
  const rows = await fn(0) // warm-up, and the row count as a sanity check
  const lat: number[] = []
  for (let i = 0; i < READS; i++) {
    const t = performance.now()
    await fn(i)
    lat.push(performance.now() - t)
  }
  const tc = performance.now()
  let next = 0
  await Promise.all(
    Array.from({ length: CONC }, async () => {
      while (next < READS) await fn(next++)
    }),
  )
  const concS = (performance.now() - tc) / 1000
  readOut[name] = {
    rows,
    p50Ms: +ms(pct(lat, 50)),
    p95Ms: +ms(pct(lat, 95)),
    [`reqPerSecAt${CONC}`]: +(READS / concS).toFixed(0),
  }
}
out.read = readOut
console.log(JSON.stringify(out, null, 1))
