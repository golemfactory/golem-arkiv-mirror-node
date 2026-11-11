/** Runs SDK 1.0.x (@arkiv-network/sdk, user-defined columns) against an isolated native chain. */
import assert from "node:assert/strict"
import { readFileSync, writeFileSync } from "node:fs"
import { createPublicClient, createWalletClient, ExpirationTime, MAX_LIFETIME } from "@arkiv-network/sdk"
import { and, asc, between, desc, eq, gt } from "@arkiv-network/sdk/query"
import { addr, bytes, dec, defineSchema, str, u64 } from "@arkiv-network/sdk/schema"
import { bytesToHex, http } from "viem"
import { privateKeyToAccount } from "viem/accounts"

const url = process.env.SDK_RPC_URL!
const chain = {
  id: 9405,
  name: "UDC smoke",
  nativeCurrency: { name: "GLM", symbol: "GLM", decimals: 18 },
  rpcUrls: { default: { http: [url] } },
}
const transport = http(url, { retryCount: 0, timeout: 20_000 })
const alice = privateKeyToAccount(`0x${"02".repeat(32)}`)
const read = createPublicClient({ chain, transport, pollingInterval: 100 })
const write = createWalletClient({ chain, transport, account: alice, pollingInterval: 100 })
const expiration = ExpirationTime.fromBlocks(5000)

const Posts = defineSchema("posts", {
  author: addr().indexed(),
  category: str().indexed(),
  score: dec().indexed(),
  views: u64().indexed(),
  title: str(),
  body: bytes(),
  thumbnail: bytes().optional(),
  mimeType: str().default("text/markdown"),
})

// The SDK network-health probe: a named schema with an optional empty body.
const Probe = defineSchema("probe", {
  purpose: str().indexed(),
  body: bytes().optional(),
})
// One bytes column at the 128 KiB per-entity cap.
const Blob = defineSchema("blob", { body: bytes() })
const maxBody = new Uint8Array(128 * 1024).map((_, i) => (i * 7) % 256)
const GLM = 10n ** 18n

const rpc = (q: string, opts: Record<string, unknown>) =>
  read.request({ method: "arkiv_query", params: [q, opts] } as any) as Promise<any>
const step = (name: string) => console.log("ok:", name)
const report = process.env.SDK_REPORT_FILE!
const stage = process.argv[2] ?? "exercise"

// The five `q` posts the paging steps read, in creation order or sorted.
const pages = (size: number, offset: number) =>
  read
    .select({ $key: true, views: true })
    .from(Posts)
    .where(eq("category", "q"))
    .withCount()
    .limit(size)
    .offset(offset)
    .fetch()
// biome-ignore lint/suspicious/noExplicitAny: an asc() or desc() key, checked by the SDK
const sortedBy = async (order: any) => {
  const keys: string[] = []
  for (let offset = 0; offset < 6; offset += 2) {
    const window = await qPosts().orderBy(order).withCount().limit(2).offset(offset).fetch()
    assert.equal(window.count, 5n)
    keys.push(...window.entities.map((e) => e.$key))
  }
  return keys
}
function qPosts() {
  return read.select({ $key: true, views: true, title: true }).from(Posts).where(eq("category", "q"))
}

if (stage !== "exercise") {
  // After a three-role restart (reopen) and through the replicated full node (follower).
  const { entityKey, bigKey, longKeys, maxKey, qKeys } = JSON.parse(readFileSync(report, "utf8"))
  for (const [key, expiresAt] of Object.entries(longKeys as Record<string, string>)) {
    assert.equal((await read.getEntity(key as `0x${string}`)).$expiresAt, BigInt(expiresAt))
  }
  assert.equal((await read.getEntity(maxKey, Blob)).body.value, bytesToHex(maxBody))
  const row = await read.getEntity(entityKey, Posts)
  assert.equal(row.title.value, "Patched")
  assert.equal(row.score.value, "4.9")
  assert.equal(row.thumbnail, undefined)
  const big = await read.getEntity(bigKey, Posts)
  assert.equal(big.body.value.length, 2 + 2 * 100 * 1024)
  const page = await read.select({ $key: true }).from(Posts).where(gt("score", dec("4.8"))).fetch()
  assert.deepEqual(
    page.entities.map((e) => e.$key),
    [entityKey],
  )
  const walked = []
  for (let offset = 0; offset < 6; offset += 2) {
    const window = await pages(2, offset)
    assert.equal(window.count, 5n)
    walked.push(...window.entities.map((e) => e.$key))
  }
  assert.deepEqual(walked, qKeys)
  assert.equal(await read.count().from(Posts).where(eq("category", "q")).fetch(), 5n)
  // Views 1, 4, 1, 5, 3 by then: DESC, ties in creation order.
  assert.deepEqual(await sortedBy(desc("views")), [3, 1, 4, 0, 2].map((i) => qKeys[i]))
  console.log(`UDC SMOKE ${stage.toUpperCase()} PASSED`)
  process.exit(0)
}

