//! LOUDS index for the exact byte trie, preserving every terminal value.
//! The source Darts layout follows MeCab's BSD-licensed dictionary format.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

pub fn u32_at(data: &[u8], offset: usize) -> Result<u32, ()> {
    Ok(u32::from_le_bytes(
        data.get(offset..offset + 4)
            .ok_or(())?
            .try_into()
            .map_err(|_| ())?,
    ))
}

fn u64_at(data: &[u8], offset: usize) -> Result<u64, ()> {
    Ok(u64::from_le_bytes(
        data.get(offset..offset + 8)
            .ok_or(())?
            .try_into()
            .map_err(|_| ())?,
    ))
}

pub struct Index {
    pub nodes: usize,
    labels: usize,
    bits: usize,
    samples: usize,
    terminals: usize,
    ranks: usize,
    values: usize,
    value_count: usize,
}

impl Index {
    pub fn new(data: &[u8]) -> Result<Self, ()> {
        if data.get(..8) != Some(b"FJTRIE2\0") || u32_at(data, 48)? as usize != data.len() {
            return Err(());
        }
        let index = Self {
            nodes: u32_at(data, 8)? as usize,
            labels: u32_at(data, 20)? as usize,
            bits: u32_at(data, 24)? as usize,
            samples: u32_at(data, 28)? as usize,
            terminals: u32_at(data, 32)? as usize,
            ranks: u32_at(data, 36)? as usize,
            values: u32_at(data, 40)? as usize,
            value_count: u32_at(data, 44)? as usize,
        };
        if index.nodes == 0 || index.nodes > 8_000_000 || u32_at(data, 16)? != 16 {
            return Err(());
        }
        let sections = [
            (index.labels, index.nodes - 1),
            (index.bits, (2 * index.nodes - 1).div_ceil(64) * 8),
            (index.samples, index.nodes.div_ceil(16) * 4),
            (index.terminals, index.nodes.div_ceil(64) * 8),
            (index.ranks, index.nodes.div_ceil(256) * 4),
            (index.values, index.value_count * 4),
        ];
        let mut previous = 64;
        for (offset, len) in sections {
            if offset < previous || offset + len > data.len() {
                return Err(());
            }
            previous = offset + len;
        }
        Ok(index)
    }

    fn zero(&self, data: &[u8], ordinal: usize) -> Result<usize, ()> {
        if ordinal >= self.nodes {
            return Err(());
        }
        let position = u32_at(data, self.samples + ordinal / 16 * 4)? as usize;
        let mut remaining = ordinal % 16;
        let mut word_index = position / 64;
        let mut word = !u64_at(data, self.bits + word_index * 8)? & (u64::MAX << (position % 64));
        loop {
            let count = word.count_ones() as usize;
            if remaining < count {
                for _ in 0..remaining {
                    word &= word - 1;
                }
                return Ok(word_index * 64 + word.trailing_zeros() as usize);
            }
            remaining -= count;
            word_index += 1;
            word = !u64_at(data, self.bits + word_index * 8)?;
        }
    }

    pub fn children(&self, data: &[u8], node: usize) -> Result<(usize, usize), ()> {
        if node >= self.nodes {
            return Err(());
        }
        let start = if node == 0 {
            0
        } else {
            self.zero(data, node - 1)? + 1
        };
        let mut position = start;
        let mut degree = 0;
        loop {
            let shift = position % 64;
            let ones =
                (u64_at(data, self.bits + position / 64 * 8)? >> shift).trailing_ones() as usize;
            degree += ones;
            if ones < 64 - shift {
                break;
            }
            position += ones;
            if degree > 256 {
                return Err(());
            }
        }
        let first = start.checked_sub(node).ok_or(())? + 1;
        if first + degree > self.nodes {
            return Err(());
        }
        Ok((first, degree))
    }

    pub fn child(&self, data: &[u8], node: usize, label: u8) -> Result<Option<usize>, ()> {
        let (first, degree) = self.children(data, node)?;
        let labels = data
            .get(self.labels + first - 1..self.labels + first - 1 + degree)
            .ok_or(())?;
        let found = if degree <= 8 {
            labels.iter().position(|&c| c == label)
        } else {
            labels.binary_search(&label).ok()
        };
        Ok(found.map(|index| first + index))
    }

    pub fn terminal(&self, data: &[u8], node: usize) -> Result<Option<u32>, ()> {
        if node >= self.nodes {
            return Err(());
        }
        let word_index = node / 64;
        let word = u64_at(data, self.terminals + word_index * 8)?;
        if word & (1 << (node % 64)) == 0 {
            return Ok(None);
        }
        let mut rank = u32_at(data, self.ranks + node / 256 * 4)? as usize;
        for index in node / 256 * 4..word_index {
            rank += u64_at(data, self.terminals + index * 8)?.count_ones() as usize;
        }
        let mask = if node % 64 == 63 {
            u64::MAX
        } else {
            (1 << (node % 64 + 1)) - 1
        };
        rank += (word & mask).count_ones() as usize;
        if rank == 0 || rank > self.value_count {
            return Err(());
        }
        Ok(Some(u32_at(data, self.values + (rank - 1) * 4)?))
    }
}

