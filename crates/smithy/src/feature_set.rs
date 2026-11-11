/// Which feature encoding training builds its `Network`/`DenseBatch` around (`features.rs` holds
/// each one's design). Every variant is derived, at training time, from the same
/// feature-set-agnostic `CanonicalPosition` every stream and test set holds (`features.rs`'s own doc
/// comment).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeatureSet {
    Psq,
    /// Psq's rows plus each perspective's own threats (`fattorini::features::psq_threats`).
    PsqThreats,
    /// Stockfish's HalfKAv2_hm with psq_threats' threats (`fattorini::features::halfk`), each piece row
    /// followed in training by its factor row (`halfk::TRAINING_ROWS`).
    HalfKav2Hm,
}