const body = new Uint8Array(2048).map((_, i) => i % 251)
const { entityKey } = await write.createEntity(
  Posts,
  {
    author: addr(alice.address),
    category: "docs",
    score: dec("4.5"),
    views: u64(7n),
    title: "Hello UDC",
    body,
  },
  { expiration },
)
step(`create ${entityKey}`)

const row = await read.getEntity(entityKey, Posts)
assert.equal(row.title.value, "Hello UDC")
assert.equal(row.category.value, "docs")
assert.equal(row.mimeType.value, "text/markdown")
assert.equal(row.views.value, 7n)
assert.equal(row.body.value.toLowerCase(), bytesToHex(body))
assert.equal(row.author.value.toLowerCase(), alice.address.toLowerCase())
step("typed getEntity round-trips every column, 2 KiB bytes included")

const page = await read
  .select({ $key: true, title: true, $attributeSchema: true })
  .from(Posts)
  .where(and(eq("category", "docs"), gt("score", dec("4.0"))))
  .limit(10)
  .fetch()
assert.equal(page.entities.length, 1)
assert.equal(page.entities[0].$key, entityKey)
const schema = page.entities[0].$attributeSchema as Record<string, { type: string; indexed?: boolean }>
assert.deepEqual(
  Object.fromEntries(Object.entries(schema).map(([n, s]) => [n, [s.type, s.indexed]])),
  {
    author: ["addr", true],
    category: ["str", true],
    score: ["dec", true],
    views: ["u64", true],
    title: ["str", false],
    body: ["bytes", false],
    mimeType: ["str", false],
  },
)
step("typed query on indexed columns; attributeSchema reports indexed flags")

const raw = await rpc(`arkiv.schema = str('posts')`, {
  select: { key: true, attributeSchema: true },
})
const tag = raw.data[0].attributeSchema.find((a: any) => a.name === "arkiv.schema")
assert.deepEqual(tag, { name: "arkiv.schema", type: "str", indexed: true })
step("arkiv.schema tag stored as an indexed str")

const unindexed = await rpc(`title = str('Hello UDC')`, { select: { key: true } })
assert.equal(unindexed.data.length, 0)
step("unindexed column is not matched by a predicate")

for (const bad of [{ payload: true }, { contentType: true }]) {
  await assert.rejects(rpc(`$key = key(${entityKey})`, { select: bad }))
}
step("select rejects payload and contentType")

await write.patchEntity(Posts, entityKey, {
  set: { score: dec("4.9"), title: "Patched", thumbnail: new Uint8Array([1, 2, 3]) },
})
let after = await read.getEntity(entityKey, Posts)
assert.equal(after.score.value, "4.9")
assert.equal(after.title.value, "Patched")
assert.equal(after.thumbnail?.value, "0x010203")
const byScore = await read.select({ $key: true }).from(Posts).where(gt("score", dec("4.8"))).fetch()
assert.equal(byScore.entities.length, 1)
step("patch updates indexed and unindexed columns; index follows the patch")

await write.patchEntity(Posts, entityKey, { unset: ["thumbnail"] })
after = await read.getEntity(entityKey, Posts)
assert.equal(after.thumbnail, undefined)
step("unset removes an optional bytes column")

// Node-side per-entity cap: 100 KiB body, then a 100 KiB thumbnail patch (each op is
// within the SDK's per-operation budget; the resulting entity is not).
const big = await write.createEntity(
  Posts,
  {
    author: addr(alice.address),
    category: "big",
    score: dec("1"),
    views: u64(0n),
    title: "big",
    body: new Uint8Array(100 * 1024).fill(7),
  },
  { expiration },
)
step("create with a 100 KiB bytes column")
await assert.rejects(
  write.patchEntity(Posts, big.entityKey, {
    set: { thumbnail: new Uint8Array(100 * 1024).fill(9) },
  }),
)
const bigRow = await read.getEntity(big.entityKey, Posts)
assert.equal(bigRow.thumbnail, undefined)
step("node rejects a patch that takes the entity past 128 KiB of bytes; state unchanged")

