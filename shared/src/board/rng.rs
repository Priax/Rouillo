use bitcode::{Decode, Encode};
use rand_chacha::ChaCha12Rng;
use serde::{Deserialize, Serialize};

/// The board's two random streams, kept as their seed and how far each went:
/// plain data, sent whole in every update, and a lost one costs nothing.
#[derive(Serialize, Deserialize, Encode, Decode, Debug, Clone, Default)]
pub(super) struct BoardRng {
    seed: u64,
    pieces: u64,
    pub(super) garbage: u64,
    /// Each stream as last drawn from, kept while it is where its position
    /// says: rebuilding one costs more than a draw.
    #[serde(skip)]
    #[bitcode(skip)]
    cache: [Option<ChaCha12Rng>; 2],
}

#[derive(Clone, Copy)]
pub(super) enum Stream {
    Pieces,
    Garbage,
}

impl std::hash::Hash for BoardRng {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (self.seed, self.pieces, self.garbage).hash(state);
    }
}

impl BoardRng {
    pub(super) const fn from_seed(seed: u64) -> Self {
        Self {
            seed,
            pieces: 0,
            garbage: 0,
            cache: [None, None],
        }
    }

    const fn pos(&self, stream: Stream) -> u64 {
        match stream {
            Stream::Pieces => self.pieces,
            Stream::Garbage => self.garbage,
        }
    }

    fn stream(&self, stream: Stream) -> ChaCha12Rng {
        use rand::SeedableRng;
        let mut rng = ChaCha12Rng::seed_from_u64(self.seed);
        if matches!(stream, Stream::Garbage) {
            rng.set_stream(1);
        }
        rng.set_word_pos(u128::from(self.pos(stream)));
        rng
    }

    pub(super) fn draw<T>(&mut self, stream: Stream, f: impl FnOnce(&mut ChaCha12Rng) -> T) -> T {
        let at = u128::from(self.pos(stream));
        let mut rng = match self.cache[stream as usize].take() {
            Some(rng) if rng.get_word_pos() == at => rng,
            _ => self.stream(stream),
        };
        let value = f(&mut rng);
        let pos = u64::try_from(rng.get_word_pos()).unwrap_or(u64::MAX);
        match stream {
            Stream::Pieces => self.pieces = pos,
            Stream::Garbage => self.garbage = pos,
        }
        self.cache[stream as usize] = Some(rng);
        value
    }
}
