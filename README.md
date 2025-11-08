```bash
 sudo apt-get install gcc g++ clang llvm
```

```bash
 rustup component add rust-src
 rustup component add rustc-dev
 rustup component add llvm-tools-preview
 cargo install --path .
```

`cd path/to/your/rust/project; cargo clean;`

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
         --viz-pointsto

 EXAMPLES:
     cargo pn -m deadlock -t my_crate --pn-analysis-dir=./tmp --viz-petrinet

     You need to specify the crate to analyze and replace '-' with an underscore '_'.
 "#;


```
