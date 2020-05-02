use clap::ArgMatches;
use std::collections::HashSet;

pub struct Cli<'cli> {
    pub main_file: &'cli str,
    pub compiler: &'cli str,
    pub extension: &'cli str,
    pub binary: &'cli str,
    pub standard: &'cli str,
    pub opt_level: &'cli str,
    pub tests: HashSet<&'cli str>,
    pub benchmarks: HashSet<&'cli str>,
    pub examples: HashSet<&'cli str>,
}

impl<'cli> Cli<'cli> {
    pub fn from_matches(matches: &'cli ArgMatches<'cli>) -> Result<Self, &'static str> {
        let extension = matches
            .value_of("extension")
            .ok_or("You must provide and file extension to search for")?;

        if extension != "c" && extension != "cpp" {
            return Err("Only C or C++ files are allowed (extension should be either c or cpp)");
        }

        let binary = matches
            .value_of("bin")
            .ok_or("You must provide a name for your executable")?;

        let main_file = matches
            .value_of("main_file")
            .ok_or("You must provide the main source file")?;

        let compiler = matches.value_of("compiler").ok_or("")?;

        let standard = matches.value_of("std").unwrap();

        let opt_level = matches.value_of("opt").unwrap();

        let tests: HashSet<_> = matches.values_of("tests").unwrap().collect();

        let benchmarks: HashSet<_> = matches.values_of("benchmarks").unwrap().collect();

        let examples: HashSet<_> = matches.values_of("examples").unwrap().collect();

        Ok(Self {
            binary,
            main_file,
            compiler,
            extension,
            standard,
            opt_level,
            tests,
            benchmarks,
            examples,
        })
    }
}
