use bzip2::read::BzDecoder;
use clap::ArgMatches;
use core::result::Result::{Err, Ok};
use log::{info, warn};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};

#[derive(Debug)]
pub struct Config {
    input_file: String,
    output_prefix: String,
    chunk_size: i32,
    properties: Option<Vec<String>>,
    lang: String,
}

impl Config {
    pub fn new(args: ArgMatches) -> Self {
        let input_file = args.value_of("INPUT_FILE").unwrap();
        let output_prefix = args.value_of("OUTPUT_PREFIX").unwrap();
        let lang = args.value_of("LANGUAGE").unwrap();
        let properties = args.values_of_lossy("PROPERTIES");
        return Config {
            input_file: input_file.to_string(),
            output_prefix: output_prefix.to_string(),
            chunk_size: 100000,
            properties,
            lang: lang.to_string(),
        };
    }
}

pub fn parse_and_output(config: &Config) {
    let input_file = &config.input_file;
    let output_file = &config.output_prefix;

    info!("open file...");
    let file = File::open(input_file).expect("Input file open error");
    let buf = BzDecoder::new(file);
    let mut output_f = OpenOptions::new()
        .write(true)
        .create(true)
        .open(output_file)
        .expect("Output file open error");

    let mut count = 0;
    for line in BufReader::new(buf).lines() {
        match line {
            Ok(article) => {
                writeln!(output_f, "{}", article);
            }
            Err(_) => {
                warn!("Read line error.");
            }
        }
        count += 1;
        info!("read {} lines...", count);
        if count > 10 {
            break;
        }
    }
    output_f.flush().expect("Flush error...");
}
