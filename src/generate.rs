use crate::{
    cli::Cli,
    filename_utils::*,
    parser::{DependencyMap, ParseResult},
};
use std::{collections::HashSet, fs::File, io::prelude::*};

struct GenerateContext<'c, 'p, 'd> {
    cli: &'c Cli<'c>,
    partitioned: &'p PartitionedFiles<'p>,
    dep_map: &'d DependencyMap,
    dlls: &'d Vec<String>,
}

impl<'c, 'p, 'd> GenerateContext<'c, 'p, 'd> {
    pub fn new(
        cli: &'c Cli,
        partitioned: &'p PartitionedFiles,
        dep_map: &'d DependencyMap,
        dlls: &'d Vec<String>,
    ) -> Self {
        Self {
            cli,
            partitioned,
            dep_map,
            dlls,
        }
    }
}

struct PartitionedFiles<'f> {
    standalone: Vec<&'f str>,
    tests: Vec<&'f str>,
    benchmarks: Vec<&'f str>,
    examples: Vec<&'f str>,
}

impl<'f> PartitionedFiles<'f> {
    pub fn partition(cli: &Cli, map: &'f DependencyMap) -> Self {
        macro_rules! contained_in_partition {
            ($cli:ident, $partition:ident, $running:ident) => {
                $cli.$partition.iter().any(|f| {
                    let f = self::strip_extension(f);
                    $running.starts_with(f) || **$running == f
                })
            };
        }

        let with_main: Vec<_> = map
            .keys()
            .filter(|k| map.get(*k).unwrap().1) // filter those which contain a main function
            .map(|k| strip_extension(k.as_str()))
            .collect();

        let tests: Vec<_> = with_main
            .iter()
            .filter(|v| contained_in_partition!(cli, tests, v))
            .map(|v| *v)
            .collect();

        let benchmarks: Vec<_> = with_main
            .iter()
            .filter(|v| contained_in_partition!(cli, benchmarks, v))
            .map(|v| *v)
            .collect();

        let examples: Vec<_> = with_main
            .iter()
            .filter(|v| contained_in_partition!(cli, examples, v))
            .map(|v| *v)
            .collect();

        let standalone: Vec<_> = with_main
            .into_iter()
            .filter(|v| !tests.contains(v) && !benchmarks.contains(v) && !examples.contains(v))
            .collect();

        Self {
            standalone,
            tests,
            benchmarks,
            examples,
        }
    }
}

fn get_all_file_dependencies(file: &str, ext: &str, dep_map: &DependencyMap) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut file_deps = Vec::new();
    get_all_file_dependencies_r(file, ext, dep_map, &mut seen, &mut file_deps);
    file_deps
}

fn get_all_file_dependencies_r(
    file: &str,
    ext: &str,
    dep_map: &DependencyMap,
    seen: &mut HashSet<String>,
    file_deps: &mut Vec<String>,
) {
    if dep_map.contains_key(file) {
        file_deps.push(file.to_owned());
        seen.insert(file.to_owned());

        let dependencies = &dep_map.get(file).unwrap().0;
        for dependency in dependencies {
            if !seen.contains(dependency) {
                get_all_file_dependencies_r(dependency, ext, dep_map, seen, file_deps);
            }

            let stripped = strip_extension(dependency);
            let complementary_file = if has_extension(dependency, ext) {
                format!("{}.h", stripped)
            } else {
                format!("{}.{}", stripped, ext)
            };

            if dep_map.contains_key(&complementary_file) && !seen.contains(&complementary_file) {
                get_all_file_dependencies_r(&complementary_file, ext, dep_map, seen, file_deps);
                // file_deps.push(complementary_file);
            }
        }
    }
}

