//! Normalization of chain-native closure events into one observation model.
//!
//! Five chains express "this state is now closed" in five different ways: a
//! Bitcoin outpoint is spent, an EVM contract registers a nullifier, a Sui
//! object version is consumed, an Aptos resource is consumed, a Solana account
//! is consumed. This module maps all five onto the single
//! [`SourceClosureObservationProjectionV1`], so an investigator reads one shape
//! rather than five, and pairs each projection with a
//! [`ChainClosureEvidenceRecord`] that addresses the exact chain event it came
//! from.
//!
//! Three rules shape the mapping.
//!
//! **The closure identity is what can only be closed once.** For each family it
//! is the chain's own single-use handle — the outpoint, the nullifier, the
//! object version, the resource, the account — and nothing else. Anything that
//! varies between two closures of the same handle (a slot, a sequence number, a
//! spending transaction) is deliberately excluded: folding it in would make two
//! competing closures look like two unrelated ones, which is precisely the
//! equivocation this plane exists to surface.
//!
//! **The mapping is an encoding, not a summary.** Native components are
//! concatenated in a fixed order using reversible encodings — hex bytes for
//! binary identities, big-endian fixed-width integers for indexes and versions,
//! hex-encoded UTF-8 for chain type names — so the identity can be read back to
//! the components it was built from. Nothing is hashed and nothing is dropped.
//!
//! **Normalization never verifies.** Nothing here decides whether a closure is
//! valid; that belongs to Parwana's verifier. What a foreign verifier concluded
//! is relayed on the projection's `external_verification` facet, unmerged, and
//! this module never populates it from a chain read.

use tuppira_shared::{
    CHAIN_CLOSURE_EVIDENCE_VERSION, CLOSURE_OBSERVATION_PROFILE_VERSION,
    ChainClosureEvidenceRecord, ClosureIdentityReading, ClosureRevocationReading,
    ClosureSettlementReading, ConsumedStateReading, IndexFreshnessReading,
    ObservationValidationError, ObservedCheckpointReading, ObservedOrMissing,
    SourceClosureObservationProjectionV1,
};

/// Registered closure family name for a spent Bitcoin outpoint.
pub const BITCOIN_OUTPOINT_SPEND_KIND: &str = "bitcoin-outpoint-spend";
/// Registered closure family name for an EVM nullifier registration.
pub const EVM_NULLIFIER_KIND: &str = "evm-nullifier";
/// Registered closure family name for a consumed Sui object version.
pub const SUI_OBJECT_CONSUMPTION_KIND: &str = "sui-object-consumption";
/// Registered closure family name for a consumed Aptos resource.
pub const APTOS_RESOURCE_CONSUMPTION_KIND: &str = "aptos-resource-consumption";
/// Registered closure family name for a consumed Solana account.
pub const SOLANA_ACCOUNT_CONSUMPTION_KIND: &str = "solana-account-consumption";

