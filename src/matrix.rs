//! Lossless matrix tiles. Equal contexts are interned; every retained score
//! is verified against the source. Only a bounded numeric tile cache expands.
use crate::codec;
use crate::index::u32_at;
use std::collections::HashMap;
use std::path::Path;

pub struct Matrix {
    pub rows: usize,
    pub columns: usize,
    row_shift: usize,
    column_shift: usize,
    blocks_x: usize,
    blocks: usize,
    table: usize,
    pool: usize,
}

impl Matrix {
    pub fn new(data: &[u8]) -> Result<Self, ()> {
        if data.get(..8) != Some(b"FJMATR2\0") || u32_at(data, 32)? as usize != data.len() {
            return Err(());
        }
        let rows = u32_at(data, 8)? as usize;
        let columns = u32_at(data, 12)? as usize;
        let row_shift = u32_at(data, 16)? as usize;
        let column_shift = u32_at(data, 20)? as usize;
        if rows == 0
            || columns == 0
            || rows > 8192
            || columns > 8192
            || !(4..=6).contains(&row_shift)
            || !(4..=6).contains(&column_shift)
        {
            return Err(());
        }
        let blocks_x = columns.div_ceil(1 << column_shift);
        let blocks = rows.div_ceil(1 << row_shift) * blocks_x;
        let table = u32_at(data, 24)? as usize;
        let pool = u32_at(data, 28)? as usize;
        if table < 64
            || pool < table + (blocks + 1) * 4
            || pool > data.len()
            || u32_at(data, table)? != 0
            || pool + u32_at(data, table + blocks * 4)? as usize != data.len()
        {
            return Err(());
        }
        Ok(Self {
            rows,
            columns,
            row_shift,
            column_shift,
            blocks_x,
            blocks,
            table,
            pool,
        })
    }

    fn cells(&self) -> usize {
        1 << (self.row_shift + self.column_shift)
    }

    pub fn decode(
        &self,
        data: &[u8],
        block: usize,
        values: &mut Vec<i16>,
        scratch: &mut Vec<u8>,
    ) -> Result<(), ()> {
        if block >= self.blocks {
            return Err(());
        }
        let start = self.pool + u32_at(data, self.table + block * 4)? as usize;
        let end = self.pool + u32_at(data, self.table + (block + 1) * 4)? as usize;
        let record = data.get(start..end).ok_or(())?;
        decode_tile(record, self.row_shift, self.column_shift, values, scratch)
    }
}

fn decode_tile(
    record: &[u8],
    row_shift: usize,
    column_shift: usize,
    values: &mut Vec<i16>,
    scratch: &mut Vec<u8>,
) -> Result<(), ()> {
    let cells = 1 << (row_shift + column_shift);
    let width = 1 << column_shift;
    values.resize(cells, 0);
    match record.first() {
        Some(0) => {
            if record.len() != 1 + cells * 2 {
                return Err(());
            }
            for (value, bytes) in values.iter_mut().zip(record[1..].chunks_exact(2)) {
                *value = i16::from_le_bytes([bytes[0], bytes[1]]);
            }
        }
        Some(1) => {
            codec::decompress(&record[1..], scratch, cells * 3)?;
            let mut position = 0;
            for index in 0..cells {
                let mut zig = 0u32;
                let mut complete = false;
                for shift in [0, 7, 14] {
                    let byte = *scratch.get(position).ok_or(())?;
                    position += 1;
                    zig |= ((byte & 127) as u32) << shift;
                    if byte & 128 == 0 {
                        complete = true;
                        break;
                    }
                }
                if !complete || zig > u16::MAX as u32 {
                    return Err(());
                }
                let delta = (zig >> 1) as i16 ^ -((zig & 1) as i16);
                let left = if index % width == 0 {
                    0
                } else {
                    values[index - 1]
                };
                let above = if index < width {
                    0
                } else {
                    values[index - width]
                };
                let corner = if index < width || index % width == 0 {
                    0
                } else {
                    values[index - width - 1]
                };
                values[index] = delta
                    .wrapping_add(left)
                    .wrapping_add(above)
                    .wrapping_sub(corner);
            }
            if position != scratch.len() {
                return Err(());
            }
        }
        _ => return Err(()),
    }
    Ok(())
}

struct Slot {
    key: usize,
    values: Vec<i16>,
}
#[derive(Clone, Copy)]
struct Score {
    key: u32,
    value: i16,
}
pub struct Cache {
    slots: Vec<Slot>,
    scores: Vec<Score>,
    scratch: Vec<u8>,
}

