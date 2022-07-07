use std::fmt::{Display, Formatter};
use std::fs;
use std::io::Write;
use std::path::Path;

const BITS_IN_WORD: usize = 32;
const BYTES_IN_WORD: usize = 4;
const PREFERRED_WORDS_ON_LINE: usize = 4;

pub struct Payload {
    start_address: usize,
    bytes: Vec<u8>,
}

impl Payload {
    pub fn from_hex(file: &Path, little_endian: bool, fill_value: u8) -> Result<Self, String> {
        let file_name = file.display();

        let file_content =
            fs::read_to_string(file).map_err(|_| format!("Could not read `{}`", file_name))?;

        let mut memory_map: Vec<(usize, u8)> = Vec::new();

        let mut extended_segment_address = 0;
        let mut extended_linear_address = 0;

        for (line_number, line) in file_content.lines().enumerate() {
            let bytes = hex::decode(&line[1..])
                .map_err(|err| format!("Hex decode error for file `{}`: `{}`", file_name, err))?;

            let length = bytes.len();

            let computed_checksum = hex_checksum(&bytes[0..length - 1]);

            let included_checksum = bytes.last().copied().unwrap();

            if included_checksum != computed_checksum {
                return Err(format!(
                    "Checksum mismatch for file `{}` at line {}; `{}` vs `{}`",
                    file_name, line_number, computed_checksum, included_checksum
                ));
            }

            let count = bytes[0] as usize;
            let base_address = (bytes[1] as usize) << 8 | bytes[2] as usize;
            let record_type = bytes[3];
            let bytes = &bytes[length - count - 1..length - 1];

            match record_type {
                0 => {
                    for (offset, byte) in bytes.iter().enumerate() {
                        memory_map.push((
                            (extended_linear_address << 16)
                                | (16 * extended_segment_address
                                    + base_address
                                    + (offset as usize)),
                            *byte,
                        ));
                    }
                }
                1 => break,
                2 => {
                    if count != 2 {
                        return Err(format!(
                            "Incorrect extended segment address length for file `{}` at line {}",
                            file_name, line_number
                        ));
                    }
                    extended_segment_address = u16::from_be_bytes([bytes[0], bytes[1]]) as usize;
                }
                4 => {
                    if count != 2 {
                        return Err(format!(
                            "Incorrect extended linear address length for file `{}` at line {}",
                            file_name, line_number
                        ));
                    }
                    extended_linear_address = u16::from_be_bytes([bytes[0], bytes[1]]) as usize;
                }
                _ => (),
            }
        }

        memory_map.sort();

        let start_address = match memory_map.first() {
            None => 0,
            Some((address, _byte)) => *address,
        };

        let size = match memory_map.last() {
            None => 0,
            Some((address, _byte)) => *address - start_address,
        };

        let raw_bytes = memory_map
            .iter()
            .fold(
                (start_address, Vec::with_capacity(size)),
                |(last_address, mut acc), &(address, byte)| {
                    let mut fill = match address - last_address {
                        0 | 1 => Vec::new(),
                        gap_size => vec![fill_value; gap_size - 1],
                    };

                    acc.append(&mut fill);
                    acc.push(byte);

                    (address, acc)
                },
            )
            .1;

        let bytes = if little_endian {
            assert!(raw_bytes.len() % BYTES_IN_WORD == 0);

            raw_bytes
                .chunks_exact(BYTES_IN_WORD)
                .map(|word| word.iter().rev().copied().collect::<Vec<u8>>())
                .flatten()
                .collect()
        } else {
            raw_bytes
        };

        Ok(Self {
            start_address,
            bytes,
        })
    }

    pub fn from_vhx(file: &Path, start_address: usize, chunk_size: usize) -> Result<Self, String> {
        let file_name = file.display();

        let file_content = hex::decode(
            fs::read_to_string(file)
                .map_err(|_| format!("Could not read `{}`", file_name))?
                .chars()
                .filter(|ch| ch.is_ascii_hexdigit())
                .collect::<String>(),
        )
        .map_err(|err| format!("Hex decode error for file `{}`: `{}`", file_name, err))?;

        let word_chunk_size = chunk_size / BITS_IN_WORD;

        if file_content.len() % (word_chunk_size * BYTES_IN_WORD) != 0 {
            return Err(format!(
                "File `{}` does not contain a complete vhx memory layout",
                file_name
            ));
        }

        let mut bytes = Vec::with_capacity(file_content.len());

        for line in file_content.chunks_exact(word_chunk_size * BYTES_IN_WORD) {
            for word in line.chunks_exact(BYTES_IN_WORD).rev() {
                bytes.extend_from_slice(word);
            }
        }

        Ok(Self {
            start_address,
            bytes,
        })
    }

    pub fn write_hex(&self, file: &mut fs::File, little_endian: bool) {
        let start_address = (self.start_address as u32).to_be_bytes();
        let extended_segment = [0x02, 0x00, 0x00, 0x04, start_address[0], start_address[1]];
        let extended_checksum = [hex_checksum(&extended_segment)];

        writeln!(
            file,
            ":{}{}",
            hex::encode_upper(&extended_segment),
            hex::encode_upper(&extended_checksum)
        )
        .expect("Unable to write to file");

        let step_size = PREFERRED_WORDS_ON_LINE * BYTES_IN_WORD;

        for (offset, word_group) in self.bytes.chunks(step_size).enumerate() {
            let sub_address = ((self.start_address + offset * step_size) as u32).to_be_bytes();
            let header = [word_group.len() as u8, sub_address[2], sub_address[3], 0x00];

            let mut line = Vec::with_capacity(header.len() + word_group.len() + 1);
            line.extend(&header);

            for byte_group in word_group.chunks_exact(BYTES_IN_WORD) {
                if little_endian {
                    line.extend(byte_group.iter().rev());
                } else {
                    line.extend(byte_group);
                }
            }

            line.push(hex_checksum(&line));

            writeln!(file, ":{}", hex::encode_upper(&line),).unwrap();
        }

        writeln!(file, ":00000001FF").unwrap();
    }

    pub fn write_vhx(&self, file: &mut fs::File, chunk_size: usize) {
        let words: Vec<String> = self
            .bytes
            .chunks_exact(BYTES_IN_WORD)
            .map(|word| hex::encode(word))
            .collect();

        for chunk in words.chunks_exact(chunk_size / BITS_IN_WORD) {
            for word in chunk.iter().rev() {
                write!(file, "{}", word).expect("Unable to write to file");
            }
            writeln!(file, "").unwrap();
        }
    }
}

fn hex_checksum(bytes: &[u8]) -> u8 {
    (bytes.iter().fold(0u8, |acc, &x| acc.wrapping_add(x)) ^ 0xff).wrapping_add(1u8)
}

impl Display for Payload {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        let words: Vec<String> = self
            .bytes
            .chunks_exact(BYTES_IN_WORD)
            .map(|word| hex::encode(word))
            .collect();

        for (i, word_group) in words.chunks(PREFERRED_WORDS_ON_LINE).enumerate() {
            let offset = i * PREFERRED_WORDS_ON_LINE * BYTES_IN_WORD;
            let address = ((self.start_address + offset) as u32).to_be_bytes();

            let values = word_group.to_owned().join(" ");

            writeln!(f, "{}: {}", hex::encode(&address), values)?;
        }

        Ok(())
    }
}