/// Rejection reasons at the chain-normalization boundary.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ClosureNormalizationError {
    /// A chain-native identity was not lowercase hex of even length.
    #[error("malformed chain-native identity: {0}")]
    MalformedNativeIdentity(&'static str),
    /// A required chain-native field was empty or out of range.
    #[error("invalid chain-native field: {0}")]
    InvalidNativeField(&'static str),
    /// The projection assembled from this event failed its own validation.
    #[error("normalized projection rejected: {0:?}")]
    RejectedProjection(ObservationValidationError),
    /// The evidence record assembled from this event failed its own validation.
    #[error("chain evidence rejected: {0:?}")]
    RejectedEvidence(ObservationValidationError),
}

/// One chain-native closure event, in the shape its own chain expresses it.
///
/// Each variant carries both the single-use handle that identifies the closure
/// and the locators that address the event on its chain. The two are kept apart
/// here so the mapping below cannot accidentally fold a locator into an
/// identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeClosureEventReading {
    /// A Bitcoin outpoint was spent by a transaction input.
    BitcoinOutpointSpend {
        /// Transaction that created the spent output, hex-encoded.
        funding_transaction_id_hex: String,
        /// Index of the spent output within that transaction.
        funding_output_index: u32,
        /// Transaction that spent it, hex-encoded.
        spending_transaction_id_hex: String,
        /// Index of the spending input within that transaction.
        spending_input_index: u32,
    },
    /// An EVM contract registered a nullifier in a log.
    EvmNullifierRegistration {
        /// Registering contract address, hex-encoded without `0x`.
        contract_address_hex: String,
        /// The registered nullifier, hex-encoded.
        nullifier_hex: String,
        /// Transaction carrying the log, hex-encoded.
        transaction_hash_hex: String,
        /// Index of the log within the block.
        log_index: u32,
    },
    /// A Sui object version was consumed by a transaction.
    SuiObjectConsumption {
        /// Consumed object identifier, hex-encoded.
        object_id_hex: String,
        /// The consumed version of that object.
        object_version: u64,
        /// Transaction that consumed it, hex-encoded digest.
        transaction_digest_hex: String,
    },
    /// An Aptos resource was consumed, reported by a transaction event.
    AptosResourceConsumption {
        /// Owning account address, hex-encoded without `0x`.
        account_address_hex: String,
        /// Fully-qualified Move resource type, verbatim.
        resource_type: String,
        /// Transaction that consumed it, hex-encoded hash.
        transaction_hash_hex: String,
        /// Sequence number of the reporting event.
        event_sequence_number: u64,
    },
    /// A Solana account was consumed within a slot.
    SolanaAccountConsumption {
        /// Consumed account address, hex-encoded.
        account_address_hex: String,
        /// Transaction that consumed it, hex-encoded signature.
        transaction_signature_hex: String,
        /// Index of the consuming instruction within that transaction.
        instruction_index: u32,
    },
}

