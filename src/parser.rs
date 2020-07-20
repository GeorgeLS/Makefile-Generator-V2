use crate::{cli::Cli, filename_utils::*};
use std::{
    collections::{HashMap, HashSet},
    error::Error,
    fs,
    path::{Path, PathBuf},
};
use walkdir::{DirEntry, WalkDir};

// The bool indicates whether the key (source file) has a main function in it or not
pub type DependencyMap = HashMap<String, (Vec<String>, bool)>;

#[derive(Debug)]
pub struct ParseResult {
    pub dependency_map: DependencyMap,
    pub dlls: Vec<String>,
}

pub struct Parser<'cli> {
    root_dir: PathBuf,
    cli: &'cli Cli<'cli>,
}

struct ParseContext<'c> {
    dependency_map: &'c mut DependencyMap,
    dlls: &'c mut Vec<String>,
    seen: HashSet<String>,
}

// These are some default mappings for dynamic linked libraries
lazy_static! {
    static ref DLL_MAP: HashMap<&'static str, &'static str> = {
        let mut dll_map = HashMap::new();
        dll_map.insert("math.h", "m");
        dll_map.insert("pthread.h", "pthread");
        dll_map.insert("ncurses.h", "ncurses");
        dll_map
    };
}

impl ParseResult {
    pub fn new(dependency_map: DependencyMap, dlls: Vec<String>) -> Self {
        Self {
            dependency_map,
            dlls,
        }
    }
}

impl<'c> ParseContext<'c> {
    pub fn new(dependency_map: &'c mut DependencyMap, dlls: &'c mut Vec<String>) -> Self {
        Self {
            dependency_map,
            dlls,
            seen: HashSet::new(),
        }
    }
}

impl<'cli> Parser<'cli> {
    pub fn new(root_dir: PathBuf, cli: &'cli Cli<'cli>) -> Self {
        Self { root_dir, cli }
    }

    pub fn parse(&self) -> Result<ParseResult, Box<dyn Error>> {
        let mut dependency_map = HashMap::new();
        let mut dlls = Vec::new();

        let filter_criteria = |r: &Result<DirEntry, _>| {
            r.as_ref()
                .map(|e| e.file_type().is_file() && has_extension(e.path(), self.cli.extension))
                .unwrap_or(false)
        };

        let walker = WalkDir::new(&self.root_dir).into_iter();
        for entry in walker
            .filter_entry(|e| !is_hidden(e))
            .filter(|r| filter_criteria(r))
        {
            if let Ok(entry) = entry {
                let mut ctx = ParseContext::new(&mut dependency_map, &mut dlls);
                let filename = entry.path().strip_prefix(&self.root_dir)?;
                read_file_and_get_include_files_recursively(&self.root_dir, filename, &mut ctx)?;
            }
        }

        Ok(ParseResult::new(dependency_map, dlls))
    }
}

#[derive(Debug, Eq, PartialEq)]
enum IncludeFile<'i> {
    System(&'i str),
    User(&'i str),
}

fn extract_include_filename(line: &str) -> IncludeFile<'_> {
    let (start_index, end_index) = (line.find('<'), line.find('>'));

    let mut is_system_file = true;
    let (start_index, end_index) = if start_index.is_none() || end_index.is_none() {
        let start_index = line.find('"').unwrap();
        let start_pos = start_index + 1;
        let end_index = line[start_pos..].find('"').unwrap();
        let end_index = start_pos + end_index;
        is_system_file = false;
        (start_index, end_index)
    } else {
        (start_index.unwrap(), end_index.unwrap())
    };

    let include_file = &line[(start_index + 1)..end_index];

    if is_system_file {
        IncludeFile::System(include_file)
    } else {
        IncludeFile::User(include_file)
    }
}

fn get_include_files_and_update_dlls(source: &str, dlls: &mut Vec<String>) -> Vec<String> {
    let mut include_files = Vec::new();
    source
        .lines()
        .filter(|line| line.trim_start().starts_with("#include"))
        .for_each(|line| {
            let include_file = extract_include_filename(line);
            match include_file {
                IncludeFile::System(include_file) => {
                    if DLL_MAP.contains_key(include_file) {
                        let linkage_name = DLL_MAP.get(include_file).unwrap().to_string();
                        if !dlls.contains(&linkage_name) {
                            dlls.push(linkage_name);
                        }
                    }
                }
                IncludeFile::User(include_file) => {
                    include_files.push(include_file.to_string());
                }
            }
        });

    include_files
}

fn read_file_and_get_include_files_recursively(
    root_dir: &PathBuf,
    filename: &Path,
    ctx: &mut ParseContext,
) -> Result<(), Box<dyn Error>> {
    let contents = fs::read_to_string(filename)?;
    let has_main = contents.contains("main(");
    let mut include_files = get_include_files_and_update_dlls(&contents, ctx.dlls);

    for include_file in &mut include_files {
        let mut full_path = root_dir.to_path_buf();
        full_path.push(filename);
        full_path.pop();
        full_path.push(&include_file);
        full_path = full_path.canonicalize()?;

        *include_file = full_path
            .strip_prefix(root_dir)?
            .to_str()
            .unwrap()
            .to_string();

        if !ctx.dependency_map.contains_key(include_file) && !ctx.seen.contains(include_file) {
            ctx.seen.insert(include_file.to_string());
            read_file_and_get_include_files_recursively(root_dir, Path::new(include_file), ctx)?;
        }
    }

    let filename = filename.to_str().unwrap();
    if !ctx.dependency_map.contains_key(filename) {
        ctx.dependency_map
            .insert(filename.to_string(), (include_files, has_main));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_include_filename_works() {
        let source = r##"
            #include <stdio.h>
            #include <stdlib.h>
            #include "my_header.h"
            #include "string_interning.h"

            int main() {
                return 0;
            }
        "##;

        let includes: Vec<_> = source
            .lines()
            .filter(|l| l.trim_start().starts_with("#include"))
            .map(|l| extract_include_filename(l))
            .collect();

        assert_eq!(
            includes,
            vec![
                IncludeFile::System("stdio.h"),
                IncludeFile::System("stdlib.h"),
                IncludeFile::User("my_header.h"),
                IncludeFile::User("string_interning.h")
            ]
        );
    }

    #[test]
    fn get_include_files_and_update_dlls_works() {
        let source = r##"
            #include <stdio.h>
            #include <stdlib.h>
            #include <math.h>
            #include <pthread.h>
            #include "my_header.h"
            #include "string_interning.h"

            int main() {
                return 0;
            }
        "##;

        let mut dlls = Vec::new();
        let include_files = get_include_files_and_update_dlls(source, &mut dlls);

        assert_eq!(include_files, vec!["my_header.h", "string_interning.h"]);
        assert_eq!(dlls, vec!["m", "pthread"]);
    }
}