fn flatten_dependencies(dep_map: &DependencyMap, ext: &str) -> DependencyMap {
    let mut new_dep_map = DependencyMap::new();

    for file in dep_map.keys().filter(|f| has_extension(f, ext)) {
        let file_deps = get_all_file_dependencies(file, ext, &dep_map);
        let has_main = dep_map.get(file).unwrap().1;
        new_dep_map.insert(file.to_owned(), (file_deps, has_main));
    }

    new_dep_map
}

pub fn generate_makefile(cli: &Cli, parse_result: ParseResult) -> std::io::Result<()> {
    let mut makefile = File::create("Makefile")?;
    let dep_map = flatten_dependencies(&parse_result.dependency_map, cli.extension);
    let partitioned = PartitionedFiles::partition(cli, &parse_result.dependency_map);
    let ctx = GenerateContext::new(cli, &partitioned, &dep_map, &parse_result.dlls);

    generate_compiler_variables(&mut makefile, &ctx)?;
    generate_file_variables(&mut makefile, &ctx)?;
    generate_targets(&mut makefile, &ctx)?;

    Ok(())
}

fn generate_compiler_variables(makefile: &mut File, ctx: &GenerateContext) -> std::io::Result<()> {
    writeln!(
        makefile,
        "CC := {compiler}\n\
        CFLAGS := -Wall\n\
        CFLAGS += -std={std}\n\
        CFLAGS += -{opt}\n\
        LFLAGS := {link_flags}",
        compiler = ctx.cli.compiler,
        std = ctx.cli.standard,
        opt = ctx.cli.opt_level,
        link_flags = ctx
            .dlls
            .iter()
            .map(|dll| format!("-l{}", dll))
            .collect::<Vec<_>>()
            .join(" ")
    )?;

    Ok(())
}

fn generate_file_variables(makefile: &mut File, ctx: &GenerateContext) -> std::io::Result<()> {
    writeln!(makefile, "\nODIR := .OBJ\n")?;

    for file in ctx.dep_map.keys() {
        generate_source_file_dependencies_variable_for_file(makefile, file, ctx)?;
    }

    writeln!(makefile)?;

    Ok(())
}

fn generate_object_file_dependencies_variable_for_file(
    makefile: &mut File,
    file: &str,
    ctx: &GenerateContext,
) -> std::io::Result<()> {
    let var_name = strip_extension(file);
    let var_name = object_file_dependencies_var_name(var_name);
    write!(makefile, "{} := ", var_name)?;

    let dependencies = &ctx.dep_map.get(file).unwrap().0;
    let object_dependencies = dependencies
        .iter()
        .filter(|d| has_extension(d, ctx.cli.extension))
        .map(|d| format!("$(ODIR)/{}.o", escape_folder(strip_extension(d))))
        .collect::<Vec<_>>()
        .join(" ");

    writeln!(makefile, "{}", object_dependencies)?;

    Ok(())
}

fn generate_source_file_dependencies_variable_for_file(
    makefile: &mut File,
    file: &str,
    ctx: &GenerateContext,
) -> std::io::Result<()> {
    let var_name = strip_extension(file);
    let var_name = source_file_dependencies_var_name(&var_name);
    write!(makefile, "{} := ", var_name)?;

    let dependencies = &ctx.dep_map.get(file).unwrap().0;
    writeln!(makefile, "{}", dependencies.join(" "))?;

    Ok(())
}