const untyped = await read.getEntity(entityKey)
assert.equal((untyped.$attributes as any)["arkiv.schema"]?.value, "posts")
step("untyped getEntity returns every attribute")

// The longest lifetime SDK 1.0.x sends: ExpirationTime.max(), MAX_LIFETIME
// blocks from the landing block. SDK 1.0.x cannot write permanent entities.
const head = async () => BigInt(await read.getBlockNumber())
const long = await write.createEntity(Probe, { purpose: "max_lifetime" }, { expiration: ExpirationTime.max() })
const longReceipt = await read.getTransactionReceipt({ hash: long.txHash })
assert.equal(long.expiresAt, longReceipt.blockNumber + MAX_LIFETIME)
assert.equal((await read.getEntity(long.entityKey)).$expiresAt, long.expiresAt)
step(`ExpirationTime.max(): expiresAt is the landing block + ${MAX_LIFETIME}`)

await assert.rejects(write.extendEntity({ entityKey: long.entityKey, expiration: ExpirationTime.fromBlocks(10) }))
const again = await write.extendEntity({ entityKey: long.entityKey, expiration: ExpirationTime.max() })
assert.ok(again.expiresAt > long.expiresAt)
assert.equal((await read.getEntity(long.entityKey)).$expiresAt, again.expiresAt)
step("extending with max() pushes the expiry out again; a shorter one is refused")

// Rent is GLM outside the gas budget: a 128 KiB create for MAX_LIFETIME blocks is one
// SDK call whose gas is execution only; the receipt's rentFee is its rent.
const call = (method: string, params: unknown[]) =>
  read.request({ method, params } as any) as Promise<any>
const balanceOf = async (who: string) => BigInt(await call("eth_getBalance", [who, "latest"]))
const nonceOf = async (who: string) => BigInt(await call("eth_getTransactionCount", [who, "latest"]))
const aliceBefore = await balanceOf(alice.address)
const max = await write.createEntity(Blob, { body: maxBody }, { expiration: ExpirationTime.max() })
const maxReceipt = (await read.getTransactionReceipt({ hash: max.txHash })) as any
assert.equal(max.expiresAt, maxReceipt.blockNumber + MAX_LIFETIME)
const rentFee = BigInt(maxReceipt.rentFee)
// MAX_LIFETIME blocks of 500 ms are 45.625 days: an eighth of the year of
// rent a full 128 KiB entity costs (0.9 to 1 GLM).
assert.ok(rentFee > (9n * GLM) / 80n && rentFee < GLM / 8n, `rent ${rentFee}`)
assert.ok(maxReceipt.gasUsed < 1_000_000n, `gas ${maxReceipt.gasUsed}`)
assert.equal(
  aliceBefore - (await balanceOf(alice.address)),
  maxReceipt.gasUsed * maxReceipt.effectiveGasPrice + rentFee,
)
assert.equal((await read.getEntity(max.entityKey, Blob)).body.value, bytesToHex(maxBody))
step(
  `128 KiB create for MAX_LIFETIME blocks in one call: ${maxReceipt.gasUsed} gas, rentFee ${rentFee} wei, both debited`,
)

// A sender holding the gas but not the rent is refused at admission, cleanly:
// no transaction, no nonce, no fee.
const describe = (e: any): string =>
  e ? [e.message, e.details, e.shortMessage, describe(e.cause)].filter(Boolean).join(" | ") : ""
const carol = privateKeyToAccount(`0x${"0c".repeat(32)}`)
const fund = async (value: bigint) =>
  read.waitForTransactionReceipt({
    hash: await (write as any).sendTransaction({ to: carol.address, value }),
  })
await fund(GLM / 100n)
const carolWrite = createWalletClient({ chain, transport, account: carol, pollingInterval: 100 })
let refusal = ""
await assert.rejects(
  carolWrite.createEntity(Blob, { body: maxBody }, { expiration: ExpirationTime.max() }),
  (e: any) => {
    refusal = describe(e)
    return /insufficient funds for gas \* price \+ value \+ rent/.test(refusal) || assert.fail(refusal)
  },
)
assert.equal(await nonceOf(carol.address), 0n)
assert.equal(await balanceOf(carol.address), GLM / 100n)
const [summary] = refusal.split(" | ")
const cause = refusal.split(" | ").find((part) => part.includes("+ rent"))
step(`a sender with gas but not rent is refused at admission, no nonce or fee: ${summary} (${cause})`)
await fund(GLM)
const carols = await carolWrite.createEntity(
  Blob,
  { body: maxBody },
  { expiration: ExpirationTime.max() },
)
assert.ok(carols.expiresAt > (await head()))
assert.equal(await nonceOf(carol.address), 1n)
step("topped up with gas plus rent, the same call commits")