impl NativeClosureEventReading {
    /// The registered closure family name for this event.
    #[must_use]
    pub fn closure_kind(&self) -> &'static str {
        match self {
            Self::BitcoinOutpointSpend { .. } => BITCOIN_OUTPOINT_SPEND_KIND,
            Self::EvmNullifierRegistration { .. } => EVM_NULLIFIER_KIND,
            Self::SuiObjectConsumption { .. } => SUI_OBJECT_CONSUMPTION_KIND,
            Self::AptosResourceConsumption { .. } => APTOS_RESOURCE_CONSUMPTION_KIND,
            Self::SolanaAccountConsumption { .. } => SOLANA_ACCOUNT_CONSUMPTION_KIND,
        }
    }

    /// The chain's single-use handle for this closure, hex-encoded.
    ///
    /// Only components that are fixed for the life of the handle take part.
    /// A Solana account consumed in two different slots yields one identity,
    /// not two, because it is one account being claimed twice — and two
    /// identities would hide exactly that.
    fn closure_identity_hex(&self) -> Result<String, ClosureNormalizationError> {
        match self {
            Self::BitcoinOutpointSpend {
                funding_transaction_id_hex,
                funding_output_index,
                ..
            } => {
                let funding = normalized_hex(
                    funding_transaction_id_hex,
                    "bitcoin.funding_transaction_id_hex",
                )?;
                Ok(format!("{funding}{funding_output_index:08x}"))
            }
            Self::EvmNullifierRegistration {
                contract_address_hex,
                nullifier_hex,
                ..
            } => {
                // The contract scopes the nullifier: the same 32 bytes
                // registered by two contracts are two closures, not one.
                let contract = normalized_hex(contract_address_hex, "evm.contract_address_hex")?;
                let nullifier = normalized_hex(nullifier_hex, "evm.nullifier_hex")?;
                Ok(format!("{contract}{nullifier}"))
            }
            Self::SuiObjectConsumption {
                object_id_hex,
                object_version,
                ..
            } => {
                let object = normalized_hex(object_id_hex, "sui.object_id_hex")?;
                Ok(format!("{object}{object_version:016x}"))
            }
            Self::AptosResourceConsumption {
                account_address_hex,
                resource_type,
                ..
            } => {
                let account = normalized_hex(account_address_hex, "aptos.account_address_hex")?;
                if resource_type.trim().is_empty() {
                    return Err(ClosureNormalizationError::InvalidNativeField(
                        "aptos.resource_type",
                    ));
                }
                // Hex-encoded UTF-8 rather than a hash: the Move type name must
                // stay readable back out of the identity.
                Ok(format!("{account}{}", hex::encode(resource_type.as_bytes())))
            }
            Self::SolanaAccountConsumption {
                account_address_hex,
                ..
            } => normalized_hex(account_address_hex, "solana.account_address_hex"),
        }
    }

    /// Chain-native locators addressing the exact evidence for this event.
    fn evidence_refs(
        &self,
        chain_id: &str,
        network_id: &str,
    ) -> Result<Vec<String>, ClosureNormalizationError> {
        let scope = format!("{chain_id}:{network_id}");
        let refs = match self {
            Self::BitcoinOutpointSpend {
                funding_transaction_id_hex,
                funding_output_index,
                spending_transaction_id_hex,
                spending_input_index,
            } => vec![
                format!(
                    "{scope}:outpoint:{}:{funding_output_index}",
                    normalized_hex(
                        funding_transaction_id_hex,
                        "bitcoin.funding_transaction_id_hex"
                    )?
                ),
                format!(
                    "{scope}:spending-input:{}:{spending_input_index}",
                    normalized_hex(
                        spending_transaction_id_hex,
                        "bitcoin.spending_transaction_id_hex"
                    )?
                ),
            ],
            Self::EvmNullifierRegistration {
                contract_address_hex,
                transaction_hash_hex,
                log_index,
                ..
            } => vec![
                format!(
                    "{scope}:contract:{}",
                    normalized_hex(contract_address_hex, "evm.contract_address_hex")?
                ),
                format!(
                    "{scope}:log:{}:{log_index}",
                    normalized_hex(transaction_hash_hex, "evm.transaction_hash_hex")?
                ),
            ],
            Self::SuiObjectConsumption {
                object_id_hex,
                object_version,
                transaction_digest_hex,
            } => vec![
                format!(
                    "{scope}:object:{}:{object_version}",
                    normalized_hex(object_id_hex, "sui.object_id_hex")?
                ),
                format!(
                    "{scope}:transaction:{}",
                    normalized_hex(transaction_digest_hex, "sui.transaction_digest_hex")?
                ),
            ],
            Self::AptosResourceConsumption {
                account_address_hex,
                resource_type,
                transaction_hash_hex,
                event_sequence_number,
            } => vec![
                format!(
                    "{scope}:resource:{}:{resource_type}",
                    normalized_hex(account_address_hex, "aptos.account_address_hex")?
                ),
                format!(
                    "{scope}:event:{}:{event_sequence_number}",
                    normalized_hex(transaction_hash_hex, "aptos.transaction_hash_hex")?
                ),
            ],
            Self::SolanaAccountConsumption {
                account_address_hex,
                transaction_signature_hex,
                instruction_index,
            } => vec![
                format!(
                    "{scope}:account:{}",
                    normalized_hex(account_address_hex, "solana.account_address_hex")?
                ),
                format!(
                    "{scope}:instruction:{}:{instruction_index}",
                    normalized_hex(
                        transaction_signature_hex,
                        "solana.transaction_signature_hex"
                    )?
                ),
            ],
        };
        Ok(refs)
    }
}

/// One chain closure event with everything normalization needs around it.
///
/// The connector supplies the Parwana identities it read from the event, what
/// the chain reported about its own checkpoint and finality, and the index
/// freshness at the moment of the read. Anything the source did not report
/// arrives as [`ObservedOrMissing::Missing`] with reasons and stays that way:
/// normalization never fills a gap in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainClosureEventReading {
    /// Stable chain identifier, such as `ethereum`.
    pub chain_id: String,
    /// Stable network identifier, such as `sepolia`.
    pub network_id: String,
    /// The chain-native event itself.
    pub native_event: NativeClosureEventReading,
    /// Parwana's identity for the consumed state, as the source expressed it.
    pub consumed_state: ConsumedStateReading,
    /// Canonical successor transition commitment, hex-encoded.
    pub successor_commitment_hex: String,
    /// Successor outputs the source associated with this closure.
    pub successor_output_refs: Vec<String>,
    /// The block or checkpoint the source read.
    pub observed_checkpoint: ObservedOrMissing<ObservedCheckpointReading>,
    /// What the source concluded under its own finality rule.
    pub settlement: ObservedOrMissing<ClosureSettlementReading>,
    /// The source's retraction report for this closure.
    pub revocation: ObservedOrMissing<ClosureRevocationReading>,
    /// Index lag at the moment the event was read.
    pub index_freshness: IndexFreshnessReading,
    /// Digest of the exact source bytes the event was decoded from.
    pub raw_event_digest: [u8; 32],
}

