# wikidata-filter

Parse wikidata all json and filter with `lang` and `claims` properties.

## Source Data

Now, this supports only Wikidata all json gzip file.
Please download `latest-all.json.gz` from https://dumps.wikimedia.org/wikidatawiki/entities/ .

### sample output json

```json
{"aliases":["ベルギー王国"],"claims":{"P31":["Q3624078","Q43702","Q6256","Q20181813","Q185441"]},"descriptions":"西ヨーロッパに位置する国家","id":"Q31","labels":"ベルギー"}
```

## Build

`cargo build --release`

> NOTE: Wikidata is so big. `--release` option contributes processing time.

## Usage

`cargo run --release <PATH/TO/latest-all.json.gz> <PATH/TO/OUTPUT_PREFIX> <OPTIONS>`

or 

`./target/release/wikidata-filter <PATH/TO/latest-all.json.gz> <PATH/TO/OUTPUT_PREFIX> <OPTIONS>`

### Options

* `-l` or `--language` (Required) : [Wikimedia language code](https://www.wikidata.org/wiki/Help:Wikimedia_language_codes/lists/all). Only one supported at this time.
* `-p` or `--properties` (Optional) : pass a comma-separated list of claims properties to include in output JSON. E.g. p31,p279.
* `--limit` (Optional) : (for test purpose) set the number > 0, the command handle # of lines from json then stop. If set 0 (default), handle all lines.

## LICENSE

MIT. See [LICENSE](./LICENSE) file