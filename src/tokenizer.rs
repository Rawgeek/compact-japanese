//! MeCab-compatible one-best Viterbi over the compact immutable dictionary.
//! Candidate insertion order, strict tie-breaking and unknown-word grouping
//! follow MeCab 0.996. See LICENSE.mecab for the BSD ownership notice.

use crate::index::{Index, u32_at};
use crate::matrix::{Cache, Matrix};
use std::ffi::{c_int, c_void};
use std::fs::File;
use std::os::fd::AsRawFd;
use std::path::Path;
use std::ptr::NonNull;
use std::sync::Arc;

unsafe extern "C" {
    fn mmap(
        address: *mut c_void,
        length: usize,
        protection: c_int,
        flags: c_int,
        file: c_int,
        offset: i64,
    ) -> *mut c_void;
    fn munmap(address: *mut c_void, length: usize) -> c_int;
}

struct Mapped {
    pointer: NonNull<u8>,
    length: usize,
}
unsafe impl Send for Mapped {}
unsafe impl Sync for Mapped {}
impl Mapped {
    fn open(path: &Path) -> Result<Self, ()> {
        let file = File::open(path).map_err(|_| ())?;
        let length = usize::try_from(file.metadata().map_err(|_| ())?.len()).map_err(|_| ())?;
        if !(128..32_000_000).contains(&length) {
            return Err(());
        }
        // PROT_READ, MAP_PRIVATE. The deployment file is immutable; replacing
        // an artifact leaves the mapped inode alive until its final owner drops.
        let pointer = unsafe { mmap(std::ptr::null_mut(), length, 1, 2, file.as_raw_fd(), 0) };
        if pointer as isize == -1 {
            return Err(());
        }
        Ok(Self {
            pointer: NonNull::new(pointer.cast()).ok_or(())?,
            length,
        })
    }
    fn bytes(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.pointer.as_ptr(), self.length) }
    }
}
impl Drop for Mapped {
    fn drop(&mut self) {
        unsafe {
            munmap(self.pointer.as_ptr().cast(), self.length);
        }
    }
}

#[derive(Clone, Copy)]
struct Section {
    start: usize,
    length: usize,
}

pub struct Engine {
    mapped: Mapped,
    sections: [Section; 5],
    index: Index,
    matrix: Matrix,
    words: usize,
    tokens: usize,
    categories: usize,
}

impl Engine {
    pub fn open(path: &Path) -> Result<Arc<Self>, ()> {
        let mapped = Mapped::open(path)?;
        let bytes = mapped.bytes();
        if bytes.get(..8) != Some(b"FJPN2\0\0\0")
            || u32_at(bytes, 8)? != 2
            || u32_at(bytes, 12)? != 756264
            || u32_at(bytes, 20)? != 5
            || u32_at(bytes, 24)? as usize != bytes.len()
        {
            return Err(());
        }
        let words = u32_at(bytes, 12)? as usize;
        let tokens = u32_at(bytes, 16)? as usize;
        if tokens < words || tokens > words + 1024 {
            return Err(());
        }
        let mut sections = [Section {
            start: 0,
            length: 0,
        }; 5];
        let mut end = 128;
        for (i, section) in sections.iter_mut().enumerate() {
            section.start = u32_at(bytes, 32 + i * 8)? as usize;
            section.length = u32_at(bytes, 36 + i * 8)? as usize;
            if section.start < end || section.start + section.length > bytes.len() {
                return Err(());
            }
            end = section.start + section.length;
        }
        if sections[1].length != tokens * 8 || sections[2].length >= 1 << 22 {
            return Err(());
        }
        let get =
            |section: Section| -> &[u8] { &bytes[section.start..section.start + section.length] };
        let index = Index::new(get(sections[0]))?;
        let matrix = Matrix::new(get(sections[3]))?;
        let characters = get(sections[4]);
        let categories = u32_at(characters, 0)? as usize;
        if categories == 0
            || categories > 256
            || u32_at(characters, 4)? != 65535
            || characters.len() != 8 + categories * 8 + 65535 * 4
        {
            return Err(());
        }
        Ok(Arc::new(Self {
            mapped,
            sections,
            index,
            matrix,
            words,
            tokens,
            categories,
        }))
    }
    fn section(&self, index: usize) -> &[u8] {
        let s = self.sections[index];
        &self.mapped.bytes()[s.start..s.start + s.length]
    }
    fn token(&self, index: usize) -> Result<u64, ()> {
        if index >= self.tokens {
            return Err(());
        }
        Ok(u64::from_le_bytes(
            self.section(1)
                .get(index * 8..index * 8 + 8)
                .ok_or(())?
                .try_into()
                .map_err(|_| ())?,
        ))
    }
    pub fn feature(&self, offset: u32) -> Result<(u8, &str), ()> {
        let data = self.section(2).get(offset as usize..).ok_or(())?;
        let flags = *data.first().ok_or(())?;
        let end = data[1..].iter().position(|&byte| byte == 0).ok_or(())? + 1;
        Ok((flags, std::str::from_utf8(&data[1..end]).map_err(|_| ())?))
    }
    fn character(&self, text: &str, offset: usize) -> Result<(u32, usize), ()> {
        let ch = text.get(offset..).ok_or(())?.chars().next().ok_or(())?;
        // MeCab's UTF-8 -> UCS-2 reader maps supplementary characters to zero.
        let code = if ch as u32 >= 65535 { 0 } else { ch as usize };
        let value = u32_at(self.section(4), 8 + self.categories * 8 + code * 4)?;
        Ok((value, ch.len_utf8()))
    }
    fn unknown(&self, category: u8) -> Result<(usize, usize), ()> {
        if category as usize >= self.categories {
            return Err(());
        }
        let start = u32_at(self.section(4), 8 + category as usize * 8)? as usize;
        let count = u32_at(self.section(4), 12 + category as usize * 8)? as usize;
        if count == 0 || start < self.words || start + count > self.tokens {
            return Err(());
        }
        Ok((start, count))
    }
}