impl Cache {
    pub fn new(matrix: &Matrix) -> Self {
        let count = (256 * 1024 / (matrix.cells() * 2)).max(1);
        Self {
            slots: (0..count)
                .map(|_| Slot {
                    key: usize::MAX,
                    values: Vec::new(),
                })
                .collect(),
            scratch: Vec::new(),
            scores: vec![
                Score {
                    key: u32::MAX,
                    value: 0
                };
                8192
            ],
        }
    }
    pub fn cost(&mut self, matrix: &Matrix, data: &[u8], left: u16, right: u16) -> Result<i16, ()> {
        let row = left as usize;
        let column = right as usize;
        if row >= matrix.rows || column >= matrix.columns {
            return Err(());
        }
        let score_key = (left as u32) << 13 | right as u32;
        let score_index = (score_key.wrapping_mul(0x9e3779b9) >> 21) as usize * 4;
        // Four-way buckets avoid repeated tile expansion when common context
        // pairs collide. Cache entries contain only IDs and numeric scores.
        for way in 0..4 {
            if self.scores[score_index + way].key == score_key {
                let value = self.scores[score_index + way].value;
                self.scores.swap(score_index, score_index + way);
                return Ok(value);
            }
        }
        let key = (row >> matrix.row_shift) * matrix.blocks_x + (column >> matrix.column_shift);
        let hash = (key as u64).wrapping_mul(0x9e3779b97f4a7c15);
        let index = (hash >> (64 - self.slots.len().trailing_zeros())) as usize;
        let slot = &mut self.slots[index];
        if slot.key != key {
            matrix.decode(data, key, &mut slot.values, &mut self.scratch)?;
            slot.key = key;
        }
        let pixel = ((row & ((1 << matrix.row_shift) - 1)) << matrix.column_shift)
            + (column & ((1 << matrix.column_shift) - 1));
        let value = slot.values.get(pixel).copied().ok_or(())?;
        self.scores
            .copy_within(score_index..score_index + 3, score_index + 1);
        self.scores[score_index] = Score {
            key: score_key,
            value,
        };
        Ok(value)
    }
}

fn encode_tile(tile: &[i16], width: usize) -> Vec<u8> {
    let mut encoded = Vec::new();
    for (index, &current) in tile.iter().enumerate() {
        let left = if index % width == 0 {
            0
        } else {
            tile[index - 1]
        };
        let above = if index < width {
            0
        } else {
            tile[index - width]
        };
        let corner = if index < width || index % width == 0 {
            0
        } else {
            tile[index - width - 1]
        };
        let delta = current
            .wrapping_sub(left)
            .wrapping_sub(above)
            .wrapping_add(corner) as i32;
        let mut zig = ((delta << 1) ^ (delta >> 15)) as u16;
        while zig >= 128 {
            encoded.push((zig as u8 & 127) | 128);
            zig >>= 7;
        }
        encoded.push(zig as u8);
    }
    let compressed = codec::compress(&encoded);
    let mut result = Vec::new();
    if compressed.len() < tile.len() * 2 {
        result.push(1);
        result.extend_from_slice(&compressed);
    } else {
        result.push(0);
        for value in tile {
            result.extend_from_slice(&value.to_le_bytes());
        }
    }
    result
}

