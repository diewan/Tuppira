# AGENT.md — Parwana AI Engineering Rules

## Mission

You are working on a security-critical cross-chain protocol.

Your task is NOT to make code compile.
Your task is to preserve protocol invariants and eliminate unsafe states.

Compilation success alone is NOT evidence of correctness.

---

# 1. Core Security Invariants

These are mandatory and non-negotiable.

1. No fabricated blockchain state.
2. No placeholder verification.
3. No bypass paths around verification/runtime/state machine.
4. No replayable transfers.
5. No mint without verified inclusion + verified finality.
6. No raw hashing in protocol logic.
7. No silent fallback behavior.
8. No partial validation.
9. No downgrade from error → warning for security failures.
10. All state transitions must be monotonic and explicit.

---

# 2. Forbidden Patterns

The following are forbidden in production code:

```rust
todo!()
unimplemented!()
unwrap()
expect()
unsafe
new_unchecked()

Ok(true)
Ok(Default::default())

assert!(true)

Sha256::digest
Keccak256::digest
blake3::hash
```

Forbidden outside:

* /tests
* /fuzz
* /benches

Also forbidden:

* fake proofs
* mock signatures
* placeholder crypto
* empty verification
* “temporary” production fixes

Never remove logic merely to satisfy the compiler.

---

# 3. Verification Rules

Every verification path MUST:

* reject missing data,
* reject malformed proofs,
* reject empty proof bundles,
* perform actual cryptographic verification,
* return Err(...) on verification failure.

Verification code may NEVER:

* silently pass,
* fallback,
* infer success from missing fields,
* substitute defaults.

`Ok(true)` in verification paths is forbidden.

---

# 4. Runtime Architecture Rules

Applications MUST use:

* csv-runtime
* TransferCoordinator
* unified runtime verification/state flow

Applications MUST NOT:

* call adapters directly,
* bypass runtime state machine,
* mint directly,
* skip replay protection,
* skip seal registry validation.

Security abstractions that are not wired into production paths are considered FAILED implementations.

---

# 5. Mandatory Execution Protocol

## 5.1 Repository-Wide Search

Before fixing anything:

1. search the repository for equivalent patterns,
2. enumerate all occurrences,
3. classify production vs test usage,
4. patch all production occurrences,
5. verify no bypass path remains.

Fixing only one occurrence is forbidden.

---

## 5.2 Transitive Verification

After every change verify:

* old code paths removed,
* deprecated APIs unreachable,
* runtime actually invokes new logic,
* CI checks target real crate/module names,
* applications cannot bypass protections.

Unused security code does not count as implemented.

---

## 5.3 Adversarial Review

Before declaring completion ask:

* How can this still fail?
* Can an attacker bypass this elsewhere?
* Does any legacy path remain callable?
* Can CI silently miss this?
* Can malformed proofs still pass?
* Can replay still occur concurrently?

You MUST attempt to break your own implementation.

---

## 5.4 Proof Obligations

Do not declare completion without:

* tests,
* negative tests,
* grep/ripgrep verification,
* invariant verification,
* regression prevention,
* CI enforcement where applicable.

---

# 6. Required Testing

Every security fix MUST include:

* positive test,
* malformed-input test,
* replay/double-use test where applicable,
* invariant regression test.

Verification fixes REQUIRE malformed proof rejection tests.

---

# 7. CI Enforcement Requirements

CI MUST fail on:

* TODO/FIXME
* unwrap()/expect()
* unsafe outside approved modules
* Result<bool> in verification paths
* Ok(true) in verification paths
* raw hashing
* direct adapter imports from applications
* fake/mock crypto in production code

Compile-fail tests are preferred over grep checks whenever possible.

---

# 8. Dependency Boundaries

Applications:

* MUST depend on csv-runtime
* MUST NOT depend directly on chain adapters

Wallet/UI code may not orchestrate transfers directly.

Transfer orchestration belongs exclusively to:

* TransferCoordinator

---

# 9. Completion Criteria

A task is complete ONLY IF:

* all invariant violations are eliminated,
* all production paths updated,
* no bypass path remains,
* no legacy insecure path remains reachable,
* tests pass,
* CI protections exist,
* equivalent anti-patterns are removed repository-wide.

“Compiles successfully” is NOT completion.

---

# 10. Agent Behavior Rules

Never:

* simplify cryptographic structures to satisfy types,
* replace proofs with bytes,
* delete validation logic,
* weaken invariants,
* postpone security work,
* leave dead security code,
* leave stub implementations.

If architecture prevents correctness:
REFactor the architecture.
Do not weaken the protocol.
