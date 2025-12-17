# 🧠 Petri Net-based Analysis Tool for Rust Programs

This repository provides an analysis tool based on **Petri Nets** for Rust programs, supporting multiple modes such as deadlock detection, data race detection, and memory safety analysis.

---

## 📄 Paper

This repository accompanies the paper:

**"Rust-PN: Petri Net-based Static Analysis for Rust Programs"**  
[arXiv:2212.02754](https://arxiv.org/abs/2212.02754)


---

## ⚙️ Installation

```bash
sudo apt-get install gcc g++ clang llvm
rustup component add rust-src rustc-dev llvm-tools-preview
cargo install --path .
```


## 🧩 Usage

```bash
cd path/to/your/rust/project; cargo clean;
```
```c
const CARGO_PN_HELP: &str = r#"Petri Net-based Analysis Tool for Rust Programs

 USAGE:
     cargo pn [OPTIONS] [-- <rustc-args>...]

 OPTIONS:
     -h, --help                      Print help information
     -V, --version                   Print version information
     -m, --mode <TYPE>              Analysis mode:
                                   - deadlock: Deadlock detection
                                   - datarace: Data race detection
                                   - memory: Memory safety analysis
                                   - [default: deadlock]
     -t, --target <NAME>            Target crate for analysis(Only underlined links can be used)
     --pn-analysis-output=<PATH>            Output path for analysis results [default: diagnostics.json]
         --type <TYPE>              Target crate type (binary/library) [default: binary]
         --api-spec <PATH>          Path to library API specification file
     --pn-test                      Do not perform state reduction

 VISUALIZATION OPTIONS:
         --viz-callgraph            Generate call graph visualization
         --viz-petrinet            Generate Petri net visualization
         --viz-stategraph          Generate state graph visualization
         --viz-unsafe              Generate unsafe operations report
         --viz-pointsto            Generate points-to relations report
         --viz-mir                 Generate MIR visualization (dot format)

 DEBUG OPTIONS:
         --stop-after <STAGE>      Stop analysis after specified stage:
                                   - mir: After MIR output
                                   - callgraph: After call graph construction
                                   - petrinet: After Petri net construction
                                   - stategraph: After state graph construction

 EXAMPLES:
     cargo pn -m deadlock -t my_crate --pn-analysis-dir=./tmp --viz-petrinet

     # Output MIR dot files for debugging
     cargo pn -t my_crate --viz-mir

     # Stop after Petri net construction (useful for debugging)
     cargo pn -t my_crate --viz-petrinet --stop-after petrinet

     # Output both MIR and Petri net for comparison
     cargo pn -t my_crate --viz-mir --viz-petrinet

     You need to specify the crate to analyze and replace '-' with an underscore '_'.
 "#;


```