#[derive(Default)]
struct Bits {
    data: Vec<u8>,
    len: usize,
}
impl Bits {
    fn push(&mut self, bit: bool) {
        if self.len.is_multiple_of(8) {
            self.data.push(0);
        }
        if bit {
            self.data[self.len / 8] |= 1 << (self.len % 8);
        }
        self.len += 1;
    }
    fn padded(&mut self) {
        while !self.data.len().is_multiple_of(8) {
            self.data.push(0);
        }
    }
}

fn append(data: &mut Vec<u8>, value: &[u8]) -> usize {
    while !data.len().is_multiple_of(8) {
        data.push(0);
    }
    let offset = data.len();
    data.extend_from_slice(value);
    offset
}

pub fn compile(source: &Path, destination: &Path) -> Result<(usize, usize, usize), String> {
    let mut file = File::open(source).map_err(|e| e.to_string())?;
    let mut header = [0u8; 72];
    file.read_exact(&mut header).map_err(|e| e.to_string())?;
    let size = u32_at(&header, 24).map_err(|_| "dictionary header")? as usize;
    if !size.is_multiple_of(8) || size > 32_000_000 {
        return Err("invalid source trie length".into());
    }
    let mut darts = vec![0; size];
    file.seek(SeekFrom::Start(72)).map_err(|e| e.to_string())?;
    file.read_exact(&mut darts).map_err(|e| e.to_string())?;
    let base = |index: usize| -> Result<i32, String> {
        u32_at(&darts, index * 8)
            .map(|n| n as i32)
            .map_err(|_| "invalid Darts base".into())
    };
    let check = |index: usize| -> Result<u32, String> {
        u32_at(&darts, index * 8 + 4).map_err(|_| "invalid Darts check".into())
    };
    let mut states = vec![base(0)? as u32];
    let mut labels = Vec::new();
    let mut degrees = Vec::new();
    let mut bits = Bits::default();
    let mut terminals = Bits::default();
    let mut samples: Vec<u32> = Vec::new();
    let mut values: Vec<u32> = Vec::new();
    let mut index = 0;
    while index < states.len() {
        if states.len() > size / 8 {
            return Err("cyclic source trie".into());
        }
        let state = states[index] as usize;
        let terminal = if check(state)? == state as u32 && base(state)? < 0 {
            Some((-base(state)? - 1) as u32)
        } else {
            None
        };
        terminals.push(terminal.is_some());
        if let Some(value) = terminal {
            values.push(value);
        }
        let mut degree = 0u16;
        for code in 1..=256 {
            let position = state + code;
            if position < size / 8 && check(position)? == state as u32 {
                let next = base(position)?;
                if next <= 0 {
                    return Err("invalid source child".into());
                }
                states.push(next as u32);
                labels.push((code - 1) as u8);
                bits.push(true);
                degree += 1;
            }
        }
        if index % 16 == 0 {
            samples.push(bits.len as u32);
        }
        bits.push(false);
        degrees.push(degree);
        index += 1;
    }
    let nodes = states.len();
    bits.padded();
    terminals.padded();
    let mut ranks: Vec<u32> = Vec::new();
    let mut count = 0;
    for bytes in terminals.data.chunks(32) {
        ranks.push(count);
        count += bytes.iter().map(|b| b.count_ones()).sum::<u32>();
    }
    let integers =
        |values: &[u32]| -> Vec<u8> { values.iter().flat_map(|n| n.to_le_bytes()).collect() };
    let mut data = vec![0; 64];
    let sections = [
        append(&mut data, &labels),
        append(&mut data, &bits.data),
        append(&mut data, &integers(&samples)),
        append(&mut data, &terminals.data),
        append(&mut data, &integers(&ranks)),
        append(&mut data, &integers(&values)),
    ];
    data[..8].copy_from_slice(b"FJTRIE2\0");
    let fields = [
        (8, nodes),
        (12, bits.len),
        (16, 16),
        (20, sections[0]),
        (24, sections[1]),
        (28, sections[2]),
        (32, sections[3]),
        (36, sections[4]),
        (40, sections[5]),
        (44, values.len()),
        (48, data.len()),
    ];
    for (offset, value) in fields {
        data[offset..offset + 4].copy_from_slice(&(value as u32).to_le_bytes());
    }
    let reader = Index::new(&data).map_err(|_| "invalid encoded trie")?;
    // Check every state/terminal and every child edge against the original
    // graph. This establishes full index coverage, beyond sample sentences.
    let mut next_child = 1;
    for (node, (&state, &degree)) in states.iter().zip(&degrees).enumerate() {
        let expected = if check(state as usize)? == state && base(state as usize)? < 0 {
            Some((-base(state as usize)? - 1) as u32)
        } else {
            None
        };
        if reader
            .terminal(&data, node)
            .map_err(|_| "terminal decode")?
            != expected
            || reader.children(&data, node).map_err(|_| "child decode")?
                != (next_child, degree as usize)
        {
            return Err("trie round-trip mismatch".into());
        }
        for child in next_child..next_child + degree as usize {
            if reader
                .child(&data, node, labels[child - 1])
                .map_err(|_| "edge decode")?
                != Some(child)
            {
                return Err("trie edge mismatch".into());
            }
        }
        next_child += degree as usize;
    }
    File::create(destination)
        .and_then(|mut out| out.write_all(&data))
        .map_err(|e| e.to_string())?;
    Ok((nodes, values.len(), data.len()))
}