fn generate_targets(makefile: &mut File, ctx: &GenerateContext) -> std::io::Result<()> {
    macro_rules! generate_target {
        ($makefile:ident, $ctx:ident, $id:ident) => {
            if $ctx.partitioned.$id.len() > 0 {
                std::write!($makefile, "{}: ", std::stringify!($id))?;

                for file in &$ctx.partitioned.$id {
                    std::write!($makefile, "{} ", self::escape_folder(file))?;
                }

                writeln!(makefile, "\n")?;

                for file in &$ctx.partitioned.$id {
                    generate_object_file_dependencies_variable_for_file(
                        makefile,
                        &format!("{}.{}", file, ctx.cli.extension),
                        ctx,
                    )?;

                    std::writeln!(
                        $makefile,
                        "\n{target}: $(ODIR) $({dep_var})\n\
                            \t$(CC) $(CFLAGS) $({dep_var}) -o {out}\n",
                        target = self::escape_folder(file),
                        dep_var = self::object_file_dependencies_var_name(file),
                        out = file
                    )?;
                }
            }
        };
    }

    writeln!(
        makefile,
        "all: binaries\n\n\
        $(ODIR):\n\
            \t@mkdir $(ODIR)\n",
    )?;

    // We should always have at least one standalone binary which is the main program
    write!(makefile, "binaries: ")?;

    let main_file = strip_extension(ctx.cli.main_file);

    for bin_file in &ctx.partitioned.standalone {
        let (prefix, name) = if *bin_file != main_file {
            ("bin_", *bin_file)
        } else {
            ("", ctx.cli.binary)
        };

        write!(
            makefile,
            "{prefix}{name} ",
            prefix = prefix,
            name = escape_folder(name)
        )?;
    }

    writeln!(makefile, "\n")?;

    for bin_file in &ctx.partitioned.standalone {
        generate_object_file_dependencies_variable_for_file(
            makefile,
            &format!("{}.{}", bin_file, ctx.cli.extension),
            ctx,
        )?;

        let (prefix, name) = if *bin_file != main_file {
            ("bin_", *bin_file)
        } else {
            ("", ctx.cli.binary)
        };

        writeln!(
            makefile,
            "\n{prefix}{name}: $(ODIR) $({dep_var})\n\
                    \t$(CC) $(CFLAGS) $({dep_var}) -o {out} $(LFLAGS)\n",
            prefix = prefix,
            name = escape_folder(name),
            dep_var = object_file_dependencies_var_name(bin_file),
            out = name
        )?;
    }

    generate_target!(makefile, ctx, tests);
    generate_target!(makefile, ctx, benchmarks);
    generate_target!(makefile, ctx, examples);

    for file in ctx
        .dep_map
        .keys()
        .filter(|k| has_extension(k, ctx.cli.extension))
        .map(|k| strip_extension(k))
    {
        writeln!(
            makefile,
            "$(ODIR)/{out}.o: $(ODIR) $({source_var})\n\
                \t$(CC) -c $(CFLAGS) {file}.{extension} -o $(ODIR)/{out}.o\n",
            file = file,
            source_var = source_file_dependencies_var_name(file),
            extension = ctx.cli.extension,
            out = escape_folder(file),
        )?;
    }

    generate_clean_target(makefile, ctx)?;

    Ok(())
}

fn generate_clean_target(makefile: &mut File, ctx: &GenerateContext) -> std::io::Result<()> {
    write!(
        makefile,
        ".PHONY: clean\n\
        clean:\n\
            \trm -rf .OBJ ",
    )?;

    let main_file = strip_extension(ctx.cli.main_file);

    let all_files = ctx
        .partitioned
        .standalone
        .iter()
        .map(|f| if *f != main_file { f } else { &ctx.cli.binary })
        .chain(ctx.partitioned.tests.iter())
        .chain(ctx.partitioned.benchmarks.iter())
        .chain(ctx.partitioned.examples.iter());

    for file in all_files {
        write!(makefile, "{} ", file)?;
    }

    writeln!(makefile)?;

    Ok(())
}

#[inline]
fn escape_folder(filename: &str) -> String {
    filename.replace('/', "_")
}

#[inline]
fn file_dependencies_var_name(filename: &str, category: &str) -> String {
    let var_name = escape_folder(filename);
    format!("{}_{}_DEPS", var_name.to_ascii_uppercase(), category)
}

#[inline]
fn source_file_dependencies_var_name(filename: &str) -> String {
    file_dependencies_var_name(filename, "SOURCE")
}

#[inline]
fn object_file_dependencies_var_name(filename: &str) -> String {
    file_dependencies_var_name(filename, "OBJECT")
}