pub fn compile(
    source: &Path,
    destination: &Path,
    maps: &Path,
    row_shift: usize,
    column_shift: usize,
) -> Result<(usize, usize, usize), String> {
    let original = std::fs::read(source).map_err(|e| e.to_string())?;
    if original.len() < 4 {
        return Err("short matrix".into());
    }
    let columns = u16::from_le_bytes([original[0], original[1]]) as usize;
    let rows = u16::from_le_bytes([original[2], original[3]]) as usize;
    if rows == 0
        || columns == 0
        || rows > 8192
        || columns > 8192
        || original.len() != 4 + rows * columns * 2
    {
        return Err("invalid matrix dimensions".into());
    }
    let matrix = &original[4..];
    let value = |row: usize, column: usize| -> i16 {
        let offset = (row * columns + column) * 2;
        i16::from_le_bytes([matrix[offset], matrix[offset + 1]])
    };
    let mut row_lookup: HashMap<&[u8], u16> = HashMap::new();
    let mut row_map = Vec::new();
    let mut row_representatives = Vec::new();
    for row in 0..rows {
        let bytes = &matrix[row * columns * 2..(row + 1) * columns * 2];
        let next = row_lookup.len() as u16;
        let mapped = *row_lookup.entry(bytes).or_insert_with(|| {
            row_representatives.push(row);
            next
        });
        row_map.push(mapped);
    }
    let mut hashes = vec![0xcbf29ce484222325u64; columns];
    for &row in &row_representatives {
        for (column, hash) in hashes.iter_mut().enumerate() {
            *hash = (*hash ^ value(row, column) as u16 as u64).wrapping_mul(0x100000001b3);
        }
    }
    let mut column_lookup: HashMap<u64, Vec<usize>> = HashMap::new();
    let mut column_map = Vec::new();
    let mut column_representatives: Vec<usize> = Vec::new();
    for (column, hash) in hashes.into_iter().enumerate() {
        let bucket = column_lookup.entry(hash).or_default();
        let found = bucket.iter().copied().find(|&id| {
            row_representatives
                .iter()
                .all(|&row| value(row, column) == value(row, column_representatives[id]))
        });
        let mapped = if let Some(found) = found {
            found
        } else {
            let id = column_representatives.len();
            column_representatives.push(column);
            bucket.push(id);
            id
        };
        column_map.push(mapped as u16);
    }
    let height = 1 << row_shift;
    let width = 1 << column_shift;
    let blocks_y = row_representatives.len().div_ceil(height);
    let blocks_x = column_representatives.len().div_ceil(width);
    let mut offsets: Vec<u32> = Vec::new();
    let mut pool = Vec::new();
    let mut scratch = Vec::new();
    let mut decoded = Vec::new();
    for y in 0..blocks_y {
        for x in 0..blocks_x {
            let mut tile = vec![0i16; height * width];
            for row in 0..height {
                for column in 0..width {
                    if let (Some(&r), Some(&c)) = (
                        row_representatives.get(y * height + row),
                        column_representatives.get(x * width + column),
                    ) {
                        tile[row * width + column] = value(r, c);
                    }
                }
            }
            let record = encode_tile(&tile, width);
            decode_tile(&record, row_shift, column_shift, &mut decoded, &mut scratch)
                .map_err(|_| "matrix tile decode")?;
            if decoded != tile {
                return Err("matrix score round-trip mismatch".into());
            }
            offsets.push(pool.len() as u32);
            pool.extend_from_slice(&record);
        }
    }
    offsets.push(pool.len() as u32);
    let mut data = vec![0; 64];
    for offset in &offsets {
        data.extend_from_slice(&offset.to_le_bytes());
    }
    let pool_offset = data.len();
    data.extend_from_slice(&pool);
    data[..8].copy_from_slice(b"FJMATR2\0");
    for (offset, number) in [
        (8, row_representatives.len()),
        (12, column_representatives.len()),
        (16, row_shift),
        (20, column_shift),
        (24, 64),
        (28, pool_offset),
        (32, data.len()),
    ] {
        data[offset..offset + 4].copy_from_slice(&(number as u32).to_le_bytes());
    }
    Matrix::new(&data).map_err(|_| "invalid encoded matrix")?;
    // Hashes only locate possible matches. Exact equality above and this
    // exhaustive remap check prevent a collision from changing any score.
    for (row, &mapped_row) in row_map.iter().enumerate() {
        for (column, &mapped_column) in column_map.iter().enumerate() {
            if value(row, column)
                != value(
                    row_representatives[mapped_row as usize],
                    column_representatives[mapped_column as usize],
                )
            {
                return Err("context remap mismatch".into());
            }
        }
    }
    std::fs::write(destination, &data).map_err(|e| e.to_string())?;
    let mut map_data = Vec::new();
    for number in [
        rows,
        columns,
        row_representatives.len(),
        column_representatives.len(),
    ] {
        map_data.extend_from_slice(&(number as u16).to_le_bytes());
    }
    for number in row_map.iter().chain(&column_map) {
        map_data.extend_from_slice(&number.to_le_bytes());
    }
    std::fs::write(maps, map_data).map_err(|e| e.to_string())?;
    Ok((
        row_representatives.len(),
        column_representatives.len(),
        data.len(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tiles_preserve_signed_extremes_and_wrapping_deltas() {
        let tile: Vec<i16> = (0..1024)
            .map(|i| match i % 4 {
                0 => i16::MIN,
                1 => i16::MAX,
                2 => -1,
                _ => 0,
            })
            .collect();
        let encoded = encode_tile(&tile, 32);
        let mut decoded = Vec::new();
        decode_tile(&encoded, 5, 5, &mut decoded, &mut Vec::new()).unwrap();
        assert_eq!(decoded, tile);
    }
}
