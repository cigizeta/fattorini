use fattorini::features::{halfk, psq, psq_threats};

/// Which feature encoding training builds its `Network`/`DenseBatch` around (`features.rs` holds
/// each one's design). Every variant is derived, at training time, from the same
/// feature-set-agnostic `CanonicalPosition` every stream and test set holds (`features.rs`'s own doc
/// comment).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeatureSet {
    Psq,
    /// Psq's rows plus each perspective's own threats (`fattorini::features::psq_threats`).
    PsqThreats,
    /// Stockfish's HalfKAv2_hm with psq_threats' threats (`fattorini::features::halfk`), its king
    /// buckets the layout's, each piece row followed in training by its factor row
    /// (`Layout::training_rows`).
    HalfKav2Hm(halfk::Layout),
    /// `HalfKav2Hm` with psq's 12 castling and en passant rows after the factor rows
    /// (`Layout::training_rows_with_state`, `halfk::push_state_rows`): halfk_threats_pairwise_hidden's.
    HalfKav2HmState(halfk::Layout),
}

impl FeatureSet {
    /// Whether this is one of halfk's: factorised, its piece rows in the king's bucket.
    pub fn halfk(self) -> bool {
        self.layout().is_some()
    }

    /// A halfk feature set's king layout, `None` for the others.
    pub fn layout(self) -> Option<halfk::Layout> {
        match self {
            FeatureSet::HalfKav2Hm(layout) | FeatureSet::HalfKav2HmState(layout) => Some(layout),
            FeatureSet::Psq | FeatureSet::PsqThreats => None,
        }
    }

    /// The transform's rows in training.
    pub fn training_rows(self) -> usize {
        match self {
            FeatureSet::Psq => psq::ROWS,
            FeatureSet::PsqThreats => psq_threats::ROWS,
            FeatureSet::HalfKav2Hm(layout) => layout.training_rows(),
            FeatureSet::HalfKav2HmState(layout) => layout.training_rows_with_state(),
        }
    }
}
