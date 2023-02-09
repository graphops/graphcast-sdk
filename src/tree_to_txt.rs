use serde_json::Value;
use std::fs;
use std::io::{BufWriter, Write};

/// Convert TXT records from the tree_creator to standard ZONE file
fn main() {
    // Set up file
    let json_file = fs::read_to_string("txt_records.json").expect("Unable to read file");
    let json: Value = serde_json::from_str(&json_file).expect("Unable to parse JSON");
    let output_file = fs::File::create("test_tree_zone.txt").expect("Unable to create file");
    let mut output = BufWriter::new(output_file);

    // Initialize with standard configs
    writeln!(output, "$ORIGIN testfleet.graphcast.xyz.\n$TTL 86400\ngraphcast.xyz	3600	IN	SOA	graphcast.xyz root.graphcast.xyz 2042508586 7200 3600 86400 3600\ntestfleet.graphcast.xyz.	86400	IN	A	165.227.156.72\ngraphcast.xyz.	1	IN	CNAME	testfleet.graphcast.xyz.").expect("Failed to write default configs");
    // Iterate over the result tree
    for (key, value) in json
        .as_object()
        .expect("Could not convert JSON to object")
        .get("result")
        .expect("Unable to get result from JSON file")
        .as_object()
        .expect("Could not convert JSON to object")
        .iter()
    {
        let record = format!("{key}. IN TXT {value}");
        writeln!(output, "{record}").expect("Unable to write to file");
    }
    println!("Finished converting, check test_tree_zone.txt");
}