const status = (await (await fetch(`${url}/signed/v1/status`)).json()) as any
const gas = status.capabilities.gas

// A permissionless-extension entity takes anyone's extension, here up to the
// SDK's own MAX_LIFETIME, below this chain's one-year rent cap. Extending past
// that cap, or to permanence, is its owner's alone (PermanenceOwnerOnly), but
// SDK 1.0.x cannot ask for either: the Rust tests cover them.
const cap = BigInt(gas.maxRentLifetimeBlocks)
assert.ok(MAX_LIFETIME < cap)
const open = await write.createEntity(
  Probe,
  { purpose: "permissionless" },
  { expiration, flags: { permissionlessExtension: true } },
)
const farther = await carolWrite.extendEntity({
  entityKey: open.entityKey,
  expiration: ExpirationTime.fromBlocks(20_000),
})
assert.ok(farther.expiresAt > open.expiresAt)
const longest = await carolWrite.extendEntity({
  entityKey: open.entityKey,
  expiration: ExpirationTime.max(),
})
assert.ok(longest.expiresAt > farther.expiresAt)
assert.equal((await read.getEntity(open.entityKey)).$expiresAt, longest.expiresAt)
step(`anyone extends a permissionless entity, up to max(): ${longest.expiresAt}`)

// The per-block storage cap (200,000 units here): two full 128 KiB entities
// submitted together land in consecutive blocks, the second deferred, not
// dropped. The producer is paused while both enter the pool.
assert.equal(gas.maxNewStorageUnitsPerBlock, "200000")
// Native REST answers 429 while busy, before doing anything: retry that only.
const control = async (action: string) => {
  for (let tries = 0; ; tries++) {
    const response = await fetch(`${process.env.SDK_PRIVATE_URL}/signed/v1/control`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${process.env.ARKIV_PROPOSER_TOKEN}`,
      },
      body: JSON.stringify({ action }),
    })
    if (response.ok) return
    assert.ok(response.status === 429 && tries < 100, `${action}: ${response.status}`)
    await new Promise((resolve) => setTimeout(resolve, 100))
  }
}
const pendingNonce = async (who: string) =>
  BigInt(await call("eth_getTransactionCount", [who, "pending"]))
const bob = privateKeyToAccount(`0x${"03".repeat(32)}`)
const bobWrite = createWalletClient({ chain, transport, account: bob, pollingInterval: 100 })
const before = [await pendingNonce(alice.address), await pendingNonce(bob.address)]
await control("pause")
const pair = [write, bobWrite].map((client) =>
  client.createEntity(Blob, { body: maxBody }, { expiration }),
)
for (let tries = 0; ; tries++) {
  const now = [await pendingNonce(alice.address), await pendingNonce(bob.address)]
  if (now[0] > before[0] && now[1] > before[1]) break
  assert.ok(tries < 200, "both creates reach the pool")
  await new Promise((resolve) => setTimeout(resolve, 50))
}
await control("resume")
const landed = await Promise.all(
  (await Promise.all(pair)).map(async ({ txHash }) => {
    const receipt = await read.getTransactionReceipt({ hash: txHash })
    assert.equal(receipt.status, "success")
    return receipt.blockNumber
  }),
)
const [first, second] = landed.sort((a, b) => (a < b ? -1 : 1))
assert.equal(second - first, 1n, `blocks ${landed}`)
step(`storage cap: two 128 KiB creates sent together landed in blocks ${first} and ${second}`)
assert.equal(gas.permanentEntitiesSupported, true)
assert.equal(gas.rentRule, "glm-rent-v3")
assert.equal(gas.rentCharging, "sender-balance-outside-gas")
assert.equal(gas.rentReceiptField, "rentFee")
// A year in seconds; 63,072,000 blocks at this chain's 500 ms.
assert.equal(gas.maxRentLifetimeSeconds, "31536000")
assert.equal(gas.maxRentLifetimeBlocks, "63072000")
assert.equal(gas.rentBound, "sender-balance")
step(
  `status: rent ${gas.rentWeiPerStorageUnitDay} wei/unit/day outside gas, capped at ${gas.maxRentLifetimeSeconds} s`,
)

// SDK 1.0.x reads: limit/offset windows, withCount(), client.count(),
// BETWEEN and a range on a str column, through the schema.
const qKeys: string[] = []
for (const views of [3n, 1n, 4n, 1n, 5n]) {
  const created = await write.createEntity(
    Posts,
    {
      author: addr(alice.address),
      category: "q",
      score: dec("2"),
      views: u64(views),
      title: `views ${views}`,
      body: new Uint8Array([Number(views)]),
    },
    { expiration },
  )
  qKeys.push(created.entityKey)
}
const windows = []
for (let offset = 0; offset < 6; offset += 2) windows.push(await pages(2, offset))
assert.deepEqual(
  windows.map((w) => w.entities.length),
  [2, 2, 1],
)
assert.ok(windows.every((w) => w.count === 5n))
assert.deepEqual(
  windows.flatMap((w) => w.entities.map((e) => e.$key)),
  qKeys,
)
assert.equal((await pages(2, 5)).entities.length, 0)
assert.equal((await pages(2, 500)).count, 5n)
step("limit/offset windows walk the matches in creation order; withCount() counts them all")

assert.equal(await read.count().from(Posts).where(eq("category", "q")).fetch(), 5n)
assert.equal(await read.count().from(Posts).where(between("views", u64(1n), u64(3n))).fetch(), 3n)
assert.equal(await read.count().from(Posts).where(gt("category", "p")).fetch(), 5n)
const counted = await rpc(`arkiv.schema = str('posts')`, { select: { count: true } })
assert.deepEqual(counted.data, [])
assert.equal(counted.cursor, undefined)
step(`client.count(), BETWEEN and a str range; count alone reads no rows (${counted.count} posts)`)

// ORDER BY through the schema: offset windows of the whole sorted match set.
// Views 3, 1, 4, 1, 5: DESC puts the 1s last, in creation order.
assert.deepEqual(await sortedBy(desc("views")), [4, 2, 0, 1, 3].map((i) => qKeys[i]))
assert.deepEqual(await sortedBy(asc("$key")), [...qKeys].sort())
// title is not indexed: a sort reads it all the same.
assert.deepEqual(await sortedBy(asc("title")), [1, 3, 0, 2, 4].map((i) => qKeys[i]))
await assert.rejects(
  rpc(`category = str('q') ORDER BY views`, {}),
  (e: any) => /query rejected \(type\)/.test(describe(e)) || assert.fail(describe(e)),
)
step("orderBy(desc/asc) sorts the whole match set, unindexed columns too, paged by offset")

// The SDK pages at the head, so a write between two requests shifts the
// windows. Pinning page one's blockNumber with atBlock (beyond the SDK) keeps
// every offset page on that snapshot.
const counting = { key: true, count: true }
const pageOne = await rpc(`category = str('q')`, { select: counting, limit: "0x2", offset: "0x0" })
await write.deleteEntity({ entityKey: qKeys[0] as `0x${string}` })
const atHead = await rpc(`category = str('q')`, { select: counting, limit: "0x2", offset: "0x2" })
const pinned = await rpc(`category = str('q')`, {
  select: counting,
  limit: "0x2",
  offset: "0x2",
  atBlock: pageOne.blockNumber,
})
assert.equal(atHead.count, "0x4")
assert.deepEqual(
  atHead.data.map((e: any) => e.key),
  qKeys.slice(3, 5),
)
assert.equal(pinned.count, "0x5")
assert.deepEqual(
  pinned.data.map((e: any) => e.key),
  qKeys.slice(2, 4),
)
const recreated = await write.createEntity(
  Posts,
  {
    author: addr(alice.address),
    category: "q",
    score: dec("2"),
    views: u64(3n),
    title: "views 3",
    body: new Uint8Array([3]),
  },
  { expiration },
)
qKeys.shift()
qKeys.push(recreated.entityKey)
step("offset pages at a pinned atBlock stay on its snapshot; at the head a delete shifts them")

writeFileSync(
  report,
  JSON.stringify({
    entityKey,
    bigKey: big.entityKey,
    longKeys: {
      [long.entityKey]: again.expiresAt.toString(),
      [open.entityKey]: longest.expiresAt.toString(),
    },
    maxKey: max.entityKey,
    qKeys,
  }),
)
console.log("UDC SMOKE PASSED")
