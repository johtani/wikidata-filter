use crate::output::{OutputJson, OutputManager};
use bzip2::read::BzDecoder;
use clap::ArgMatches;
use core::result::Result::{Err, Ok};
use futures::executor::{block_on, ThreadPool};
use futures::task::SpawnExt;
use log::{debug, info, warn};
use serde_derive::{Deserialize, Serialize};
use serde_json::value::Value::Array;
use serde_json::{Map, Value};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::time::Instant;

macro_rules! measure {
    ( $x:expr) => {{
        let start = Instant::now();
        let result = $x;
        let end = start.elapsed();
        println!(
            "parser 計測開始から{}.{:03}秒経過しました。",
            end.as_secs(),
            end.subsec_nanos() / 1_000_000
        );
        result
    }};
}

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

#[derive(Debug, Serialize, Deserialize)]
struct Claim {
    mainsnak: Mainsnak,
}

#[derive(Debug, Serialize, Deserialize)]
struct Mainsnak {
    datatype: String,
    datavalue: Datavalue,
}

#[derive(Debug, Serialize, Deserialize)]
struct Datavalue {
    value: ValueItem,
}

#[derive(Debug, Serialize, Deserialize)]
struct ValueItem {
    id: Option<String>,
}

#[derive(Debug)]
pub struct Document {
    original_map: Map<String, Value>,
    new_map: Map<String, Value>,
}

impl Document {
    pub fn to_json_string(&self) -> String {
        return serde_json::to_string(&self.new_map).expect("to_json_string Error...");
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

    pub fn copy_claims(&mut self, config: &Config) {
        if let Some(obj) = self.original_map.get("craims") {
            let map = obj
                .as_object()
                .expect(format!("Error during converting \"craims\" to map").as_str());
            let mut copied_claims = Map::new();
            for property in &config.properties {
                if let Some(claim) = map.get(property) {
                    let mut clone_ids = vec![];
                    let prop_array = claim.as_array().expect(
                        format!("Error during converting \"{}\" to array", property).as_str(),
                    );
                    for item in prop_array {
                        let prop_obj: Claim = serde_json::from_value(item.clone())
                            .expect("Claim object parse error...");
                        if let Some(id) = prop_obj.mainsnak.datavalue.value.id {
                            clone_ids
                                .push(serde_json::to_value(id).expect("mainsnak id copy error..."));
                        }
                    }
                    if !clone_ids.is_empty() {
                        copied_claims.insert(property.to_string(), Array(clone_ids));
                    }
                }
            }
            if !copied_claims.is_empty() {
                self.new_map.insert(
                    String::from("claims"),
                    serde_json::to_value(copied_claims).expect("to_value error..."),
                );
            }
        }
    }

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
    debug!("start process_buffer...");
    measure!({
        for article in buffer {
            let mut doc = Document {
                original_map: serde_json::from_str(article.as_str())
                    .expect("something wrong during parsing json"),
                new_map: Map::new(),
            };
            process_doc(&mut doc, &config);
            output.output(doc.to_json_string());
        }
    });
    output.flush();
    debug!("finish process_buffer...");
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
        if count % 10000 == 0 {
            debug!("{} docs operated...", count);
        }
        if config.with_limiter {
            if count > 100001 {
                break;
            }
        }
    }
    debug!("Out the lines loop...");
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
    debug!("before block_on...");
    block_on(futures::future::join_all(futures));
    debug!("finish block_on...");
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
