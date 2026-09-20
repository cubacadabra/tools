use crate::{BuildError, Result};

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Cell {
    pub(super) north: bool,
    pub(super) east: bool,
    pub(super) south: bool,
    pub(super) west: bool,
}

#[derive(Clone, Copy)]
struct Direction {
    dx: isize,
    dy: isize,
    wall: usize,
    opposite: usize,
}

const DIRECTIONS: [Direction; 4] = [
    Direction {
        dx: 0,
        dy: -1,
        wall: 0,
        opposite: 2,
    },
    Direction {
        dx: 1,
        dy: 0,
        wall: 1,
        opposite: 3,
    },
    Direction {
        dx: 0,
        dy: 1,
        wall: 2,
        opposite: 0,
    },
    Direction {
        dx: -1,
        dy: 0,
        wall: 3,
        opposite: 1,
    },
];
pub(super) fn carve(width: usize, height: usize, seed: u32) -> Vec<Cell> {
    let mut cells = vec![
        Cell {
            north: true,
            east: true,
            south: true,
            west: true
        };
        width * height
    ];
    let mut visited = vec![false; width * height];
    let mut stack = vec![(0usize, 0usize)];
    let mut rng = PythonRandom::new(seed);
    visited[0] = true;
    while let Some(&(x, y)) = stack.last() {
        let choices: Vec<_> = DIRECTIONS
            .iter()
            .copied()
            .filter(|direction| {
                let next_x = x as isize + direction.dx;
                let next_y = y as isize + direction.dy;
                next_x >= 0
                    && next_x < width as isize
                    && next_y >= 0
                    && next_y < height as isize
                    && !visited[next_y as usize * width + next_x as usize]
            })
            .collect();
        if choices.is_empty() {
            stack.pop();
            continue;
        }
        let direction = choices[rng.randbelow(choices.len())];
        let next_x = (x as isize + direction.dx) as usize;
        let next_y = (y as isize + direction.dy) as usize;
        cells[y * width + x].set(direction.wall, false);
        cells[next_y * width + next_x].set(direction.opposite, false);
        visited[next_y * width + next_x] = true;
        stack.push((next_x, next_y));
    }
    cells
}

impl Cell {
    fn set(&mut self, wall: usize, value: bool) {
        match wall {
            0 => self.north = value,
            1 => self.east = value,
            2 => self.south = value,
            3 => self.west = value,
            _ => unreachable!(),
        }
    }
    fn open(&self, wall: usize) -> bool {
        match wall {
            0 => !self.north,
            1 => !self.east,
            2 => !self.south,
            3 => !self.west,
            _ => false,
        }
    }
}

pub(super) fn route(
    cells: &[Cell],
    width: usize,
    height: usize,
    start: (usize, usize),
    finish: (usize, usize),
) -> Result<Vec<(usize, usize)>> {
    let mut previous = vec![None; width * height];
    let mut queue = std::collections::VecDeque::from([start]);
    previous[start.1 * width + start.0] = Some(start);
    while let Some((x, y)) = queue.pop_front() {
        if (x, y) == finish {
            break;
        }
        for direction in DIRECTIONS {
            let next_x = x as isize + direction.dx;
            let next_y = y as isize + direction.dy;
            if next_x < 0
                || next_x >= width as isize
                || next_y < 0
                || next_y >= height as isize
                || !cells[y * width + x].open(direction.wall)
            {
                continue;
            }
            let next = (next_x as usize, next_y as usize);
            let slot = next.1 * width + next.0;
            if previous[slot].is_none() {
                previous[slot] = Some((x, y));
                queue.push_back(next);
            }
        }
    }
    if previous[finish.1 * width + finish.0].is_none() {
        return Err(BuildError(
            "manifest.maze generated a maze without a start-to-finish path".to_owned(),
        ));
    }
    let mut result = Vec::new();
    let mut cursor = finish;
    loop {
        result.push(cursor);
        if cursor == start {
            break;
        }
        cursor = previous[cursor.1 * width + cursor.0].unwrap();
    }
    result.reverse();
    Ok(result)
}

pub(super) struct PythonRandom {
    state: [u32; 624],
    index: usize,
}

impl PythonRandom {
    pub(super) fn new(seed: u32) -> Self {
        let mut state = [0u32; 624];
        state[0] = 19650218;
        for i in 1..624 {
            state[i] = (1812433253u64
                .wrapping_mul((state[i - 1] ^ (state[i - 1] >> 30)) as u64)
                .wrapping_add(i as u64)) as u32;
        }
        let key = [seed];
        let mut i = 1usize;
        let mut j = 0usize;
        let mut k = 624usize;
        while k > 0 {
            state[i] = (state[i] ^ ((state[i - 1] ^ (state[i - 1] >> 30)).wrapping_mul(1664525)))
                .wrapping_add(key[j])
                .wrapping_add(j as u32);
            i += 1;
            j += 1;
            if i >= 624 {
                state[0] = state[623];
                i = 1;
            }
            if j >= key.len() {
                j = 0;
            }
            k -= 1;
        }
        k = 623;
        while k > 0 {
            state[i] = (state[i]
                ^ ((state[i - 1] ^ (state[i - 1] >> 30)).wrapping_mul(1566083941)))
            .wrapping_sub(i as u32);
            i += 1;
            if i >= 624 {
                state[0] = state[623];
                i = 1;
            }
            k -= 1;
        }
        state[0] = 0x8000_0000;
        Self { state, index: 624 }
    }

    fn next_u32(&mut self) -> u32 {
        if self.index >= 624 {
            for i in 0..624 {
                let y = (self.state[i] & 0x8000_0000) | (self.state[(i + 1) % 624] & 0x7fff_ffff);
                self.state[i] = self.state[(i + 397) % 624]
                    ^ (y >> 1)
                    ^ if y & 1 != 0 { 0x9908_b0df } else { 0 };
            }
            self.index = 0;
        }
        let mut y = self.state[self.index];
        self.index += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^= y >> 18;
        y
    }

    fn randbelow(&mut self, upper: usize) -> usize {
        assert!(upper > 0, "random upper bound must be positive");
        // Python's _randbelow uses ``n.bit_length()``, including one bit for
        // an upper bound of one.
        let bits = (usize::BITS - upper.leading_zeros()).min(32) as usize;
        loop {
            let value = (self.next_u32() >> (32 - bits)) as usize;
            if value < upper {
                return value;
            }
        }
    }

    pub(super) fn shuffle<T>(&mut self, values: &mut [T]) {
        for index in (1..values.len()).rev() {
            values.swap(index, self.randbelow(index + 1));
        }
    }
}