const NONE: u32 = u32::MAX;
const MAX_NODES: usize = 262144;
const TYPES: u32 = (1 << 18) - 1;

#[derive(Clone, Copy)]
struct Candidate {
    packed: u64,
    start: u16,
    end: u16,
    kind: u8,
    unknown: bool,
    whitespace: bool,
}

#[derive(Clone, Copy)]
struct LatticeNode {
    cost: i64,
    previous: u32,
    next_end: u32,
    feature: u32,
    start: u16,
    end: u16,
    right: u16,
    kind: u8,
    unknown: bool,
    whitespace: bool,
}

#[derive(Clone, Copy)]
pub struct Parsed {
    pub start: usize,
    pub end: usize,
    pub feature: u32,
    pub kind: u8,
    pub unknown: bool,
    pub whitespace: bool,
}

pub struct Parser {
    pub engine: Arc<Engine>,
    cache: Cache,
    ends: Vec<u32>,
    nodes: Vec<LatticeNode>,
    candidates: Vec<Candidate>,
    best: Vec<Parsed>,
}

impl Parser {
    pub fn new(engine: Arc<Engine>) -> Result<Self, ()> {
        Ok(Self {
            cache: Cache::new(&engine.matrix),
            engine,
            ends: Vec::new(),
            nodes: Vec::new(),
            candidates: Vec::new(),
            best: Vec::new(),
        })
    }

    fn add_unknown(
        &mut self,
        start: usize,
        end: usize,
        position: usize,
        kind: u8,
    ) -> Result<(), ()> {
        let (first, count) = self.engine.unknown(kind)?;
        for token in first..first + count {
            self.candidates.push(Candidate {
                packed: self.engine.token(token)?,
                start: start as u16,
                end: end as u16,
                kind,
                unknown: true,
                whitespace: start != position,
            });
        }
        Ok(())
    }

    fn lookup(&mut self, text: &str, position: usize) -> Result<(), ()> {
        self.candidates.clear();
        let mut start = position;
        let (space, _) = self.engine.character(" ", 0)?;
        let mut previous = space;
        while start < text.len() {
            let (info, len) = self.engine.character(text, start)?;
            if previous & info & TYPES == 0 {
                break;
            }
            previous = info;
            start += len;
        }
        if start == text.len() {
            return Ok(());
        }
        let (info, length) = self.engine.character(text, start)?;
        let kind = ((info >> 18) & 255) as u8;
        let mut node = 0;
        for (length, &byte) in text.as_bytes()[start..].iter().enumerate() {
            let Some(next) = self
                .engine
                .index
                .child(self.engine.section(0), node, byte)?
            else {
                break;
            };
            node = next;
            if let Some(range) = self.engine.index.terminal(self.engine.section(0), node)? {
                let first = (range >> 8) as usize;
                let count = (range & 255) as usize;
                if count == 0 || first + count > self.engine.words {
                    return Err(());
                }
                for token in first..first + count {
                    self.candidates.push(Candidate {
                        packed: self.engine.token(token)?,
                        start: start as u16,
                        end: (start + length + 1) as u16,
                        kind,
                        unknown: false,
                        whitespace: start != position,
                    });
                }
            }
        }
        if !self.candidates.is_empty() && info & (1 << 31) == 0 {
            return Ok(());
        }
        let first_end = start + length;
        let mut group_end = None;
        if info & (1 << 30) != 0 {
            let mut end = first_end;
            let mut previous = info;
            let mut count = 0;
            while end < text.len() {
                let (next, len) = self.engine.character(text, end)?;
                if previous & next & TYPES == 0 {
                    break;
                }
                previous = next;
                end += len;
                count += 1;
            }
            if count <= 24 {
                self.add_unknown(start, end, position, kind)?;
            }
            group_end = Some(end);
        }
        let mut end = first_end;
        for _ in 1..=((info >> 26) & 15) {
            if end > text.len() {
                break;
            }
            if group_end == Some(end) {
                continue;
            }
            self.add_unknown(start, end, position, kind)?;
            if end == text.len() {
                break;
            }
            let (next, len) = self.engine.character(text, end)?;
            if info & next & TYPES == 0 {
                break;
            }
            end += len;
        }
        if self.candidates.is_empty() {
            self.add_unknown(start, end, position, kind)?;
        }
        Ok(())
    }

