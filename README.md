# wikidata-filter

Parse wikidata all json and filter with `lang` and `claims` properties.

## Source Data

Now, this supports only Wikidata all json bzip2 file.
Please download `latest-all.json.bz2` from https://dumps.wikimedia.org/wikidatawiki/entities/ .

## Build

`cargo build --release`

> NOTE: Wikidata is so big. `--release` option contributes processing time.

## Usage

`cargo run --release <PATH/TO/latest-all.json.bz2> <PATH/TO/OUTPUT_PREFIX> <OPTIONS>`

or 

`./target/release/wikidata-filter <PATH/TO/latest-all.json.bz2> <PATH/TO/OUTPUT_PREFIX> <OPTIONS>`

### Options

* `-l` or `--language` (Required) : [Wikimedia language code](https://www.wikidata.org/wiki/Help:Wikimedia_language_codes/lists/all). Only one supported at this time.
* `-p` or `--properties` (Optional) : pass a comma-separated list of claims properties to include in output JSON. E.g. p31,p279.

## LICENSE

MIT. See [LICENSE](./LICENSE) file