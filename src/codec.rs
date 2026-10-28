//! Small LZ4 block codec. Matrix tiles are bounded and independently decoded.
use std::collections::HashMap;

pub fn compress(data: &[u8]) -> Vec<u8> {
    let mut output = Vec::new();
    let mut table = HashMap::new();
    let mut anchor = 0;
    let mut index = 0;
    fn length(output: &mut Vec<u8>, mut value: usize) {
        while value >= 255 {
            output.push(255);
            value -= 255;
        }
        output.push(value as u8);
    }
    while index + 12 <= data.len() {
        let word = u32::from_le_bytes(data[index..index + 4].try_into().unwrap());
        let previous = table.insert(word, index);
        let Some(previous) = previous.filter(|&p| index - p <= 65535) else {
            index += 1;
            continue;
        };
        let mut end = index + 4;
        while end < data.len() - 5 && data[previous + end - index] == data[end] {
            end += 1;
        }
        let literal = index - anchor;
        let matched = end - index - 4;
        output.push(((literal.min(15) << 4) | matched.min(15)) as u8);
        if literal >= 15 {
            length(&mut output, literal - 15);
        }
        output.extend_from_slice(&data[anchor..index]);
        output.extend_from_slice(&((index - previous) as u16).to_le_bytes());
        if matched >= 15 {
            length(&mut output, matched - 15);
        }
        index = end;
        anchor = end;
    }
    let literal = data.len() - anchor;
    output.push((literal.min(15) << 4) as u8);
    if literal >= 15 {
        length(&mut output, literal - 15);
    }
    output.extend_from_slice(&data[anchor..]);
    output
}

pub fn decompress(data: &[u8], output: &mut Vec<u8>, limit: usize) -> Result<(), ()> {
    output.clear();
    let mut index = 0;
    fn length(data: &[u8], index: &mut usize, initial: usize, limit: usize) -> Result<usize, ()> {
        let mut value = initial;
        if initial == 15 {
            loop {
                let byte = *data.get(*index).ok_or(())? as usize;
                *index += 1;
                value = value.checked_add(byte).ok_or(())?;
                if value > limit {
                    return Err(());
                }
                if byte != 255 {
                    break;
                }
            }
        }
        Ok(value)
    }
    while index < data.len() {
        let token = data[index];
        index += 1;
        let literal = length(data, &mut index, (token >> 4) as usize, limit)?;
        if output.len() + literal > limit {
            return Err(());
        }
        output.extend_from_slice(data.get(index..index + literal).ok_or(())?);
        index += literal;
        if index == data.len() {
            return Ok(());
        }
        let offset = u16::from_le_bytes(
            data.get(index..index + 2)
                .ok_or(())?
                .try_into()
                .map_err(|_| ())?,
        ) as usize;
        index += 2;
        if offset == 0 || offset > output.len() {
            return Err(());
        }
        let matched = length(data, &mut index, (token & 15) as usize, limit)? + 4;
        if output.len() + matched > limit {
            return Err(());
        }
        for _ in 0..matched {
            output.push(output[output.len() - offset]);
        }
    }
    if data.is_empty() { Err(()) } else { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repetitive_and_incompressible_blocks_round_trip() {
        for data in [
            vec![0; 4096],
            (0..4096).map(|i| ((i * 37 + i / 31) % 251) as u8).collect(),
            b"abcabcabcabcabc hello world".to_vec(),
        ] {
            let encoded = compress(&data);
            let mut decoded = Vec::new();
            decompress(&encoded, &mut decoded, data.len()).unwrap();
            assert_eq!(decoded, data);
            assert!(decompress(&encoded, &mut decoded, data.len() - 1).is_err());
        }
    }
    #[test]
    fn malformed_backreferences_are_rejected() {
        for data in [vec![], vec![0, 0, 0], vec![0xff], vec![0x10, 1, 1]] {
            assert!(decompress(&data, &mut Vec::new(), 4096).is_err());
        }
    }
}
