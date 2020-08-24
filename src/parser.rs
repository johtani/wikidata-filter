use crate::output::{OutputJson, OutputManager};
use bzip2::read::BzDecoder;
use clap::ArgMatches;
use core::result::Result::{Err, Ok};
use futures::executor::{block_on, ThreadPool};
use futures::task::SpawnExt;
use log::{info, warn};
use serde_json::{Map, Value};
use std::fs::File;
use std::io::{BufRead, BufReader};

#[derive(Debug, Clone)]
pub struct Config {
    input_file: String,
    output_prefix: String,
    chunk_size: usize,
    properties: Vec<String>,
    lang: String,
    with_limiter: bool,
}

impl Config {
    pub fn new(args: ArgMatches) -> Self {
        let input_file = args.value_of("INPUT_FILE").unwrap();
        let output_prefix = args.value_of("OUTPUT_PREFIX").unwrap();
        let lang = args.value_of("LANGUAGE").unwrap();
        let prop_str = args.value_of("PROPERTIES").unwrap();
        let properties = prop_str.split(",").map(|x| x.to_string()).collect();
        return Config {
            input_file: input_file.to_string(),
            output_prefix: output_prefix.to_string(),
            chunk_size: 100000,
            properties,
            lang: lang.to_string(),
            with_limiter: false,
        };
    }
}

#[derive(Debug)]
pub struct Document {
    original_map: Map<String, Value>,
    new_map: Map<String, Value>,
}

impl Document {
    pub fn to_json_string(&self) -> String {
        return serde_json::to_string(&self.new_map).unwrap();
    }

    pub fn copy_id(&mut self) {
        let value = self.original_map.get("id").unwrap().clone();
        &self.new_map.insert(String::from("id"), value);
    }

    pub fn copy_labels(&mut self, config: &Config) {
        self.copy_lang_value(config, "labels");
    }
    pub fn copy_desc(&mut self, config: &Config) {
        self.copy_lang_value(config, "descriptions");
    }
    pub fn copy_aliases(&mut self, config: &Config) {
        self.copy_lang_values(config, "aliases");
    }

    pub fn copy_claims(&mut self, config: &Config) {}

    fn copy_lang_value(&mut self, config: &Config, key: &str) {
        if let Some(obj) = self.original_map.get(key) {
            let map = obj
                .as_object()
                .expect(format!("Error during converting \"{}\" to map", key).as_str());
            if let Some(lang) = map.get(&config.lang) {
                let lang_map = lang
                    .as_object()
                    .expect("Error during converting \"lang\" to map");
                if let Some(lang_value) = lang_map.get("value") {
                    &self.new_map.insert(String::from(key), lang_value.clone());
                }
            }
        }
    }

    fn copy_lang_values(&mut self, config: &Config, key: &str) {
        if let Some(obj) = self.original_map.get(key) {
            let map = obj
                .as_object()
                .expect(format!("Error during converting \"{}\" to map", key).as_str());
            if let Some(lang) = map.get(&config.lang) {
                let lang_array = lang
                    .as_array()
                    .expect("Error during converting \"lang\" to array");

                let values: Vec<Value> = lang_array
                    .iter()
                    .filter_map(|item| {
                        let lang_map = item
                            .as_object()
                            .expect("Error during converting \"lang\" to map");
                        if let Some(lang_value) = lang_map.get("value") {
                            Some(lang_value.clone())
                        } else {
                            None
                        }
                    })
                    .collect();
                if values.len() > 0 {
                    &self.new_map.insert(String::from(key), Value::from(values));
                }
            }
        }
    }
}

async fn process_buffer(buffer: Vec<String>, config: Config, mut output: OutputJson) {
    for article in buffer {
        let mut doc = Document {
            original_map: serde_json::from_str(article.as_str())
                .expect("something wrong during parsing json"),
            new_map: Map::new(),
        };
        process_doc(&mut doc, &config);
        output.output(doc.to_json_string());
    }
    output.flush();
}

pub fn parse_and_output(config: &Config) {
    let pool = ThreadPool::builder()
        .create()
        .expect("Create thread pool error");

    let mut futures = vec![];
    let input_file = &config.input_file;
    let mut output_manager = OutputManager::new(&config.output_prefix);

    info!("open file...");
    let file = File::open(input_file).expect("Input file open error");
    let buf = BzDecoder::new(file);
    let mut count = 0;
    let mut buffer: Vec<String> = vec![];

    for line in BufReader::new(buf).lines() {
        match line {
            Ok(mut article) => {
                if article != "[" && article != "]" {
                    article.pop().unwrap();
                    buffer.push(article);
                    if buffer.len() == config.chunk_size {
                        futures.push(
                            pool.spawn_with_handle(process_buffer(
                                buffer,
                                config.clone(),
                                output_manager.create_output_json(),
                            ))
                            .expect("Spawn error..."),
                        );
                        buffer = vec![];
                    }
                }
            }
            Err(_) => {
                warn!("Read line error. line[{}]", count);
            }
        }
        count += 1;
        if count > 10001 {
            break;
        }
    }
    //TODO handle last docs in buffer
    if !buffer.is_empty() {
        futures.push(
            pool.spawn_with_handle(process_buffer(
                buffer,
                config.clone(),
                output_manager.create_output_json(),
            ))
            .expect("Spawn error..."),
        );
    }
    block_on(futures::future::join_all(futures));
}

fn process_doc(doc: &mut Document, config: &Config) {
    doc.copy_id();
    // add label
    doc.copy_labels(config);
    // add description
    doc.copy_desc(config);
    // add aliases
    doc.copy_aliases(config);
    // add claims
    doc.copy_claims(config);
}