/// A normalized closure and the chain evidence it came from, kept together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedClosureObservation {
    /// The single observation model every chain family maps onto.
    pub projection: SourceClosureObservationProjectionV1,
    /// The way back to the chain event this projection was derived from.
    pub evidence: ChainClosureEvidenceRecord,
}

/// Map one chain-native closure event onto the single observation model.
///
/// The `external_verification` facet is always
/// [`ObservedOrMissing::Missing`]: a chain read is not a verdict, and Tuppira
/// does not produce one. A verdict reaches a projection only by being relayed
/// from the verifier that computed it.
///
/// # Errors
///
/// Returns [`ClosureNormalizationError`] for a malformed chain-native identity,
/// an empty required field, or a projection or evidence record that fails its
/// own validation. Nothing partial is returned: an event that cannot be mapped
/// completely is not mapped at all.
pub fn normalize_chain_closure_event(
    observation_id: &str,
    reading: &ChainClosureEventReading,
) -> Result<NormalizedClosureObservation, ClosureNormalizationError> {
    let closure_kind = reading.native_event.closure_kind();
    let projection = SourceClosureObservationProjectionV1 {
        schema_version: CLOSURE_OBSERVATION_PROFILE_VERSION,
        consumed_state: reading.consumed_state.clone(),
        closure_identity: ClosureIdentityReading {
            closure_kind: closure_kind.to_string(),
            closure_identity_hex: reading.native_event.closure_identity_hex()?,
            successor_commitment_hex: reading.successor_commitment_hex.clone(),
        },
        chain_id: reading.chain_id.clone(),
        network_id: reading.network_id.clone(),
        successor_output_refs: reading.successor_output_refs.clone(),
        observed_checkpoint: reading.observed_checkpoint.clone(),
        settlement: reading.settlement.clone(),
        // A chain read is evidence that something happened, never a verdict on
        // whether it was valid. Populating this from a chain read would make
        // Tuppira the authority the whole plane is built to avoid being.
        external_verification: ObservedOrMissing::Missing {
            reasons: vec![
                "normalization relays chain evidence and never computes a closure verdict".into(),
            ],
        },
        revocation: reading.revocation.clone(),
        index_freshness: reading.index_freshness.clone(),
    };
    projection
        .validate()
        .map_err(ClosureNormalizationError::RejectedProjection)?;

    let evidence = ChainClosureEvidenceRecord {
        schema_version: CHAIN_CLOSURE_EVIDENCE_VERSION,
        observation_id: observation_id.to_string(),
        native_event_kind: closure_kind.to_string(),
        evidence_refs: reading
            .native_event
            .evidence_refs(&reading.chain_id, &reading.network_id)?,
        raw_event_digest: reading.raw_event_digest,
    };
    evidence
        .validate()
        .map_err(ClosureNormalizationError::RejectedEvidence)?;

    Ok(NormalizedClosureObservation {
        projection,
        evidence,
    })
}

