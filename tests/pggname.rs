//! Integration tests for the `pggname` binary.
//!
//! The tests use the graphs in `test-data`:
//!
//! * `base`: the base graph.
//! * `chopped`: the base graph with nodes chopped to 3 bp.
//! * `subgraph`: a subgraph of `chopped`.
//!
//! Each graph is available in GFA and GBZ formats, both without name information (`noname`) and
//! with a precomputed name (`name`). The `subgraph.name.*` graphs also store the subgraph
//! relationship to `chopped`.
//!
//! Tests that modify graphs work on copies in a per-test scratch directory.

use gbz::{GBZ, GraphName};

use simple_sds::serialize;

use flate2::Compression;
use flate2::write::GzEncoder;

use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

//-----------------------------------------------------------------------------

const BASE: &str = "608eae09d127fc30f489dbd81fa8ecdc93c4a9d480658936e289a35a7703e4ef";
const CHOPPED: &str = "dc7ce3e648c4f92498a2e9f5d0064104e58862a392631970a95764261a5647ee";
const SUBGRAPH: &str = "6bd654934ac999eed298e4f274e590c30c36f7c2c259f0d5f4b5cd4f9462a79c";

// A syntactically valid name that is not the name of any test graph.
const FAKE: &str = "0000000000000000000000000000000000000000000000000000000000000000";

const GRAPHS: &[(&str, &str)] = &[("base", BASE), ("chopped", CHOPPED), ("subgraph", SUBGRAPH)];
const VARIANTS: &[&str] = &["noname", "name"];
const FORMATS: &[&str] = &["gfa", "gbz"];

// Translation between the unitigs of `base` and `chopped`.
const BASE_TO_CHOPPED: &[(&str, &str)] = &[
    (">11", ">1>2"),
    (">12", ">3"),
    (">13", ">4"),
    (">14", ">5>6"),
    (">21", ">7"),
    (">22", ">8"),
    (">23", ">9"),
    (">24", ">10>11"),
];

//-----------------------------------------------------------------------------

// Returns the path to a file in `test-data`.
fn data(graph: &str, variant: &str, format: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("test-data")
        .join(format!("{}.{}.{}", graph, variant, format))
}

// All test files as (path, expected name).
fn all_files() -> Vec<(PathBuf, &'static str)> {
    let mut result = Vec::new();
    for &(graph, name) in GRAPHS {
        for &variant in VARIANTS {
            for &format in FORMATS {
                result.push((data(graph, variant, format), name));
            }
        }
    }
    result
}

// Creates an empty scratch directory for the test.
fn scratch_dir(test: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("pggname-tests").join(test);
    if dir.exists() {
        fs::remove_dir_all(&dir).unwrap();
    }
    fs::create_dir_all(&dir).unwrap();
    dir
}

// Copies a file to the directory and returns the path to the copy.
fn copy_to(dir: &Path, source: &Path) -> PathBuf {
    let target = dir.join(source.file_name().unwrap());
    fs::copy(source, &target).unwrap();
    target
}

fn path_str(path: &Path) -> &str {
    path.to_str().unwrap()
}

//-----------------------------------------------------------------------------

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_pggname"))
        .args(args)
        .output()
        .expect("Failed to run pggname")
}

