# Streaming genesis measurements, October 2026

Dated run records for [streaming genesis](../streaming-genesis-v2.md),
[bulk construction](../bulk-prefill.md) and
[staged-store adoption](../genesis-adoption.md). Every figure is a single run
on one shared host, not a repeated controlled trial. The Sourcify inputs of
these runs were converted with the earlier payload-era converter (row codec
2), which current nodes refuse; the figures describe the import and
installation phases, which are unchanged.

## Build once (prebuilt state), 2026-10-20

These are single sequential runs of one release binary on the shared host, at
`nice 19`/idle I/O priority while the full Ethereum producer was running beside
them. There was no cgroup cap; the free-space reserve was 80 GiB. They are not
repeated controlled trials.

The 263-entity Sourcify fixture went through convert, `stream-build` and
producer start twice: in default build-once mode and
with the producer rebuilding the state from the stream. Both produced identical bundle reports and
heads, with genesis `0x12832bb2…2281` and state root `0x7dd55378…ac27`. All 263
entities, and the 16 `kind = str('verified_contract')` keys, read back through
`arkiv_query` identically. The build-once producer logged one `moved` install and
no normalization or index phase.

On the matched 100k stream (445,600 entities, 1,853,670,399 bytes, base and
sponsor as in [the scaling report](2026-10-19-bulk-prefill-scaling.md)):

| Phase | Producer rebuild | Build once |
| --- | ---: | ---: |
| `stream-build` wall time | 406.8 s | 407.3 s |
| Producer ready | 697.2 s | **325.4 s** |
| Producer stream audits | 3 × ~18 s | 4 × ~17.5 s |
| Producer normalization | 108.8 s | none |
| Producer index construction | 252.8 s | none |
| Installation interval | 277.9 s | 251.4 s |
| Producer kernel `write_bytes` | 67.7 GB | 30.6 GB |
| End to end (build + ready) | 1,104.0 s | 732.7 s |

Both producers reached the historical 100k commitment: state root
`0x8074f966…aec0`, genesis `0xa1182828…793f`. The kept state directory used
5.1 GB, and the sampled peak of the node directory was 12.06 GB in both modes.

At full scale (15,298,744 entities), build once removes the producer's repeat of
5,499 s normalization and 20,441 s index construction. It adds one reuse audit
of about 480 s, so the projected saving is about **7.1 h**. The producer still
performs four stream audits (about 32 min at 48.2 GB) and the final installation
copy. That copy is now the dominant producer cost; at 100k it was 77% of
build-once readiness.

## Adopt instead of copy, 2026-10-21

The 100k-contract bundle has 445,600 entities and a 1.85 GB stream, and its staged
store is 6.0 GiB. The machine has NVMe RAID0 and 12 cores. The staged file's page
cache was dropped (`fsync` plus `POSIX_FADV_DONTNEED`) before each install. The
times cover only the install step:

| Install | Time | Bytes written | Second database |
| --- | --- | --- | --- |
| copy | 263.2 s | about 29 GB of file-system output | 6.0 GiB |
| adopt, 1 verification worker | 151.5 s | 2.6 MB | none |
| adopt, 8 verification workers | 25.4 s | 2.6 MB | none |

End to end, the producer binary on the same bundle builds the staged store and
then installs it, so the cache is warm:

| Producer | Ready | Install step | Peak RSS | Height-zero head and query proofs |
| --- | --- | --- | --- | --- |
| `--genesis-install copy` | 608.5 s | 220.5 s | 0.66 GiB | reference |
| `--genesis-install adopt` | 410.6 s | 25.5 s (verify and relabel 15.1 s) | 0.91 GiB | identical |

The adopted node restarted in 38.7 s, the stream re-audit, and produced block 1.
A producer killed with SIGKILL during verification left its staged store
untouched. The next start reused it without rebuilding and adopted it.

The copy costs 0.59 ms per entity here, but 2.8 ms per entity on the full run,
where neither database fits in the page cache. Adoption is bound by random reads
of one database instead of reads and writes of two. These sample figures do not
establish a full-dataset time.

## Bulk construction against incremental replay, 2026-10-19

Test coverage includes exact trie root/object-set equality and proofs, multiple
external-merge levels, shared postings, row IDs, multiple creators and expiry
heights, supported value types, admission and accounting failures, truncated or
modified sort runs, interrupted object writes, incomplete scratch rebuilding,
completion-receipt corruption and completed-state reopening.

Release acceptance on the 263-entity / 16-contract fixture passed bundle creation,
producer/full/light head agreement, full-node stream download, account/entity
proofs, a live signed write and restart. The light node retained no prefill. The
newly authored bundle report exactly matched the historical incremental bundle,
including genesis/state roots, storage units and rent. This isolated acceptance
run took 2.013 seconds and peaked at 54.1 MiB cgroup memory with no swap; it is a
correctness check, not a large-import speed estimate.

A fresh producer imported the original 100,000-contract / 445,583-entity bundle
and reached the expected height-zero genesis and state roots in **612.267 seconds
(10m 12.3s)**. The run used the same canonical bundle as the historical comparison,
under a 32 GiB cgroup cap, no swap and an 80 GiB disk reserve. All memory-event
counters stayed zero. The diagnostic producer was stopped after readiness.

