# NAM-03 observation-plane naming migration

Base revision: `7044fee9c196aaaa0876f03737365ecc4e8b2c76`

## Semantic boundaries

The ingestion path now names each lifecycle and representation explicitly:

```text
RawSourceEvent
  -> authenticate
  -> NormalizedObservationInput
  -> ObservationRecord
  -> ObservationProjectionV1
```

For example, a Piteka export arrives as exact bytes in `RawSourceEvent`. Its
signature material is checked before normalization. `NormalizedObservationInput`
then carries the complete `ObservationRecord`, its optional raw-payload custody
descriptor, and a `DeploymentAttestationProjectionV1`. Persistence validates
the observation and tenant scope before the API maps it to
`ObservationProjectionV1`, which intentionally omits raw payload digests and
custody locators.

An `ObservationRecord` has source identity, source event identity, collection
run, acquisition time, authenticity references, tenant visibility, normalized
content digest, and an optional exact-source-payload digest. Nested provider
values are `Reading` types because their provenance belongs to the containing
observation. Missing profile evidence uses `ObservedOrMissing` with reasons.
Standalone RPC finality samples are `AnchorFinalityReading` because they do not
carry collection-run and raw-payload provenance.

`SourceReconciliationAssessment` is a derived, non-authoritative comparison.
It is neither a source fact nor a Parwana verifier conclusion. Anchor reading
assessment delegates finality semantics to Parwana.

## Compatibility and deployment

This is a source-name migration. Serde field names, SQLite tables and columns,
REST paths and response fields are unchanged. GraphQL retains the historical
type names `ObservationGql` and `SourceHealthGql` while Rust uses projection
names. Historical rows are read through the unchanged `ObservationRecord`
mapping, so no data rewrite or dual-write window is required.

Deploy producers and consumers in any order because the wire and stored-data
contracts are unchanged. Downstream Rust source consumers must update imports
atomically to the new public names.

Rollback is the inverse source rename at revision control level. It requires
no database rollback and does not discard data. The fail-safe is to stop
ingestion if an unknown schema version or incomplete observation reaches the
typed boundary; existing append-only history remains readable.

## Verification

Run:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

The shared wire-shape test proves historical deployment projections still
deserialize and serialize byte-for-byte. Storage tests cover append-only
history, raw-payload commitments, and cross-tenant visibility. API schema tests
continue to ensure custody fields are not disclosed.
