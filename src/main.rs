use std::{collections::HashMap, env::args, ffi::OsStr, fs, path::Path};

fn extract_included_filename(line: &str) -> Option<&str> {
    let mut start_index = line.find("<");
    let mut end_index = if start_index.is_some() {
        line.find(">")
    } else {
        None
    };

    if start_index.is_none() && end_index.is_none() {
        start_index = line.find("\"");
        end_index = if start_index.is_some() {
            line[(start_index.unwrap() + 1)..].find("\"")
        } else {
            None
        };

        if end_index.is_some() {
            end_index = Some(start_index.unwrap() + end_index.unwrap() + 1);
        }
    }

    let res = if start_index.is_some() && end_index.is_some() {
        let start_index = start_index.unwrap() + 1;
        let end_index = end_index.unwrap();
        Some(&line[start_index..end_index])
    } else {
        None
    };

    res
}

#[derive(Debug)]
struct ExtensionSplittedFile<'fname> {
    original: &'fname str,
    filename: &'fname str,
    extension: Option<&'fname str>,
}

impl<'fname> ExtensionSplittedFile<'fname> {
    pub fn new(filename: &'fname str) -> Self {
        let extension = Path::new(filename).extension().and_then(OsStr::to_str);
        let mut filename_without_ext = filename;
        if extension.is_some() {
            filename_without_ext = &filename[..(filename.len() - extension.unwrap().len() - 1)];
        }
        Self {
            original: filename,
            filename: filename_without_ext,
            extension,
        }
    }

    #[inline]
    pub fn with_extension(&self) -> &'fname str {
        self.original
    }

    #[inline]
    pub fn filename(&self) -> &'fname str {
        self.filename
    }

    #[inline]
    pub fn extension(&self) -> Option<&'fname str> {
        self.extension
    }
}

fn get_include_files(source: &str) -> Vec<ExtensionSplittedFile> {
    let mut include_files = Vec::new();
    for line in source.lines() {
        if line.starts_with("#include") {
            if let Some(include_file) = extract_included_filename(line) {
                include_files.push(ExtensionSplittedFile::new(include_file));
            }
        }
    }
    include_files
}

fn read_file_and_get_include_files<'fname>(filename: String, mut map: HashMap<String, Vec<ExtensionSplittedFile>>) {
    if let Ok(contents) = fs::read_to_string(&filename) {
        let source_file = ExtensionSplittedFile::new(&filename);
        let include_files = get_include_files(&contents);
        if !map.contains_key(&filename) {
            map.insert(filename, include_files);
        }
    } else {
        eprintln!("Error while reading file `{}`", filename);
    }
}

fn main() {
    if let Some(fname) = args().skip(1).next() {
        let mut map = HashMap::new();
        if let Ok(contents) = fs::read_to_string(&fname) {
            let source_file = ExtensionSplittedFile::new(&fname);
            let include_files = get_include_files(&contents);
            if !map.contains_key(&fname) {
                map.insert(&fname, include_files);
            }
        } else {
            eprintln!("Error while reading file `{}`", fname);
        }
    } else {
        eprintln!("Please provide an input filename to read");
    }
}
