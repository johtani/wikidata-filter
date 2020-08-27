type="no_match"
for i in `seq 5`
do
cargo run ~/IdeaProjects/rust-workspace/wiki-extractor/input_data/latest-all.json.bz2 ./tmp_dir/sample_10.json -p p31,p32 >& result_${type}_${i}.log
done