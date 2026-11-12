//! Seeded generators. The algorithm and the seed decide the generated
//! workload, so neither may change under a test that pins what it produced.

/// xorshift32.
pub struct Generator(pub u32);

impl Generator {
    pub fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }

    pub fn index(&mut self, length: usize) -> usize {
        self.next() as usize % length
    }

    pub fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.index(items.len())]
    }

    pub fn chance(&mut self, one_in: usize) -> bool {
        self.index(one_in) == 0
    }
}

/// xorshift64.
pub struct Generator64(pub u64);

impl Generator64 {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    pub fn below(&mut self, bound: usize) -> usize {
        let bound = u64::try_from(bound).expect("bounds fit u64");
        usize::try_from(self.next() % bound).expect("remainders fit usize")
    }

    pub fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.below(items.len())]
    }
}