// Runs the binary, checks that it succeeded, and returns the standard output.
fn run_ok(args: &[&str]) -> String {
    let output = run(args);
    assert!(
        output.status.success(),
        "pggname {:?} failed: {}", args, String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

// Runs the binary, checks that it failed, and returns the standard error.
fn run_err(args: &[&str]) -> String {
    let output = run(args);
    assert!(!output.status.success(), "pggname {:?} succeeded unexpectedly", args);
    String::from_utf8(output.stderr).unwrap()
}

// Expected output line for a graph name.
fn name_line(name: &str, path: &Path) -> String {
    format!("{}  {}\n", name, path.display())
}

//-----------------------------------------------------------------------------

fn load_gbz(path: &Path) -> GBZ {
    serialize::load_from(path).unwrap()
}

// Returns the name information stored in the GBZ tags.
fn gbz_tags(path: &Path) -> GraphName {
    GraphName::from_tags(load_gbz(path).tags()).unwrap()
}

// Returns the name information stored in the GFA headers.
fn gfa_headers(path: &Path) -> GraphName {
    let contents = fs::read_to_string(path).unwrap();
    let headers: Vec<String> = contents.lines()
        .filter(|line| line.starts_with("H\t"))
        .map(String::from)
        .collect();
    GraphName::from_header_lines(&headers).unwrap()
}

fn subgraphs(name: &GraphName) -> BTreeSet<(String, String)> {
    name.subgraph_iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
}

fn translations(name: &GraphName) -> BTreeSet<(String, String)> {
    name.translation_iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
}

fn pairs(pairs: &[(&str, &str)]) -> BTreeSet<(String, String)> {
    pairs.iter().map(|&(a, b)| (a.to_string(), b.to_string())).collect()
}

//-----------------------------------------------------------------------------

// Writes a copy of the GFA file with a name header after the first line.
fn gfa_with_name(source: &Path, target: &Path, name: &str) {
    let contents = fs::read_to_string(source).unwrap();
    let (first, rest) = contents.split_once('\n').unwrap();
    let modified = format!("{}\nH\tNM:Z:{}\n{}", first, name, rest);
    fs::write(target, modified).unwrap();
}

// Writes a copy of the GBZ file that stores the given name and no relationships.
fn gbz_with_name(source: &Path, target: &Path, name: &str) {
    let mut graph = load_gbz(source);
    GraphName::new(name.to_string()).set_tags(graph.tags_mut());
    serialize::serialize_to(&graph, target).unwrap();
}

// Writes a gzip-compressed copy of the file, using a separate gzip member for each part.
fn gzip_members(source: &Path, target: &Path, parts: usize) {
    let contents = fs::read(source).unwrap();
    let part_len = contents.len().div_ceil(parts);
    let mut output = Vec::new();
    for chunk in contents.chunks(part_len) {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(chunk).unwrap();
        output.extend(encoder.finish().unwrap());
    }
    fs::write(target, output).unwrap();
}

//-----------------------------------------------------------------------------

// Sanity checks for the test data, as the other tests rely on these properties.

#[test]
fn test_data_name_information() {
    for &(graph, name) in GRAPHS {
        let expected = if graph == "subgraph" {
            let mut result = GraphName::new(SUBGRAPH.to_string());
            result.make_subgraph_of(&GraphName::new(CHOPPED.to_string()));
            result
        } else {
            GraphName::new(name.to_string())
        };

        let gbz = gbz_tags(&data(graph, "name", "gbz"));
        assert_eq!(gbz, expected, "Wrong name information in {}.name.gbz", graph);
        let gfa = gfa_headers(&data(graph, "name", "gfa"));
        assert_eq!(gfa, expected, "Wrong name information in {}.name.gfa", graph);

        let gbz = gbz_tags(&data(graph, "noname", "gbz"));
        assert_eq!(gbz, GraphName::default(), "{}.noname.gbz has name information", graph);
        let gfa = gfa_headers(&data(graph, "noname", "gfa"));
        assert_eq!(gfa, GraphName::default(), "{}.noname.gfa has name information", graph);
    }
}

//-----------------------------------------------------------------------------

// Hash mode.

#[test]
fn names_of_single_graphs() {
    for (path, name) in all_files() {
        let expected = name_line(name, &path);
        let output = run_ok(&[path_str(&path)]);
        assert_eq!(output, expected, "Wrong output for {}", path.display());
        let output = run_ok(&["--recompute", path_str(&path)]);
        assert_eq!(output, expected, "Wrong output for {} with --recompute", path.display());
    }
}

#[test]
fn names_of_multiple_graphs() {
    let files = all_files();
    let args: Vec<&str> = files.iter().map(|(path, _)| path_str(path)).collect();
    let expected: String = files.iter().map(|(path, name)| name_line(name, path)).collect();
    assert_eq!(run_ok(&args), expected, "Wrong output for all graphs");

    let mut recompute = vec!["-r"];
    recompute.extend(args.iter());
    assert_eq!(run_ok(&recompute), expected, "Wrong output for all graphs with -r");
}

#[test]
fn gzip_compressed_gfa() {
    let dir = scratch_dir("gzip_compressed_gfa");
    for &(graph, name) in GRAPHS {
        for &variant in VARIANTS {
            let source = data(graph, variant, "gfa");
            for parts in [1, 3] {
                let target = dir.join(format!("{}.{}.{}.gfa.gz", graph, variant, parts));
                gzip_members(&source, &target, parts);
                let output = run_ok(&[path_str(&target)]);
                assert_eq!(output, name_line(name, &target), "Wrong output for {}", target.display());
            }
        }
    }
}

#[test]
fn stored_name_is_used() {
    let dir = scratch_dir("stored_name_is_used");
    let gfa = dir.join("fake.gfa");
    gfa_with_name(&data("base", "noname", "gfa"), &gfa, FAKE);
    let gbz = dir.join("fake.gbz");
    gbz_with_name(&data("base", "noname", "gbz"), &gbz, FAKE);

    for path in [&gfa, &gbz] {
        let output = run_ok(&[path_str(path)]);
        assert_eq!(output, name_line(FAKE, path), "Stored name not used for {}", path.display());
        let output = run_ok(&["--recompute", path_str(path)]);
        assert_eq!(output, name_line(BASE, path), "Name not recomputed for {}", path.display());
    }
}

//-----------------------------------------------------------------------------

// Compare mode.

// Expected verdict for comparing the first graph to the second.
fn expected_verdict(first: &str, second: &str) -> &'static str {
    match (first, second) {
        _ if first == second => "same",
        ("base", "chopped") | ("chopped", "base") => "isomorphic",
        ("subgraph", "chopped") => "subgraph",
        ("chopped", "subgraph") => "supergraph",
        _ => "unrelated",
    }
}

#[test]
fn compare_all_pairs() {
    for &(first_graph, first_name) in GRAPHS {
        for &(second_graph, second_name) in GRAPHS {
            let verdict = expected_verdict(first_graph, second_graph);
            for &first_variant in VARIANTS {
                for &second_variant in VARIANTS {
                    for &first_format in FORMATS {
                        for &second_format in FORMATS {
                            let first = data(first_graph, first_variant, first_format);
                            let second = data(second_graph, second_variant, second_format);
                            let expected = format!(
                                "{}{}{}\n",
                                name_line(first_name, &first), name_line(second_name, &second), verdict
                            );
                            let output = run_ok(&["--compare", path_str(&first), path_str(&second)]);
                            assert_eq!(
                                output, expected,
                                "Wrong output when comparing {} to {}", first.display(), second.display()
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn compare_uses_stored_names() {
    // A subgraph that claims to be the base graph.
    let dir = scratch_dir("compare_uses_stored_names");
    let fake = dir.join("fake.gfa");
    gfa_with_name(&data("subgraph", "noname", "gfa"), &fake, BASE);
    let base = data("base", "noname", "gbz");

    let output = run_ok(&["-c", path_str(&fake), path_str(&base)]);
    let expected = format!("{}{}same\n", name_line(BASE, &fake), name_line(BASE, &base));
    assert_eq!(output, expected, "Stored names not used in comparison");

    let output = run_ok(&["-c", "-r", path_str(&fake), path_str(&base)]);
    let expected = format!("{}{}unrelated\n", name_line(SUBGRAPH, &fake), name_line(BASE, &base));
    assert_eq!(output, expected, "Stored names not discarded with --recompute");
}

// Returns the translation file as a set of pairs.
fn read_translation(path: &Path) -> BTreeSet<(String, String)> {
    let contents = fs::read_to_string(path).unwrap();
    contents.lines().map(|line| {
        let fields: Vec<&str> = line.split('\t').collect();
        assert_eq!(fields.len(), 2, "Invalid translation line: {}", line);
        (fields[0].to_string(), fields[1].to_string())
    }).collect()
}

#[test]
fn translation_for_isomorphic_graphs() {
    let dir = scratch_dir("translation_for_isomorphic_graphs");
    let forward = pairs(BASE_TO_CHOPPED);
    let reverse: BTreeSet<(String, String)> = forward.iter().map(|(a, b)| (b.clone(), a.clone())).collect();

    for &variant in VARIANTS {
        for &base_format in FORMATS {
            for &chopped_format in FORMATS {
                let base = data("base", variant, base_format);
                let chopped = data("chopped", variant, chopped_format);
                let cases = [(&base, &chopped, &forward), (&chopped, &base, &reverse)];
                for (i, (first, second, expected)) in cases.into_iter().enumerate() {
                    let translation = dir.join(format!("{}.{}.{}.{}.tsv", variant, base_format, chopped_format, i));
                    let output = run_ok(&[
                        "-c", "-t", path_str(&translation), path_str(first), path_str(second)
                    ]);
                    assert!(output.ends_with("isomorphic\n"), "Wrong verdict for {} and {}", first.display(), second.display());
                    assert_eq!(
                        &read_translation(&translation), expected,
                        "Wrong translation from {} to {}", first.display(), second.display()
                    );
                }
            }
        }
    }
}

#[test]
fn no_translation_for_other_relationships() {
    let dir = scratch_dir("no_translation_for_other_relationships");
    let cases = [("base", "base"), ("subgraph", "chopped"), ("chopped", "subgraph"), ("base", "subgraph")];
    for (first_graph, second_graph) in cases {
        let first = data(first_graph, "noname", "gbz");
        let second = data(second_graph, "noname", "gfa");
        let translation = dir.join(format!("{}.{}.tsv", first_graph, second_graph));
        run_ok(&["-c", "-t", path_str(&translation), path_str(&first), path_str(&second)]);
        assert!(!translation.exists(), "Translation written for {} and {}", first_graph, second_graph);
    }
}

//-----------------------------------------------------------------------------

// Storing names in hash mode.

#[test]
fn store_name_in_gbz() {
    let dir = scratch_dir("store_name_in_gbz");
    for &(graph, name) in GRAPHS {
        let path = copy_to(&dir, &data(graph, "noname", "gbz"));
        let output = run_ok(&["--store-name", path_str(&path)]);
        assert_eq!(output, name_line(name, &path), "Wrong output for {}", path.display());
        assert_eq!(gbz_tags(&path), GraphName::new(name.to_string()), "Wrong tags in {}", path.display());
        // The graph must still be the same.
        let output = run_ok(&["-c", "-r", path_str(&path), path_str(&data(graph, "noname", "gbz"))]);
        assert!(output.ends_with("same\n"), "Graph {} changed", path.display());
    }
}

#[test]
fn store_name_preserves_existing_tags() {
    let dir = scratch_dir("store_name_preserves_existing_tags");
    for &(graph, _) in GRAPHS {
        let source = data(graph, "name", "gbz");
        let path = copy_to(&dir, &source);
        run_ok(&["-s", path_str(&path)]);
        assert_eq!(fs::read(&path).unwrap(), fs::read(&source).unwrap(), "File {} was rewritten", path.display());
    }
}

#[test]
fn store_name_with_recompute_discards_relationships() {
    let dir = scratch_dir("store_name_with_recompute_discards_relationships");
    let path = copy_to(&dir, &data("subgraph", "name", "gbz"));
    let output = run_ok(&["-s", "-r", path_str(&path)]);
    assert_eq!(output, name_line(SUBGRAPH, &path), "Wrong output for {}", path.display());
    assert_eq!(gbz_tags(&path), GraphName::new(SUBGRAPH.to_string()), "Relationships not discarded");
}

#[test]
fn store_name_with_recompute_fixes_wrong_name() {
    let dir = scratch_dir("store_name_with_recompute_fixes_wrong_name");
    let path = dir.join("fake.gbz");
    gbz_with_name(&data("base", "noname", "gbz"), &path, FAKE);
    run_ok(&["-s", path_str(&path)]);
    assert_eq!(gbz_tags(&path), GraphName::new(FAKE.to_string()), "Stored name changed without -r");
    run_ok(&["-s", "-r", path_str(&path)]);
    assert_eq!(gbz_tags(&path), GraphName::new(BASE.to_string()), "Stored name not fixed with -r");
}

#[test]
fn store_name_does_not_modify_gfa() {
    let dir = scratch_dir("store_name_does_not_modify_gfa");
    for &(graph, name) in GRAPHS {
        let source = data(graph, "noname", "gfa");
        let path = copy_to(&dir, &source);
        let output = run_ok(&["-s", "-r", path_str(&path)]);
        assert_eq!(output, name_line(name, &path), "Wrong output for {}", path.display());
        assert_eq!(fs::read(&path).unwrap(), fs::read(&source).unwrap(), "File {} was modified", path.display());
    }
}

//-----------------------------------------------------------------------------

// Storing relationships in compare mode.

#[test]
fn compare_stores_subgraph_relationship() {
    let dir = scratch_dir("compare_stores_subgraph_relationship");
    // Both argument orders should store the same information.
    for reversed in [false, true] {
        let subgraph = copy_to(&dir, &data("subgraph", "noname", "gbz"));
        let chopped = copy_to(&dir, &data("chopped", "noname", "gbz"));
        let (first, second, verdict) = if reversed {
            (&chopped, &subgraph, "supergraph")
        } else {
            (&subgraph, &chopped, "subgraph")
        };
        let output = run_ok(&["-c", "-s", path_str(first), path_str(second)]);
        assert!(output.ends_with(&format!("{}\n", verdict)), "Wrong verdict (reversed: {})", reversed);

        // The result should match the precomputed graphs.
        assert_eq!(
            gbz_tags(&subgraph), gbz_tags(&data("subgraph", "name", "gbz")),
            "Wrong tags in the subgraph (reversed: {})", reversed
        );
        assert_eq!(
            gbz_tags(&chopped), gbz_tags(&data("chopped", "name", "gbz")),
            "Wrong tags in the supergraph (reversed: {})", reversed
        );
    }
}

#[test]
fn compare_stores_translation_relationship() {
    let dir = scratch_dir("compare_stores_translation_relationship");
    let base = copy_to(&dir, &data("base", "noname", "gbz"));
    let chopped = copy_to(&dir, &data("chopped", "noname", "gbz"));
    let output = run_ok(&["-c", "-s", path_str(&base), path_str(&chopped)]);
    assert!(output.ends_with("isomorphic\n"), "Wrong verdict");

    let expected = pairs(&[(BASE, CHOPPED), (CHOPPED, BASE)]);
    for (path, name) in [(&base, BASE), (&chopped, CHOPPED)] {
        let tags = gbz_tags(path);
        assert_eq!(tags.name().map(String::as_str), Some(name), "Wrong name in {}", path.display());
        assert!(subgraphs(&tags).is_empty(), "Subgraph relationships in {}", path.display());
        assert_eq!(translations(&tags), expected, "Wrong translations in {}", path.display());
    }
}

#[test]
fn compare_stores_only_names_for_unrelated_graphs() {
    let dir = scratch_dir("compare_stores_only_names_for_unrelated_graphs");
    let base = copy_to(&dir, &data("base", "noname", "gbz"));
    let subgraph = copy_to(&dir, &data("subgraph", "noname", "gbz"));
    let output = run_ok(&["-c", "-s", path_str(&base), path_str(&subgraph)]);
    assert!(output.ends_with("unrelated\n"), "Wrong verdict");
    assert_eq!(gbz_tags(&base), GraphName::new(BASE.to_string()), "Wrong tags in {}", base.display());
    assert_eq!(gbz_tags(&subgraph), GraphName::new(SUBGRAPH.to_string()), "Wrong tags in {}", subgraph.display());
}

#[test]
fn compare_stores_relationships_with_gfa() {
    let dir = scratch_dir("compare_stores_relationships_with_gfa");
    let subgraph = copy_to(&dir, &data("subgraph", "noname", "gbz"));
    let chopped_source = data("chopped", "noname", "gfa");
    let chopped = copy_to(&dir, &chopped_source);
    let output = run_ok(&["-c", "-s", path_str(&subgraph), path_str(&chopped)]);
    assert!(output.ends_with("subgraph\n"), "Wrong verdict");
    assert_eq!(gbz_tags(&subgraph), gbz_tags(&data("subgraph", "name", "gbz")), "Wrong tags in the subgraph");
    assert_eq!(fs::read(&chopped).unwrap(), fs::read(&chopped_source).unwrap(), "GFA file was modified");
}

#[test]
fn compare_preserves_existing_tags() {
    let dir = scratch_dir("compare_preserves_existing_tags");
    let subgraph_source = data("subgraph", "name", "gbz");
    let chopped_source = data("chopped", "name", "gbz");
    let subgraph = copy_to(&dir, &subgraph_source);
    let chopped = copy_to(&dir, &chopped_source);
    run_ok(&["-c", "-s", path_str(&subgraph), path_str(&chopped)]);
    assert_eq!(fs::read(&subgraph).unwrap(), fs::read(&subgraph_source).unwrap(), "Subgraph was rewritten");
    assert_eq!(fs::read(&chopped).unwrap(), fs::read(&chopped_source).unwrap(), "Supergraph was rewritten");
}

//-----------------------------------------------------------------------------

// Errors.

#[test]
fn usage_without_arguments() {
    let stderr = run_err(&[]);
    assert!(stderr.contains("Usage:"), "No usage message: {}", stderr);
}

#[test]
fn invalid_options() {
    let base = data("base", "noname", "gfa");
    let base = path_str(&base);

    let stderr = run_err(&["-c", base]);
    assert!(stderr.contains("requires two graphs"), "Wrong error for -c with one graph: {}", stderr);
    let stderr = run_err(&["-c", base, base, base]);
    assert!(stderr.contains("requires two graphs"), "Wrong error for -c with three graphs: {}", stderr);
    let stderr = run_err(&["-t", "translation.tsv", base]);
    assert!(stderr.contains("requires --compare"), "Wrong error for -t without -c: {}", stderr);
    run_err(&["--no-such-option", base]);
}

#[test]
fn missing_input_file() {
    let dir = scratch_dir("missing_input_file");
    let missing = dir.join("missing.gfa");
    let stderr = run_err(&[path_str(&missing)]);
    assert!(stderr.contains("missing.gfa"), "Error does not name the file: {}", stderr);
    let stderr = run_err(&["-c", path_str(&missing), path_str(&data("base", "noname", "gbz"))]);
    assert!(stderr.contains("missing.gfa"), "Error does not name the file: {}", stderr);
}

#[test]
fn invalid_gfa() {
    let dir = scratch_dir("invalid_gfa");
    let invalid = dir.join("invalid.gfa");
    fs::write(&invalid, "H\tVN:Z:1.1\nL\t1\t+\t2\t+\t0M\n").unwrap();
    let stderr = run_err(&[path_str(&invalid)]);
    assert!(stderr.contains("invalid.gfa"), "Error does not name the file: {}", stderr);
}

//-----------------------------------------------------------------------------
