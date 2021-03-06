#[macro_use]
extern crate clap;
use clap::{App, AppSettings, Arg};
use log::info;
use metered::{measure, ResponseTime};
use std::env;
use wikidata_filter::parser::{parse_and_output, Config};

fn main() {
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "info");
    }
    env_logger::init();
    info!("Start!...");
    let app = App::new(crate_name!())
        .setting(AppSettings::DeriveDisplayOrder)
        .version(crate_version!())
        .author(crate_authors!())
        .about(crate_description!())
        .help_message("Prints help information.")
        .version_message("Prints version information.")
        .version_short("v")
        .arg(
            Arg::with_name("INPUT_FILE")
                .help("The file path of Wikidata dump JSON gzip, e.g. `latest-all.json.gz`. ")
                .value_name("INPUT_FILE")
                .required(true)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("OUTPUT_PREFIX")
                .help("The file prefix of output json files, e.g. `path/to/output_`. The command creates `path/to/output_01.json`")
                .value_name("OUTPUT_PREFIX")
                .required(true)
                .takes_value(true),
        ).arg(
        Arg::with_name("PROPERTIES")
            .help("pass a comma-separated list of properties. E.g. p31,p21.")
            .short("p")
            .long("properties")
            .required(false)
            .takes_value(true),
        ).arg(
            Arg::with_name("LANGUAGE")
            .help("Wikimedia language code. Only one supported at this time.")
            .short("l")
            .long("language")
            .default_value("ja")
            .required(false)
            .takes_value(true)
        ).arg(
        Arg::with_name("LIMITS")
            .help("The limit number of reading lines from json file. If --limit is 100, the command only read first 100 lines. If set 0, the command proceed all lines.")
            .long("limit")
            .default_value("0")
            .required(false)
            .min_values(0)
            .takes_value(true)
        );

    let config = Config::new(app.get_matches());
    let ref response_time: ResponseTime = ResponseTime::default();
    info!("{:?}", config);
    measure!(response_time, {
        parse_and_output(&config);
    });
    info!("Finish!...");
    info!("{}", serde_json::to_string(&response_time).unwrap());
}
