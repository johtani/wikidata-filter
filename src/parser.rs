use crate::output::{OutputJson, OutputManager};
use clap::ArgMatches;
use core::result::Result::{Err, Ok};
use flate2::read::MultiGzDecoder;
use futures::executor::{block_on, ThreadPool};
use futures::task::SpawnExt;
use log::{debug, info, warn};
use regex::Regex;
use serde_json::value::Value::Array;
use serde_json::{Map, Value};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::str::FromStr;

#[derive(Debug, Clone)]
pub struct Config {
    input_file: String,
    output_prefix: String,
    chunk_size: usize,
    properties: Vec<String>,
    lang: String,
    with_limiter: bool,
    limit: u64,
    lang_regex: Regex,
}

impl Config {
    pub fn new(args: ArgMatches) -> Self {
        let input_file = args.value_of("INPUT_FILE").unwrap();
        let output_prefix = args.value_of("OUTPUT_PREFIX").unwrap();
        let lang = args.value_of("LANGUAGE").unwrap();
        let prop_str = args.value_of("PROPERTIES").unwrap();
        let properties = prop_str
            .split(",")
            .map(|x| x.to_uppercase().to_string())
            .collect();
        let limit_str = args.value_of("LIMITS").unwrap();
        let limit = u64::from_str(limit_str).expect("aa");
        return Config {
            input_file: input_file.to_string(),
            output_prefix: output_prefix.to_string(),
            chunk_size: 100000,
            properties,
            lang: lang.to_string(),
            with_limiter: limit > 0,
            limit,
            lang_regex: Regex::new(format!("\"{}\"", lang).as_str()).unwrap(),
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

    fn copy_clone_ids(&self, item: &Value, clone_ids: &mut Vec<Value>) {
        let map = item.as_object().expect("Claim object parse error...");
        if let Some(mainsnak) = map.get("mainsnak") {
            let mainsnak_map = mainsnak
                .as_object()
                .expect("Mainsnak object parse error...");
            if let Some(datavalue) = mainsnak_map.get("datavalue") {
                let datavalue_map = datavalue
                    .as_object()
                    .expect("Datavalue object parse error...");
                if let Some(value) = datavalue_map.get("value") {
                    let value_map = value.as_object().expect("Value object parse error...");
                    if let Some(id) = value_map.get("id") {
                        clone_ids.push(id.clone());
                    }
                }
            }
        }
    }

    pub fn copy_claims(&mut self, config: &Config) {
        if let Some(obj) = self.original_map.get("claims") {
            let map = obj
                .as_object()
                .expect(format!("Error during converting \"claims\" to map").as_str());
            let mut copied_claims = Map::new();
            for property in &config.properties {
                if let Some(claim) = map.get(property) {
                    let mut clone_ids = vec![];
                    let prop_array = claim.as_array().expect(
                        format!("Error during converting \"{}\" to array", property).as_str(),
                    );
                    for item in prop_array {
                        //measure_ns!({
                        &self.copy_clone_ids(item, &mut clone_ids);
                        //});
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
    for mut article in buffer {
        //TODO 最後の行の処理
        let last = article.pop().unwrap();
        if last != ',' {
            article.push(last);
        }
        let mut doc = Document {
            original_map: serde_json::from_str(article.as_str())
                .expect("something wrong during parsing json"),
            new_map: Map::new(),
        };
        process_doc(&mut doc, &config);
        output.output(doc.to_json_string());
    }
    output.flush();
    debug!("finish process_buffer...");
}

fn skip_parse(article: &str, config: &Config) -> bool {
    // need lang chars in article
    // TODO check properties?
    return !config.lang_regex.is_match(article);
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
    //let buf = BzDecoder::new(file);
    let buf = MultiGzDecoder::new(file);
    let mut count = 0;
    let mut buffer: Vec<String> = vec![];

    for line in BufReader::new(buf).lines() {
        match line {
            Ok(article) => {
                if !skip_parse(&article, &config) {
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
        if count % 1_000_000 == 0 {
            info!("{} docs operated...", count);
        }
        if config.with_limiter {
            if count > config.limit {
                info!("{} docs operated...", count);
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

#[cfg(test)]
mod tests {
    use crate::parser::*;
    use std::time::Instant;

    macro_rules! measure_ns {
        ( $x:expr) => {{
            let start = Instant::now();
            let result = $x;
            let end = start.elapsed();
            println!("  {}", end.subsec_nanos());
            result
        }};
    }

    fn dummy_json() -> String {
        return String::from(
            r#"{"type":"item","id":"Q278","labels":{"fr":{"language":"fr","value":"Talisker"},"en":{"language":"en","value":"Talisker"},"it":{"language":"it","value":"Talisker"},"nb":{"language":"nb","value":"Talisker"},"de":{"language":"de","value":"Talisker"},"es":{"language":"es","value":"Talisker"},"ru":{"language":"ru","value":"Talisker"},"br":{"language":"br","value":"Talisker"},"da":{"language":"da","value":"Talisker"},"gd":{"language":"gd","value":"Talisker"},"he":{"language":"he","value":"\u05d8\u05d0\u05dc\u05d9\u05e1\u05e7\u05e8"},"ja":{"language":"ja","value":"\u30bf\u30ea\u30b9\u30ab\u30fc\u84b8\u7559\u6240"},"nl":{"language":"nl","value":"Talisker"},"pl":{"language":"pl","value":"Talisker"},"sl":{"language":"sl","value":"Talisker"},"sv":{"language":"sv","value":"Talisker"},"hu":{"language":"hu","value":"Talisker"},"fi":{"language":"fi","value":"Talisker"},"de-ch":{"language":"de-ch","value":"Talisker"},"en-ca":{"language":"en-ca","value":"Talisker distillery"},"en-gb":{"language":"en-gb","value":"Talisker distillery"},"ca":{"language":"ca","value":"Talisker"},"pt-br":{"language":"pt-br","value":"Talisker"},"ta":{"language":"ta","value":"\u0b9f\u0bbe\u0bb2\u0bbf\u0bb8\u0bcd\u0b95\u0bb0\u0bcd \u0bb5\u0b9f\u0bbf\u0b9a\u0bbe\u0bb2\u0bc8"},"oc":{"language":"oc","value":"Talisker"},"cs":{"language":"cs","value":"Talisker"},"el":{"language":"el","value":"\u03a4\u03ac\u03bb\u03b9\u03c3\u03ba\u03b5\u03c1"},"uk":{"language":"uk","value":"Talisker"},"sco":{"language":"sco","value":"Talisker"},"nds":{"language":"nds","value":"Talisker"},"ne":{"language":"ne","value":"\u0924\u093e\u0932\u093f\u0938\u094d\u0915\u0930"},"ro":{"language":"ro","value":"Talisker"},"cv":{"language":"cv","value":"Talisker"},"no":{"language":"no","value":"Talisker"}},"descriptions":{"en":{"language":"en","value":"Island single malt Scotch whisky distillery"},"fr":{"language":"fr","value":"distillerie \u00e9cossaise de whisky"},"it":{"language":"it","value":"Distilleria produttrice di whisky scozzese"},"nb":{"language":"nb","value":"skotsk whisky-brenneri"},"de":{"language":"de","value":"Whiskybrennerei auf der Insel Skye in Schottland"},"es":{"language":"es","value":"destiler\u00eda de whisky en Escocia"},"ru":{"language":"ru","value":"\u043c\u0430\u0440\u043a\u0430 \u0448\u043e\u0442\u043b\u0430\u043d\u0434\u0441\u043a\u043e\u0433\u043e \u0432\u0438\u0441\u043a\u0438"},"hu":{"language":"hu","value":"sk\u00f3t whisky lep\u00e1rl\u00f3"},"fi":{"language":"fi","value":"viskitislaamo Carbostin kyl\u00e4ss\u00e4 Skotlannissa"},"ca":{"language":"ca","value":"destil\u00b7leria anglesa"},"nl":{"language":"nl","value":"whisky"},"pt-br":{"language":"pt-br","value":"destilaria de u\u00edsque na Esc\u00f3cia"},"uk":{"language":"uk","value":"\u043c\u0430\u0440\u043a\u0430 \u043e\u0434\u043d\u043e\u0433\u043e \u0437 \u0432\u0456\u0434\u043e\u043c\u0438\u0445 \u0448\u043e\u0442\u043b\u0430\u043d\u0434\u0441\u043a\u0438\u0445 \u043e\u0434\u043d\u043e\u0441\u043e\u043b\u043e\u0434\u043e\u0432\u0438\u0445 \u0432\u0456\u0441\u043a\u0456"},"ne":{"language":"ne","value":"\u0906\u0907\u0932\u094d\u092f\u093e\u0923\u094d\u0921 \u090f\u0915\u0932 \u092e\u093e\u0932\u094d\u091f \u0938\u094d\u0915\u091a \u0935\u094d\u0939\u093f\u0938\u094d\u0915\u0940"},"ro":{"language":"ro","value":"Distileria Talisker"}},"aliases":{"uk":[{"language":"uk","value":"\u0422\u0430\u043b\u0438\u0441\u043a\u0435\u0440"}],"ro":[{"language":"ro","value":"whisky Talisker"}]},"claims":{"P17":[{"mainsnak":{"snaktype":"value","property":"P17","datavalue":{"value":{"entity-type":"item","numeric-id":145,"id":"Q145"},"type":"wikibase-entityid"},"datatype":"wikibase-item"},"type":"statement","id":"q278$E37DC3AD-9383-4D9A-B306-32641AD053E6","rank":"normal"}],"P373":[{"mainsnak":{"snaktype":"value","property":"P373","datavalue":{"value":"Talisker distillery","type":"string"},"datatype":"string"},"type":"statement","id":"q278$ACA89AD6-2565-4E86-960D-B78DEC4345CE","rank":"normal"}],"P18":[{"mainsnak":{"snaktype":"value","property":"P18","datavalue":{"value":"Glenmorangie Distillery Stills.jpg","type":"string"},"datatype":"commonsMedia"},"type":"statement","id":"q278$D479F3A1-FC56-4F5E-9659-34E16037935E","rank":"normal"}],"P242":[{"mainsnak":{"snaktype":"value","property":"P242","datavalue":{"value":"Skye talisker.png","type":"string"},"datatype":"commonsMedia"},"type":"statement","id":"q278$0F63A592-1C27-4304-8B1F-CC33BC8C549C","rank":"normal"}],"P625":[{"mainsnak":{"snaktype":"value","property":"P625","datavalue":{"value":{"latitude":57.302777777778,"longitude":-6.3561111111111,"altitude":null,"precision":null,"globe":"http:\/\/www.wikidata.org\/entity\/Q2"},"type":"globecoordinate"},"datatype":"globe-coordinate"},"type":"statement","id":"q278$614FB001-69B6-48B5-835D-AFA5F12A22B5","rank":"normal"}],"P31":[{"mainsnak":{"snaktype":"value","property":"P31","datavalue":{"value":{"entity-type":"item","numeric-id":10373548,"id":"Q10373548"},"type":"wikibase-entityid"},"datatype":"wikibase-item"},"type":"statement","id":"q278$4634A61F-555A-4072-8B54-6F2C20C0DDF1","rank":"normal"}],"P856":[{"mainsnak":{"snaktype":"value","property":"P856","datavalue":{"value":"http:\/\/www.malts.com\/","type":"string"},"datatype":"url"},"type":"statement","id":"Q278$005e3703-414a-ff23-0be2-881b5c036cc3","rank":"normal"}],"P646":[{"mainsnak":{"snaktype":"value","property":"P646","datavalue":{"value":"\/m\/01xfc0","type":"string"},"datatype":"external-id"},"type":"statement","id":"Q278$76BB779F-10A6-4C54-9BAF-094EE489C933","rank":"normal","references":[{"hash":"2b00cb481cddcac7623114367489b5c194901c4a","snaks":{"P248":[{"snaktype":"value","property":"P248","datavalue":{"value":{"entity-type":"item","numeric-id":15241312,"id":"Q15241312"},"type":"wikibase-entityid"},"datatype":"wikibase-item"}],"P577":[{"snaktype":"value","property":"P577","datavalue":{"value":{"time":"+2013-10-28T00:00:00Z","timezone":0,"before":0,"after":0,"precision":11,"calendarmodel":"http:\/\/www.wikidata.org\/entity\/Q1985727"},"type":"time"},"datatype":"time"}]},"snaks-order":["P248","P577"]}]}],"P1566":[{"mainsnak":{"snaktype":"value","property":"P1566","datavalue":{"value":"2636306","type":"string"},"datatype":"external-id"},"type":"statement","id":"Q278$5A5349F8-3A20-4734-8830-BC8F43632386","rank":"normal","references":[{"hash":"88694a0f4d1486770c269f7db16a1982f74da69d","snaks":{"P248":[{"snaktype":"value","property":"P248","datavalue":{"value":{"entity-type":"item","numeric-id":830106,"id":"Q830106"},"type":"wikibase-entityid"},"datatype":"wikibase-item"}]},"snaks-order":["P248"]}]}],"P571":[{"mainsnak":{"snaktype":"value","property":"P571","datavalue":{"value":{"time":"+1830-00-00T00:00:00Z","timezone":0,"before":0,"after":0,"precision":9,"calendarmodel":"http:\/\/www.wikidata.org\/entity\/Q1985727"},"type":"time"},"datatype":"time"},"type":"statement","id":"Q278$5F33C545-36E4-484E-8516-E2C8E40771A3","rank":"normal","references":[{"hash":"9a24f7c0208b05d6be97077d855671d1dfdbc0dd","snaks":{"P143":[{"snaktype":"value","property":"P143","datavalue":{"value":{"entity-type":"item","numeric-id":48183,"id":"Q48183"},"type":"wikibase-entityid"},"datatype":"wikibase-item"}]},"snaks-order":["P143"]}]}],"P3616":[{"mainsnak":{"snaktype":"value","property":"P3616","datavalue":{"value":"26285","type":"string"},"datatype":"external-id"},"type":"statement","qualifiers":{"P1810":[{"snaktype":"value","property":"P1810","hash":"9a8d88f264166d8e78756a2eee4f11ec675c7caa","datavalue":{"value":"Talisker, Inverness Shire","type":"string"},"datatype":"string"}]},"qualifiers-order":["P1810"],"id":"Q278$7789C37D-E8B0-44D4-B4E5-972DD1A275E5","rank":"normal","references":[{"hash":"50f62b183dbb3d9bffbb679d5544b7a9219ffdf9","snaks":{"P813":[{"snaktype":"value","property":"P813","datavalue":{"value":{"time":"+2017-03-01T00:00:00Z","timezone":0,"before":0,"after":0,"precision":11,"calendarmodel":"http:\/\/www.wikidata.org\/entity\/Q1985727"},"type":"time"},"datatype":"time"}]},"snaks-order":["P813"]}]}],"P276":[{"mainsnak":{"snaktype":"value","property":"P276","datavalue":{"value":{"entity-type":"item","numeric-id":987762,"id":"Q987762"},"type":"wikibase-entityid"},"datatype":"wikibase-item"},"type":"statement","id":"Q278$CE141B66-2D62-4DF0-BD75-5A748A8A5259","rank":"normal"}],"P131":[{"mainsnak":{"snaktype":"value","property":"P131","datavalue":{"value":{"entity-type":"item","numeric-id":208279,"id":"Q208279"},"type":"wikibase-entityid"},"datatype":"wikibase-item"},"type":"statement","qualifiers":{"P3831":[{"snaktype":"value","property":"P3831","hash":"347014e0a85050a101a9e87eef544c780a3c5d39","datavalue":{"value":{"entity-type":"item","numeric-id":837766,"id":"Q837766"},"type":"wikibase-entityid"},"datatype":"wikibase-item"}]},"qualifiers-order":["P3831"],"id":"Q278$940D7BF2-8080-4677-840F-BA649BF2C0CC","rank":"normal"},{"mainsnak":{"snaktype":"value","property":"P131","datavalue":{"value":{"entity-type":"item","numeric-id":68815035,"id":"Q68815035"},"type":"wikibase-entityid"},"datatype":"wikibase-item"},"type":"statement","qualifiers":{"P3831":[{"snaktype":"value","property":"P3831","hash":"458df4dbe99ce4bdca7d4957769af6ce9f53fa81","datavalue":{"value":{"entity-type":"item","numeric-id":5124673,"id":"Q5124673"},"type":"wikibase-entityid"},"datatype":"wikibase-item"}]},"qualifiers-order":["P3831"],"id":"Q278$7F90A6CA-A4CE-496C-A3A8-58EF67DFD29F","rank":"normal","references":[{"hash":"e67c82cab9198dd5d9800b7f2b6a31b4b2a9096f","snaks":{"P3616":[{"snaktype":"value","property":"P3616","datavalue":{"value":"26285","type":"string"},"datatype":"external-id"}]},"snaks-order":["P3616"]}]}],"P6766":[{"mainsnak":{"snaktype":"value","property":"P6766","datavalue":{"value":"1126032433","type":"string"},"datatype":"external-id"},"type":"statement","id":"Q278$586E2D42-2ECB-4BF4-9483-C928AF879C2D","rank":"normal"}],"P7959":[{"mainsnak":{"snaktype":"value","property":"P7959","datavalue":{"value":{"entity-type":"item","numeric-id":1247390,"id":"Q1247390"},"type":"wikibase-entityid"},"datatype":"wikibase-item"},"type":"statement","id":"Q278$b4315e88-0832-474a-b4b2-ed538b3293fe","rank":"normal"}]},"sitelinks":{"frwiki":{"site":"frwiki","title":"Talisker","badges":[]},"enwiki":{"site":"enwiki","title":"Talisker distillery","badges":[]},"dewiki":{"site":"dewiki","title":"Talisker","badges":[]},"itwiki":{"site":"itwiki","title":"Talisker","badges":[]},"brwiki":{"site":"brwiki","title":"Talisker","badges":[]},"dawiki":{"site":"dawiki","title":"Talisker","badges":[]},"gdwiki":{"site":"gdwiki","title":"Talisker","badges":[]},"hewiki":{"site":"hewiki","title":"\u05d8\u05d0\u05dc\u05d9\u05e1\u05e7\u05e8","badges":[]},"jawiki":{"site":"jawiki","title":"\u30bf\u30ea\u30b9\u30ab\u30fc\u84b8\u7559\u6240","badges":[]},"nlwiki":{"site":"nlwiki","title":"Talisker (whisky)","badges":[]},"nowiki":{"site":"nowiki","title":"Talisker","badges":[]},"plwiki":{"site":"plwiki","title":"Talisker","badges":[]},"ruwiki":{"site":"ruwiki","title":"Talisker","badges":[]},"slwiki":{"site":"slwiki","title":"Talisker","badges":[]},"svwiki":{"site":"svwiki","title":"Talisker","badges":[]},"cswiki":{"site":"cswiki","title":"Talisker","badges":[]},"ndswiki":{"site":"ndswiki","title":"Talisker","badges":[]},"cvwiki":{"site":"cvwiki","title":"Talisker (\u0432\u0438\u0441\u043a\u0438)","badges":[]},"commonswiki":{"site":"commonswiki","title":"Category:Talisker distillery","badges":[]}},"lastrevid":1187928120}"#,
        );
    }

    fn dummy_config() -> Config {
        return Config {
            input_file: String::from(""),
            output_prefix: String::from(""),
            chunk_size: 100000,
            properties: vec![String::from("P31")],
            lang: String::from("ja"),
            with_limiter: true,
            limit: 0,
            lang_regex: Regex::new(format!("\"{}\"", "ja").as_str()).unwrap(),
        };
    }

    #[test]
    fn check_perf_skip_parse() {
        let json = dummy_json();
        let article = json.as_str();
        let config = &dummy_config();
        let lang = format!("\"{}\"", config.lang);
        measure_ns!({
            for _i in 0..100 {
                match article {
                    "[" => true,
                    "]" => true,
                    _ => !config.lang_regex.is_match(article),
                };
            }
        });
        measure_ns!({
            for _i in 0..100 {
                if article.len() > 1 {
                    !config.lang_regex.is_match(article)
                } else {
                    true
                };
            }
        });

        measure_ns!({
            for _i in 0..100 {
                match article {
                    "[" => true,
                    "]" => true,
                    _ => false,
                };
            }
        });

        measure_ns!({
            for _i in 0..100 {
                article.len();
            }
        });

        measure_ns!({
            for _i in 0..100 {
                let hoge = config.lang_regex.is_match(article);
            }
        });

        measure_ns!({
            for _i in 0..100 {
                article.contains(lang.as_str());
            }
        });
        measure_ns!({
            for _i in 0..100 {
                if let Some(_) = article.find(lang.as_str()) {
                    //
                }
            }
        });
    }

    #[test]
    fn check_perf_process_doc() {
        let article = dummy_json();
        let config = &dummy_config();
        let mut doc = Document {
            original_map: serde_json::from_str(article.as_str())
                .expect("something wrong during parsing json"),
            new_map: Map::new(),
        };

        measure_ns!({
            for _i in 0..100 {
                doc.copy_id();
            }
        });
        // add label
        measure_ns!({
            for _i in 0..100 {
                doc.copy_labels(config);
            }
        });
        // add description
        measure_ns!({
            for _i in 0..100 {
                doc.copy_desc(config);
            }
        });
        // add aliases
        measure_ns!({
            for _i in 0..100 {
                doc.copy_aliases(config);
            }
        });
        // add claims
        measure_ns!({
            for _i in 0..100 {
                doc.copy_claims(config);
            }
        });
        assert_eq!(doc.new_map.len(), 3);
    }

    #[test]
    fn check_claims() {
        let article = dummy_json();
        let config = dummy_config();
        let mut doc = Document {
            original_map: serde_json::from_str(article.as_str())
                .expect("something wrong during parsing json"),
            new_map: Map::new(),
        };
        doc.copy_id();
        doc.copy_claims(&config);
        assert_eq!(doc.new_map.len(), 2);
    }

    #[test]
    fn check_speed_check_last_char() {
        let mut article = dummy_json();
        article.push(',');
        let mut article2 = article.clone();
        let mut article3 = article2.clone();
        let mut articles: Vec<String> = vec![];
        let mut articles2: Vec<String> = vec![];
        let mut articles3: Vec<String> = vec![];
        for i in 0..1000 {
            let mut article = dummy_json();
            article.push(',');
            articles.push(article);
            article = dummy_json();
            article.push(',');
            articles2.push(article);
            article = dummy_json();
            article.push(',');
            articles3.push(article);
        }
        measure_ns!({
            for mut article in articles {
                if article.ends_with(",") {
                    article.pop().unwrap();
                }
            }
        });
        measure_ns!({
            for mut article2 in articles2 {
                match article2.chars().last() {
                    Some(',') => {
                        article2.pop().unwrap();
                    }
                    _ => {}
                }
            }
        });
        measure_ns!({
            for mut article3 in articles3 {
                let last = article3.pop().unwrap();
                if last != ',' {
                    article3.push(last);
                }
            }
        });

        assert_eq!(article, article2);
        assert_eq!(article2, article3);
    }
}
