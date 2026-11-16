pub type Set = Vec<u64>;

pub fn empty(size: usize) -> Set {
    vec![0; size.div_ceil(64)]
}

pub fn put(set: &mut Set, at: usize) {
    set[at / 64] |= 1 << (at % 64);
}

pub fn count(set: &Set) -> usize {
    set.iter().map(|word| word.count_ones() as usize).sum()
}

pub fn join(into: &mut Set, other: &Set) {
    for (word, more) in into.iter_mut().zip(other) {
        *word |= more;
    }
}

pub fn gain(covered: &Set, set: &Set) -> usize {
    covered
        .iter()
        .zip(set)
        .map(|(done, word)| (word & !done).count_ones() as usize)
        .sum()
}

pub fn within(set: &Set, other: &Set) -> bool {
    set.iter().zip(other).all(|(word, more)| word & !more == 0)
}

pub fn greedy(sets: &[(&Set, usize)]) -> Vec<usize> {
    let size = sets.first().map_or(0, |(set, _)| set.len());
    let mut covered = vec![0; size];
    let mut chosen = Vec::new();
    loop {
        let best = sets
            .iter()
            .enumerate()
            .map(|(at, (set, cost))| (gain(&covered, set), std::cmp::Reverse(*cost), at))
            .max();
        match best {
            Some((gained, _, at)) if gained > 0 => {
                join(&mut covered, sets[at].0);
                chosen.push(at);
            }
            _ => return chosen,
        }
    }
}