    pub fn analyze(&mut self, text: &str) -> Result<&[Parsed], ()> {
        self.ends.clear();
        self.ends.resize(text.len() + 1, NONE);
        self.nodes.clear();
        self.best.clear();
        self.nodes.push(LatticeNode {
            cost: 0,
            previous: NONE,
            next_end: NONE,
            feature: 0,
            start: 0,
            end: 0,
            right: 0,
            kind: 0,
            unknown: false,
            whitespace: false,
        });
        self.ends[0] = 0;
        for position in 0..text.len() {
            if self.ends[position] == NONE {
                continue;
            }
            self.lookup(text, position)?;
            // MeCab prepends candidates during lookup and prepends connected
            // nodes at each end position. Preserve both orders for exact ties.
            for candidate in self.candidates.iter().rev() {
                let left = (candidate.packed & 8191) as u16;
                let right = ((candidate.packed >> 13) & 8191) as u16;
                let word_cost = ((candidate.packed >> 26) as u16) as i16;
                let mut previous = self.ends[position];
                let mut best_cost = i64::MAX;
                let mut best = NONE;
                while previous != NONE {
                    let node = &self.nodes[previous as usize];
                    let cost = node.cost
                        + word_cost as i64
                        + self.cache.cost(
                            &self.engine.matrix,
                            self.engine.section(3),
                            left,
                            node.right,
                        )? as i64;
                    if cost < best_cost {
                        best_cost = cost;
                        best = previous;
                    }
                    previous = node.next_end;
                }
                let end = candidate.end as usize;
                if best == NONE || end > text.len() || self.nodes.len() >= MAX_NODES {
                    return Err(());
                }
                let node = LatticeNode {
                    cost: best_cost,
                    previous: best,
                    next_end: self.ends[end],
                    feature: (candidate.packed >> 42) as u32,
                    start: candidate.start,
                    end: candidate.end,
                    right,
                    kind: candidate.kind,
                    unknown: candidate.unknown,
                    whitespace: candidate.whitespace,
                };
                self.ends[end] = self.nodes.len() as u32;
                self.nodes.push(node);
            }
        }
        let position = self.ends.iter().rposition(|&head| head != NONE).ok_or(())?;
        let mut previous = self.ends[position];
        let mut best_cost = i64::MAX;
        let mut best = NONE;
        while previous != NONE {
            let node = &self.nodes[previous as usize];
            let cost = node.cost
                + self
                    .cache
                    .cost(&self.engine.matrix, self.engine.section(3), 0, node.right)?
                    as i64;
            if cost < best_cost {
                best_cost = cost;
                best = previous;
            }
            previous = node.next_end;
        }
        while best != 0 && best != NONE {
            let node = &self.nodes[best as usize];
            self.best.push(Parsed {
                start: node.start as usize,
                end: node.end as usize,
                feature: node.feature,
                kind: node.kind,
                unknown: node.unknown,
                whitespace: node.whitespace,
            });
            best = node.previous;
        }
        if best == NONE {
            return Err(());
        }
        self.best.reverse();
        Ok(&self.best)
    }

    pub fn finish(&mut self) {
        self.nodes.clear();
        self.candidates.clear();
        self.best.clear();
        self.ends.clear();
        if self.nodes.capacity() > 8192 {
            self.nodes.shrink_to(8192);
        }
        if self.ends.capacity() > 4096 {
            self.ends.shrink_to(4096);
        }
        if self.best.capacity() > 1024 {
            self.best.shrink_to(1024);
        }
        if self.candidates.capacity() > 1024 {
            self.candidates.shrink_to(1024);
        }
    }
}
