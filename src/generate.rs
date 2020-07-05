use crate::{
    cli::Cli,
    filename_utils::*,
    parser::{DependencyMap, ParseResult},
};
use std::{collections::HashSet, fs::File, io::prelude::*};

struct GenerateContext<'c, 'p, 'pr> {
    cli: &'c Cli<'c>,
    partitioned: &'p PartitionedFiles<'p>,
    parse_result: &'pr ParseResult,
}

impl<'c, 'p, 'pr> GenerateContext<'c, 'p, 'pr> {
    pub fn new(
        cli: &'c Cli,
        partitioned: &'p PartitionedFiles,
        parse_result: &'pr ParseResult,
    ) -> Self {
        Self {
            cli,
            partitioned,
            parse_result,
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

pub fn generate_makefile(cli: &Cli, parse_result: ParseResult) -> std::io::Result<()> {
    let mut makefile = File::create("Makefile")?;
    let partitioned = PartitionedFiles::partition(cli, &parse_result.dependency_map);
    let ctx = GenerateContext::new(cli, &partitioned, &parse_result);

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
            .parse_result
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

    for file in ctx
        .parse_result
        .dependency_map
        .keys()
        .filter(|f| has_extension(f, ctx.cli.extension))
    {
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

    write_object_file_dependencies(makefile, &file, ctx)?;
    writeln!(makefile)?;
    Ok(())
}

fn write_object_file_dependencies(
    makefile: &mut File,
    file: &str,
    ctx: &GenerateContext,
) -> std::io::Result<()> {
    let mut seen = HashSet::new();
    write_object_file_dependencies_r(makefile, file, &mut seen, ctx)?;
    Ok(())
}

fn write_object_file_dependencies_r(
    makefile: &mut File,
    filename: &str,
    seen: &mut HashSet<String>,
    ctx: &GenerateContext,
) -> std::io::Result<()> {
    write!(makefile, "$(ODIR)/{}.o ", escape_folder(strip_extension(filename)))?;
    seen.insert(filename.to_owned());

    let dependencies = &ctx.parse_result.dependency_map.get(filename).unwrap().0;
    for dependency in dependencies
        .iter()
        .map(|d| format!("{}.{}", strip_extension(d), ctx.cli.extension))
    {
        if ctx.parse_result.dependency_map.contains_key(&dependency) && !seen.contains(&dependency)
        {
            write_object_file_dependencies_r(makefile, &dependency, seen, ctx)?;
        }
    }
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

    write_source_file_dependencies(makefile, &file, ctx)?;
    writeln!(makefile)?;

    Ok(())
}

fn write_source_file_dependencies(
    makefile: &mut File,
    filename: &str,
    ctx: &GenerateContext,
) -> std::io::Result<()> {
    let mut seen = HashSet::new();
    write_source_file_dependecies_r(makefile, filename, &mut seen, ctx)?;
    Ok(())
}

fn write_source_file_dependecies_r(
    makefile: &mut File,
    filename: &str,
    seen: &mut HashSet<String>,
    ctx: &GenerateContext,
) -> std::io::Result<()> {
    write!(makefile, "{} ", filename)?;
    seen.insert(filename.to_owned());

    let dependencies = &ctx.parse_result.dependency_map.get(filename).unwrap().0;
    for dependency in dependencies {
        if !seen.contains(dependency) {
            seen.insert(dependency.to_owned());
            write!(makefile, "{} ", dependency)?;
        }

        let dependency = strip_extension(dependency);
        let dependency = std::format!("{}.{}", dependency, ctx.cli.extension);

        if ctx.parse_result.dependency_map.contains_key(&dependency) && !seen.contains(&dependency)
        {
            write_source_file_dependecies_r(makefile, &dependency, seen, ctx)?;
        }
    }

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
                            \t$(CC) $({dep_var}) -o {out}\n",
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
                    \t$(CC) $({dep_var}) -o {out} $(LFLAGS)\n",
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
        .parse_result
        .dependency_map
        .keys()
        .filter(|k| has_extension(k, ctx.cli.extension))
        .map(|k| strip_extension(k))
    {
        writeln!(
            makefile,
            "$(ODIR)/{out}.o: $(ODIR) $({source_var})\n\
                \t$(CC) -c {file}.{extension} -o $(ODIR)/{out}.o\n",
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