| Measurement | Previous incremental run | Bulk run |
| --- | ---: | ---: |
| Producer ready | 1,270.554 s | 612.267 s |
| State construction, including compaction where applicable | 975.555 s | 342.884 s |
| Final installation interval | 238.226 s | 212.553 s |
| Standalone stream audits | 53.036 s | 53.818 s |
| Process peak RSS | 943.0 MiB | 643.3 MiB |
| Cgroup peak including file cache | 14.30 GiB | 13.64 GiB |
| Kernel process `write_bytes` counter | 168,471,179,264 | 67,520,536,576 |
| Kernel `cancelled_write_bytes` counter | 0 | 2,841,800,704 |

Bulk construction comprised 104.583 seconds of validation and sorted-run
production, followed by 238.301 seconds merging and building indexes. The index
phase processed 18,973,546 term memberships. Final installation is measured from
bulk completion to the last audit; it includes surrounding metadata/cleanup
work and is not an isolated inner-copy timer. CPU time was 460.09 seconds user
and 144.07 seconds system.

Observed startup time fell 51.8%, construction time about 64.9%, and the kernel
write counter 59.9%. These are single-run historical comparisons, not repeated
controlled A/B measurements. The write counters are kernel task I/O accounting,
not device-level measurements; cancelled writes include deleted dirty scratch
pages. The final copy still costs roughly three and a half minutes, and write
amplification remains substantial. The 100k run checked producer construction
and readiness, not a repeat of the three-role acceptance at that scale.

The verified commitments remain:

- Genesis: `0x612bab0b9ee7e20f1a0fc99a4d6bda5db26291b69e0218eef1ac57df67d747ae`
- State root: `0xf91cbca6523233b6214baa3da89bab61144c7c817126122bb433d3fe94e7a458`
- Storage units: `3324006410`
- Burned rent: `9684277486347000000000` wei

The original input and acceptance are described in
[the 100k validation record](../runs/2026-10-19-streaming-prefill-100k.md).


## Incremental importer, 2026-10-19

The measurements below describe the incremental importer that bulk
construction replaced.

The isolated `scripts/streaming-prefill-smoke.py` accepts `--prefill` and a new
`--work-dir`, creates disposable fixture credentials, and stops every process on
exit. It passed on the real 16-contract / 263-entity Sourcify fixture:

- Independent full-follower download and replay; producer/full/light head agreement.
- Verified account and entity query proofs, including all 16 contract entities.
- Live signed write at block one, persisted receipt, and restart of all roles.
- No prefill file in the light-node directory.

After bounded posting reuse, the 256 MiB final-destination cache, and the
five-to-three producer audit reduction, the same release-build fixture passed
again with the unchanged genesis/state commitments. The isolated acceptance
run took 1.762 seconds and peaked at 52.9 MiB cgroup memory with no swap; this
small fixture does not measure large-import speedup.
New cases cover cache collisions and namespace separation, malformed
posting counts, missing/corrupt objects, copy-flush failures and retry, retained
inode replacement, same-inode mutation/truncation, genesis binding and finalized
checkpoint recovery.

A fresh producer then imported the original 100,000-contract / 445,583-entity
bundle (1,858,260,091 canonical bytes) and reached the expected height-zero
genesis hash and state root in **1,270.554 seconds (21m 10.6s)**. This producer
measurement used a fresh destination and the unchanged canonical bundle under
the 32 GiB memory cap, no swap and 80 GiB disk reserve. The process was stopped
after readiness; it did not rerun the full three-role acceptance at this scale.

| Measurement | Historical baseline | Optimized run |
| --- | ---: | ---: |
| Producer readiness | approximately 1,300–1,310 s | 1,270.554 s |
| Standalone audits | 5 passes, 89.944 s | 3 passes, 53.036 s |
| Replay including compaction | 958.354 s | 975.555 s |
| Compaction | 133.363 s | 132.430 s |
| Replay excluding compaction | 824.990 s | 843.125 s |
| Process physical bytes written | 169,436,819,456 | 168,471,179,264 |

The interval from replay completion to the final audit was 238.226 seconds.
It includes final installation and surrounding overhead; the inner object copy
was not separately timed. Peak process RSS was 965,676 KiB (943.0 MiB), while
peak cgroup memory was 15,356,203,008 bytes (14.30 GiB), including file cache.
All memory-event counters remained zero and no swap was used.

The observed readiness improvement is only about 2–3%, and physical writes fell
about 0.6%. These are historical comparisons, not a controlled repeated A/B
benchmark. The audit reduction is clear, but replay excluding compaction took
longer in this run and compaction was nearly unchanged. These changes do not
solve the main import bottleneck.
The input and original acceptance are documented in
[the 100,000-contract validation record](../runs/2026-10-19-streaming-prefill-100k.md).

Debug-build streaming construction of synthetic 29-attribute entities with
1 KiB payloads, on the local host, produced these preliminary measurements
using the original 64 KiB batches and 8 MiB caches (before the import tuning):

| Entities | Canonical bytes | Wall time | Peak process RSS | Peak cgroup memory |
| --- | ---: | ---: | ---: | ---: |
| 1,000 | 1,620,980 | 4.51 s | 35,916 KiB | 30.3 MiB |
| 10,000 | 16,349,480 | 61.62 s | 44,212 KiB | 121 MiB |

The time utility and cgroup use different accounting; cgroup values can include
file-cache charges and are not interchangeable with RSS. These small-scale
results are not a capacity estimate for the full Sourcify dataset.

A separate 10,000-row run was killed with SIGKILL during replay; no bundle was
published at interruption. Resumption produced the exact same genesis hash as
the uninterrupted run and removed completed scratch. This does not exhaust
all power-loss or disk-full timing windows.

Inputs beyond this 445,583-entity sample, disk amplification and sustained import
throughput remain the next scale gate.
