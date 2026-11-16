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
    /// `HalfKav2Hm` with psq's 12 castling and en passant rows after the factor rows
    /// (`halfk::TRAINING_ROWS_WITH_STATE`, `halfk::push_state_rows`): halfk_threats_pairwise_hidden's.
    HalfKav2HmState,
}

impl FeatureSet {
    /// Whether this is one of halfk's: factorised, its piece rows in the king's bucket.
    pub fn halfk(self) -> bool {
        matches!(self, FeatureSet::HalfKav2Hm | FeatureSet::HalfKav2HmState)
    }
}
