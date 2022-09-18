mod payload;

use std::fs::File;
use std::path::PathBuf;

use clap::{ArgEnum, Parser};

use payload::Payload;

/// Wrestle .hex files into .vhx or vice versa
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
pub struct Cli {
    /// Path of the file to convert
    #[clap(value_parser = legal_file_type)]
    input: PathBuf,

    /// Output path of converted file; leave empty to only inspect memory
    #[clap(value_parser = legal_file_type)]
    output: Option<PathBuf>,

    /// Chunk size to use for .vhx files (64 or 128)
    #[clap(short, long, value_parser = legal_chunk_size, default_value_t = 128)]
    chunk_size: usize,

    /// Endianness to use for .hex files
    #[clap(short, long, value_parser, default_value = "little")]
    endianness: Endianness,

    /// Start address for .vhx files, only relevant when converting .vhx -> .hex
    #[clap(short, long, value_parser = legal_u32, default_value_t = 0)]
    start_address: u32,

    /// Byte value to fill holes in the memory layout with
    #[clap(short, long, value_parser = legal_u8, default_value_t = 0xff)]
    fill: u8,
}

fn legal_u32(arg: &str) -> Result<u32, String> {
    Ok(legal_hex_or_decimal(arg)? as u32)
}

fn legal_u8(arg: &str) -> Result<u8, String> {
    Ok(legal_hex_or_decimal(arg)? as u8)
}

fn legal_hex_or_decimal(arg: &str) -> Result<usize, String> {
    if arg.starts_with("0x") {
        let bytes = hex::decode(&arg[2..])
            .map_err(|_| format!("`{}` is not a valid hexadecimal number", arg))?;

        let mut result = 0usize;
        for byte in bytes {
            result <<= 8;
            result |= byte as usize;
        }

        Ok(result)
    } else {
        arg.parse::<usize>()
            .map_err(|_| format!("`{}` is not a valid number", arg))
    }
}

fn legal_file_type(arg: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(arg);

    let extension = path
        .extension()
        .ok_or(String::from("No file extension specified"))?;

    match extension.to_str().unwrap() {
        "hex" | "vhx" | "vhx128" => Ok(path),
        ex => Err(format!("Unsupported file type `{}`", ex)),
    }
}

fn legal_chunk_size(arg: &str) -> Result<usize, String> {
    let size: usize = arg
        .parse()
        .map_err(|_| format!("`{}` is not a valid chunk size", arg))?;

    match size {
        64 | 128 => Ok(size),
        _ => Err(String::from("Chunk size must be either 64 or 128")),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, ArgEnum, Debug)]
enum Endianness {
    Little,
    Big,
}

fn main() -> Result<(), String> {
    let args = Cli::parse();

    let payload = match args.input.extension().unwrap().to_str().unwrap() {
        "hex" => Payload::from_hex(
            &args.input,
            args.endianness == Endianness::Little,
            args.fill,
        ),
        "vhx" | "vhx128" => Payload::from_vhx(&args.input, args.start_address as usize, args.chunk_size),
        _ => panic!("Unsupported file type was accepted by argument parser"),
    }?;

    if let Some(output) = args.output {
        let mut output_file = File::create(&output)
            .map_err(|_| format!("Could not create file `{}`", output.display()))?;

        match output.extension().unwrap().to_str().unwrap() {
            "hex" => payload.write_hex(&mut output_file, args.endianness == Endianness::Little),
            "vhx" | "vhx128"  => payload.write_vhx(&mut output_file, args.chunk_size),
            _ => panic!("Unsupported file type was accepted by argument parser"),
        }
    } else {
        print!("{}", payload);
    }

    Ok(())
}