/// Accept a chain-native identity only in the one spelling the model stores.
///
/// A `0x` prefix is stripped and uppercase hex is lowered, because the same
/// identity written two ways would index as two closures and hide a conflict.
/// Anything that is not hex, or is empty or odd-length, is rejected rather than
/// coerced.
fn normalized_hex(value: &str, field: &'static str) -> Result<String, ClosureNormalizationError> {
    let trimmed = value.trim();
    let body = trimmed.strip_prefix("0x").unwrap_or(trimmed);
    if body.is_empty() || !body.len().is_multiple_of(2) {
        return Err(ClosureNormalizationError::MalformedNativeIdentity(field));
    }
    if !body.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ClosureNormalizationError::MalformedNativeIdentity(field));
    }
    Ok(body.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tuppira_shared::{
        ClosureObservationState, RetractionStatus, SourceReportedSettlement,
        assess_closure_conflicts,
    };

    const TRANSITION_ID: &str = "11111111111111111111111111111111111111111111111111111111111111ab";
    const SUCCESSOR: &str = "22222222222222222222222222222222222222222222222222222222222222cd";

    fn reading(native_event: NativeClosureEventReading) -> ChainClosureEventReading {
        ChainClosureEventReading {
            chain_id: "ethereum".into(),
            network_id: "sepolia".into(),
            native_event,
            consumed_state: ConsumedStateReading {
                transition_id_hex: TRANSITION_ID.into(),
                output_index: 0,
                state_type: 1,
            },
            successor_commitment_hex: SUCCESSOR.into(),
            successor_output_refs: vec!["output:0".into()],
            observed_checkpoint: ObservedOrMissing::Observed {
                value: ObservedCheckpointReading {
                    block_height: 900,
                    block_id_hex: "abcdef01".into(),
                },
            },
            settlement: ObservedOrMissing::Observed {
                value: ClosureSettlementReading {
                    finality_policy: "confirmations".into(),
                    observed_depth: 64,
                    required_depth: 12,
                    reported_settlement: SourceReportedSettlement::Final,
                },
            },
            revocation: ObservedOrMissing::Missing {
                reasons: vec!["source exposes no retraction feed".into()],
            },
            index_freshness: IndexFreshnessReading {
                indexed_tip_height: 1_000,
                indexed_tip_block_id_hex: "beef".into(),
                indexed_tip_observed_at: 1_760_000_100,
                lag_blocks: ObservedOrMissing::Observed { value: 100 },
            },
            raw_event_digest: [6; 32],
        }
    }

    fn bitcoin() -> NativeClosureEventReading {
        NativeClosureEventReading::BitcoinOutpointSpend {
            funding_transaction_id_hex: "aa".repeat(32),
            funding_output_index: 3,
            spending_transaction_id_hex: "bb".repeat(32),
            spending_input_index: 1,
        }
    }

    fn evm() -> NativeClosureEventReading {
        NativeClosureEventReading::EvmNullifierRegistration {
            contract_address_hex: format!("0x{}", "CC".repeat(20)),
            nullifier_hex: "dd".repeat(32),
            transaction_hash_hex: "ee".repeat(32),
            log_index: 3,
        }
    }

    fn sui() -> NativeClosureEventReading {
        NativeClosureEventReading::SuiObjectConsumption {
            object_id_hex: "11".repeat(32),
            object_version: 7,
            transaction_digest_hex: "22".repeat(32),
        }
    }

    fn aptos() -> NativeClosureEventReading {
        NativeClosureEventReading::AptosResourceConsumption {
            account_address_hex: "33".repeat(32),
            resource_type: "0x1::diewan::Seal".into(),
            transaction_hash_hex: "44".repeat(32),
            event_sequence_number: 9,
        }
    }

    fn solana() -> NativeClosureEventReading {
        NativeClosureEventReading::SolanaAccountConsumption {
            account_address_hex: "55".repeat(32),
            transaction_signature_hex: "66".repeat(64),
            instruction_index: 2,
        }
    }

    fn every_family() -> Vec<NativeClosureEventReading> {
        vec![bitcoin(), evm(), sui(), aptos(), solana()]
    }

    // ── One observation model for five chains ─────────────────────────────────

    #[test]
    fn every_chain_family_maps_onto_the_one_observation_model() {
        for (index, native_event) in every_family().into_iter().enumerate() {
            let observation_id = format!("obs:closure:{index}");
            let normalized = normalize_chain_closure_event(&observation_id, &reading(native_event));

            let Ok(normalized) = normalized else {
                panic!("family {index} must map onto the observation model");
            };
            assert_eq!(normalized.projection.validate(), Ok(()));
            assert_eq!(
                normalized.projection.schema_version,
                CLOSURE_OBSERVATION_PROFILE_VERSION
            );
            // The same consumed state and successor survive every mapping, so
            // conflicts across chains compare on identical terms.
            assert_eq!(
                normalized.projection.consumed_state.transition_id_hex,
                TRANSITION_ID
            );
            assert_eq!(
                normalized
                    .projection
                    .closure_identity
                    .successor_commitment_hex,
                SUCCESSOR
            );
        }
    }

    #[test]
    fn each_family_keeps_its_own_registered_closure_kind() {
        let kinds: Vec<&str> = every_family()
            .iter()
            .map(NativeClosureEventReading::closure_kind)
            .collect();

        assert_eq!(
            kinds,
            vec![
                BITCOIN_OUTPOINT_SPEND_KIND,
                EVM_NULLIFIER_KIND,
                SUI_OBJECT_CONSUMPTION_KIND,
                APTOS_RESOURCE_CONSUMPTION_KIND,
                SOLANA_ACCOUNT_CONSUMPTION_KIND,
            ]
        );
        // Five families, five names: a collapsed pair would let one chain's
        // closure be read under another's rules.
        assert_eq!(
            kinds.iter().collect::<std::collections::BTreeSet<_>>().len(),
            5
        );
    }

    // ── Raw chain evidence stays reachable ────────────────────────────────────

    #[test]
    fn every_normalized_closure_carries_a_way_back_to_its_chain_event() {
        for (index, native_event) in every_family().into_iter().enumerate() {
            let observation_id = format!("obs:closure:{index}");
            let Ok(normalized) = normalize_chain_closure_event(&observation_id, &reading(native_event))
            else {
                panic!("family {index} must normalize");
            };

            assert_eq!(normalized.evidence.observation_id, observation_id);
            assert_eq!(normalized.evidence.validate(), Ok(()));
            assert_eq!(
                normalized.evidence.native_event_kind,
                normalized.projection.closure_identity.closure_kind
            );
            assert_eq!(normalized.evidence.raw_event_digest, [6; 32]);
            // Every locator is scoped to the chain and network it addresses, so
            // a reference cannot be followed to the wrong network.
            assert!(!normalized.evidence.evidence_refs.is_empty());
            for reference in &normalized.evidence.evidence_refs {
                assert!(reference.starts_with("ethereum:sepolia:"), "{reference}");
            }
        }
    }

    #[test]
    fn the_evidence_retains_detail_the_projection_drops() {
        // The spending input index has no home in the one observation model.
        // It must still be reachable, or the normalization has lost evidence.
        let Ok(normalized) = normalize_chain_closure_event("obs:closure:1", &reading(bitcoin()))
        else {
            panic!("bitcoin spend must normalize");
        };

        assert!(
            normalized
                .evidence
                .evidence_refs
                .iter()
                .any(|reference| reference.contains("spending-input") && reference.ends_with(":1")),
            "{:?}",
            normalized.evidence.evidence_refs
        );
    }

    // ── The identity is the handle that can only be closed once ───────────────

    #[test]
    fn one_solana_account_claimed_in_two_slots_is_one_closure_identity() {
        // Folding the slot into the identity would file the two claims as
        // unrelated closures and hide the double-claim entirely.
        let mut first = reading(solana());
        let mut second = reading(solana());
        first.observed_checkpoint = ObservedOrMissing::Observed {
            value: ObservedCheckpointReading {
                block_height: 900,
                block_id_hex: "aa".into(),
            },
        };
        second.observed_checkpoint = ObservedOrMissing::Observed {
            value: ObservedCheckpointReading {
                block_height: 950,
                block_id_hex: "bb".into(),
            },
        };
        second.index_freshness.lag_blocks = ObservedOrMissing::Observed { value: 50 };
        second.successor_commitment_hex = "33".repeat(31) + "ef";

        let (Ok(first), Ok(second)) = (
            normalize_chain_closure_event("obs:closure:1", &first),
            normalize_chain_closure_event("obs:closure:2", &second),
        ) else {
            panic!("both claims must normalize");
        };

        assert_eq!(
            first.projection.closure_identity.closure_identity_hex,
            second.projection.closure_identity.closure_identity_hex
        );
        // And the pair is therefore visible as a conflict.
        let outcome = assess_closure_conflicts(
            &first.projection.consumed_state,
            &first.projection.closure_identity.successor_commitment_hex,
            &[first.projection.clone(), second.projection],
            &first.projection.index_freshness,
        );
        assert!(matches!(
            outcome,
            tuppira_shared::ClosureConflictSearchOutcome::CompetingClosuresObserved { .. }
        ));
    }

    #[test]
    fn one_nullifier_registered_by_two_contracts_is_two_closures() {
        let NativeClosureEventReading::EvmNullifierRegistration { nullifier_hex, .. } = evm() else {
            panic!("fixture must be an EVM registration");
        };
        let other_contract = NativeClosureEventReading::EvmNullifierRegistration {
            contract_address_hex: "ab".repeat(20),
            nullifier_hex,
            transaction_hash_hex: "ee".repeat(32),
            log_index: 4,
        };

        let (Ok(first), Ok(second)) = (
            normalize_chain_closure_event("obs:closure:1", &reading(evm())),
            normalize_chain_closure_event("obs:closure:2", &reading(other_contract)),
        ) else {
            panic!("both registrations must normalize");
        };

        assert_ne!(
            first.projection.closure_identity.closure_identity_hex,
            second.projection.closure_identity.closure_identity_hex
        );
    }

    #[test]
    fn two_sui_versions_of_one_object_are_two_closures() {
        let later = NativeClosureEventReading::SuiObjectConsumption {
            object_id_hex: "11".repeat(32),
            object_version: 8,
            transaction_digest_hex: "22".repeat(32),
        };

        let (Ok(first), Ok(second)) = (
            normalize_chain_closure_event("obs:closure:1", &reading(sui())),
            normalize_chain_closure_event("obs:closure:2", &reading(later)),
        ) else {
            panic!("both consumptions must normalize");
        };

        assert_ne!(
            first.projection.closure_identity.closure_identity_hex,
            second.projection.closure_identity.closure_identity_hex
        );
    }

    #[test]
    fn the_same_identity_written_two_ways_normalizes_to_one() {
        // `0x`-prefixed and uppercase spellings of one outpoint must not index
        // as two closures; two identities would hide a conflict between them.
        let prefixed = NativeClosureEventReading::BitcoinOutpointSpend {
            funding_transaction_id_hex: format!("0x{}", "AA".repeat(32)),
            funding_output_index: 3,
            spending_transaction_id_hex: "BB".repeat(32),
            spending_input_index: 1,
        };

        let (Ok(plain), Ok(prefixed)) = (
            normalize_chain_closure_event("obs:closure:1", &reading(bitcoin())),
            normalize_chain_closure_event("obs:closure:2", &reading(prefixed)),
        ) else {
            panic!("both spellings must normalize");
        };

        assert_eq!(
            plain.projection.closure_identity.closure_identity_hex,
            prefixed.projection.closure_identity.closure_identity_hex
        );
    }

    #[test]
    fn an_aptos_resource_type_stays_readable_out_of_the_identity() {
        let Ok(normalized) = normalize_chain_closure_event("obs:closure:1", &reading(aptos()))
        else {
            panic!("aptos consumption must normalize");
        };
        let identity = &normalized.projection.closure_identity.closure_identity_hex;
        let account = "33".repeat(32);

        let Some(encoded_type) = identity.strip_prefix(&account) else {
            panic!("identity must begin with the owning account");
        };
        assert_eq!(
            hex::decode(encoded_type).ok().and_then(|bytes| String::from_utf8(bytes).ok()),
            Some("0x1::diewan::Seal".to_string())
        );
    }

    // ── Normalization relays, it never verifies ───────────────────────────────

    #[test]
    fn a_chain_read_never_becomes_a_verification_verdict() {
        for (index, native_event) in every_family().into_iter().enumerate() {
            let Ok(normalized) =
                normalize_chain_closure_event("obs:closure:1", &reading(native_event))
            else {
                panic!("family {index} must normalize");
            };

            let ObservedOrMissing::Missing { reasons } =
                &normalized.projection.external_verification
            else {
                panic!("a chain read must not produce a verification verdict");
            };
            assert!(!reasons.is_empty());
            assert!(
                !normalized
                    .projection
                    .established_states()
                    .contains(&ClosureObservationState::VerifiedElsewhere)
            );
        }
    }

    #[test]
    fn an_unreported_facet_stays_unknown_after_normalization() {
        let mut bare = reading(bitcoin());
        bare.observed_checkpoint = ObservedOrMissing::Missing {
            reasons: vec!["source did not disclose a checkpoint".into()],
        };
        bare.settlement = ObservedOrMissing::Missing {
            reasons: vec!["no checkpoint to apply a finality rule to".into()],
        };
        bare.index_freshness.lag_blocks = ObservedOrMissing::Missing {
            reasons: vec!["no checkpoint to measure against".into()],
        };

        let Ok(normalized) = normalize_chain_closure_event("obs:closure:1", &bare) else {
            panic!("an incomplete but honest report must still normalize");
        };
        let states = normalized.projection.established_states();
        assert!(states.contains(&ClosureObservationState::Observed));
        assert!(states.contains(&ClosureObservationState::Unknown));
        assert!(!states.contains(&ClosureObservationState::Final));
    }

    #[test]
    fn a_source_retraction_survives_normalization() {
        let mut retracted = reading(sui());
        retracted.revocation = ObservedOrMissing::Observed {
            value: ClosureRevocationReading {
                retraction_status: RetractionStatus::Retracted,
                reorg_id: Some("reorg-17".into()),
                reported_at: 1_760_000_000,
            },
        };

        let Ok(normalized) = normalize_chain_closure_event("obs:closure:1", &retracted) else {
            panic!("a retracted closure must still normalize");
        };
        assert!(
            normalized
                .projection
                .established_states()
                .contains(&ClosureObservationState::Revoked)
        );
    }

    // ── Nothing partial escapes ───────────────────────────────────────────────

    #[test]
    fn a_malformed_native_identity_is_rejected_rather_than_coerced() {
        let malformed = NativeClosureEventReading::BitcoinOutpointSpend {
            funding_transaction_id_hex: "not-hex".into(),
            funding_output_index: 3,
            spending_transaction_id_hex: "bb".repeat(32),
            spending_input_index: 1,
        };

        assert_eq!(
            normalize_chain_closure_event("obs:closure:1", &reading(malformed)),
            Err(ClosureNormalizationError::MalformedNativeIdentity(
                "bitcoin.funding_transaction_id_hex"
            ))
        );
    }

    #[test]
    fn an_aptos_event_without_a_resource_type_is_rejected() {
        let malformed = NativeClosureEventReading::AptosResourceConsumption {
            account_address_hex: "33".repeat(32),
            resource_type: "   ".into(),
            transaction_hash_hex: "44".repeat(32),
            event_sequence_number: 9,
        };

        assert_eq!(
            normalize_chain_closure_event("obs:closure:1", &reading(malformed)),
            Err(ClosureNormalizationError::InvalidNativeField(
                "aptos.resource_type"
            ))
        );
    }

    #[test]
    fn a_lag_the_projection_rejects_is_not_normalized_anyway() {
        // A checkpoint beyond the indexed tip means the tip cannot have covered
        // it. The projection's own validation catches it, and normalization
        // must surface that rather than storing an impossible freshness.
        let mut impossible = reading(evm());
        impossible.index_freshness.indexed_tip_height = 800;

        assert_eq!(
            normalize_chain_closure_event("obs:closure:1", &impossible),
            Err(ClosureNormalizationError::RejectedProjection(
                ObservationValidationError::InvalidField("index_freshness.indexed_tip_height")
            ))
        );
    }

    #[test]
    fn normalization_is_deterministic() {
        let reading = reading(solana());
        assert_eq!(
            normalize_chain_closure_event("obs:closure:1", &reading),
            normalize_chain_closure_event("obs:closure:1", &reading)
        );
    }
}
