#include <sys/stat.h>
#include <dirent.h>
#include "lexer.h"
#include "utils.h"
#include "scoped_timer.h"
#include "common.h"
#include "report.h"
#include "index.h"
#include "parse.h"

ParseStats parse_stats{};

[[noreturn]] internal inline void usage() {
    report("\ndcgraph is a tool written in C++ that takes as input TCL code, parses it and extracts information regarding the procedure call dependencies.\n"
        "\n"
        "USAGE:\n"
        "\tdcgraph [OPTIONS]\n"
        "\tdcgraph [OPTIONS] [PATH...]\n"
        "\n"
        "ARGS:\n"
        "\t<PATH>...:\n"
        "\t\tA TCL file or a directory containing TCL files that will be searched recursively.\n"
        "\n"
        "OPTIONS:\n"
        "\t-h, --help\n"
        "\t\tPrint helpful information about the program.\n"
        "\n"
        "\t-b (TCL_FILE | DIRECTORY)+\n"
        "\t\tSpecify that you want to build an index using the provided tcl files or directory\n"
        "\n"
        "\t-f <PROCEDURE_NAME>+\n"
        "\t\tQuery the call sequence for the specified procedure(s)\n"
        "\n"
        "\t-d\n"
        "\t\tPrint the dependencies of the procedure instead of the call sequence. Can be used with\n"
        "\t\t-f and must appear before it.\n"
        "\n"
        "\t--max-depth <NUMBER>\n"
        "\t\tSpecify the maximum depth of the call sequence that will be printed. Must be a positive number. Defaults to 5.\n"
        "\n"
        "\t--delete-index\n"
        "\t\tDeletes the index file, if any.\n"
        "\n"
        "By not providing -b and -f flags the program runs in interactive mode.\n"
        "In that mode you can type a procedure name at each time and it will print the call sequence for that procedure.\n"
        "By writing -d after the procedure name, the program will print all the dependencies that the procedure has.\n"
        "That means it will print all the procedure names that call directly the procedure we are querying for."
    );
    exit(EXIT_SUCCESS);
}

enum class CLIOption : uint8_t {
    BUILD_INDEX = 0x1,
    QUERY_FUNCTION = 0x2,
    INTERACTIVE_MODE = 0x4
};

struct Config {
    Config() = default;

    local constexpr size_t NR_CLI_OPTS = 3U;
    local constexpr char *BUILD_INDEX_OPT = _("-b");
    local constexpr char *QUERY_FUNCTION_OPT = _("-f");
    local constexpr char *PRINT_DEP_OPT = _("-d");
    local constexpr char *MAX_DEPTH_OPT = _("--max-depth");
    local constexpr char *DELETE_INDEX_OPT = _("--delete-index");
    local constexpr char *HELP_OPT = _("-h");
    local constexpr char *LONG_HELP_OPT = _("--help");

    Vector<char *> opt_values[NR_CLI_OPTS]{};
    size_t max_depth{5U};
    uint8_t opts{0U};
    bool delete_index{false};
    bool print_dependencies{false};

private:
    local bool is_option(const char *v) {
        return !strcmp(v, BUILD_INDEX_OPT) ||
            !strcmp(v, QUERY_FUNCTION_OPT) ||
            !strcmp(v, PRINT_DEP_OPT) ||
            !strcmp(v, MAX_DEPTH_OPT) ||
            !strcmp(v, DELETE_INDEX_OPT);
    }

public:
    local Config parse_arguments(int argc, char *args[]) {
        --argc;
        ++args;
        Config cfg = Config();
        for (int i = 0; i < argc; ++i) {
            const char *arg = args[i];
            if (!strcmp(arg, BUILD_INDEX_OPT)) {
                cfg.opts |= u8(CLIOption::BUILD_INDEX);
                Vector<char *> &build_opt_vec = cfg.opt_values[0];
                ++i;
                while (i != argc && !is_option(args[i])) {
                    build_opt_vec.push_back(args[i]);
                    ++i;
                }
            } else if (!strcmp(arg, QUERY_FUNCTION_OPT)) {
                cfg.opts |= u8(CLIOption::QUERY_FUNCTION);
                Vector<char *> &query_f_opt_vec = cfg.opt_values[1];
                ++i;
                while (i != argc && !is_option(args[i])) {
                    query_f_opt_vec.push_back(args[i]);
                    ++i;
                }
            } else if (!strcmp(arg, MAX_DEPTH_OPT)) {
                ++i;
                i64 max_depth;
                if (!string_to_i64(args[i], &max_depth) && max_depth <= 0) {
                    fatal("You must provide a positive number as the max depth");
                }
                cfg.max_depth = max_depth;
            } else if (!strcmp(arg, PRINT_DEP_OPT)) {
                cfg.print_dependencies = true;
            } else if (!strcmp(arg, DELETE_INDEX_OPT)) {
                cfg.delete_index = true;
                // If we want to delete the index then just return
                return cfg;
            } else if (!strcmp(arg, HELP_OPT) || !strcmp(arg, LONG_HELP_OPT)) {
                usage();
            } else {
                fatal("Unknown option \"%s\". Please run \"dcgraph -h\" or \"dcgraph --help\" for more information.", arg);
            }
        }
        if (!(cfg.opts & u8(CLIOption::BUILD_INDEX)) && !(cfg.opts & u8(CLIOption::QUERY_FUNCTION))) {
            cfg.opts |= u8(CLIOption::INTERACTIVE_MODE);
        }
        return cfg;
    }
};

internal inline void set_output_color_to_red() {
    printf("\033[1;31m");
}

internal inline void reset_output_color() {
    printf("\033[0m");
}

static void print_with_leading_spaces(const char *fmt, const char *msg, int indent) {
    printf(fmt, indent, "", msg);
}

static void print_call_sequence(char *entry_point, MemoryIndexMap &map, int depth, int indent = 0) {
    static const char *enter_call_fmt = "%*s-> %s\n";
    static const char *leave_call_fmt = "%*s<- %s\n";

    if (depth == 0) return;

    print_with_leading_spaces(enter_call_fmt, entry_point, indent);

    if (!map.contains(entry_point)) {
        print_with_leading_spaces(enter_call_fmt, "...", indent + 2);
        print_with_leading_spaces(leave_call_fmt, "...", indent + 2);
    } else {
        Vector<char *> &call_list = map[entry_point];
        for (char *call : call_list) {
            if (!strcmp(entry_point, call)) {
                // We have recursion. Prevent infinite _loop_
                continue;
            }
            print_call_sequence(call, map, depth - 1, indent + 2);
        }
    }
    print_with_leading_spaces(leave_call_fmt, entry_point, indent);
}

internal void parse_tcl_files(Config &cfg, IndexMap &call_map, IndexMap &dep_map) {
    report("Parsing tcl files...");
    ScopedTimer t{"Parsed tcl files"};
    Vector<char *> &files = cfg.opt_values[0];

    for (char *fname: files) {
        char *ext = file_extension(fname);

        // Skip any files that don't have the tcl extension.
        // We assume these files not be tcl files.
        // Also if the file name hasn't got an extension
        // we continue further and we assert that it's not
        // a file and it is a directory.
        if (ext && strcmp(ext, "tcl") != 0) {
            continue;
        }

        mode_t file_type = get_file_type(fname);

        // That means that the file probably doesn't
        // exist or we don't have the right permissions to read it.
        if (file_type == -1) {
            report("Error while getting file's (%s) type: %s", fname, strerror(errno));
            if (confirm("Do you want to continue and skip this file?")) {
                continue;
            } else {
                exit(EXIT_FAILURE);
            }
        }

        // If the file isn't a regular file or a directory then
        // this is considered an error.
        if (!S_ISREG(file_type) && !S_ISDIR(file_type)) {
            report("File \"%s\" isn't a regular file or a directory.", fname);
            if (confirm("Do you want to continue and skip this file?")) {
                continue;
            } else {
                exit(EXIT_FAILURE);
            }
        }

        if (S_ISREG(file_type)) {
            parse_tcl_file(fname, call_map, dep_map);
        } else {
            parse_tcl_files_in_directory(fname, call_map, dep_map);
        }
    }
}

internal bool should_print_dependencies(char *str) {
    while (*str && !isspace(*str)) {
        ++str;
    }
    ++str;
    return !strcmp(str, "-d");
}

internal void crop_to_procedure_name(char *str) {
    while (*str && !isspace(*str)) {
        ++str;
    }
    str[0] = '\0';
}

internal void print_dependencies(char *proc_name, MemoryIndexMap &dep_map) {
    Vector<char *> &dependencies = dep_map[proc_name];
    size_t number{1U};
    unsigned int max_padding = number_of_digits(dependencies.size);
    printf("\n");
    for (char *dep: dependencies) {
        unsigned int padding = max_padding - number_of_digits(number);
        printf("%*s%zu. %s\n", padding, "", number, dep);
        ++number;
    }
    printf("\n");
}

internal void query_function(Config &cfg, MemoryIndexMap &call_map, MemoryIndexMap &dep_map) {
    Vector<char *> &procs = cfg.opt_values[1];
    for (char *proc : procs) {
        if (cfg.print_dependencies) {
            if (dep_map.contains(proc)) {
                set_output_color_to_red();
                print_dependencies(proc, dep_map);
                reset_output_color();
            } else {
                report("There's no dependency info available for procedure \"%s\"", proc);
            }
        } else {
            if (call_map.contains(proc)) {
                print_call_sequence(proc, call_map, cfg.max_depth);
            } else {
                report("There's no info available for procedure \"%s\"", proc);
            }
        }
    }
}

internal void run_interactive(Config &cfg, MemoryIndexMap &call_map, MemoryIndexMap &dep_map) {
    for (;;) {
        char *proc_name{nullptr};
        size_t len{0U};
        printf("\nEnter a procedure name (add -d at the end to print the dependencies): ");
        if (getline(&proc_name, &len, stdin) == -1) {
            fatal("There was an error reading stdin!");
        }
        proc_name[strlen(proc_name) - 1] = '\0';

        if (should_print_dependencies(proc_name)) {
            crop_to_procedure_name(proc_name);
            if (dep_map.contains(proc_name)) {
                set_output_color_to_red();
                print_dependencies(proc_name, dep_map);
                reset_output_color();
            } else {
                report("There's no dependency info available for procedure \"%s\"", proc_name);
            }
        } else {
            if (call_map.contains(proc_name)) {
                set_output_color_to_red();
                printf("\n");
                print_call_sequence(proc_name, call_map, cfg.max_depth);
                reset_output_color();
            } else {
                report("There's no info available for procedure \"%s\"", proc_name);
            }
        }

        ::free(proc_name);
        proc_name = nullptr;
        len = 0U;
    }
}

int main(int argc, char *args[]) {
    Config cfg = Config::parse_arguments(argc, args);

    if (cfg.delete_index) {
        delete_index_file();
        report("Deleted index file.");
        return EXIT_SUCCESS;
    }

    if (cfg.opts & u8(CLIOption::BUILD_INDEX)) {
        IndexMap call_map{};
        IndexMap dep_map{};
        parse_tcl_files(cfg, call_map, dep_map);
        report("Number of TCL files parsed: %zu", parse_stats.nr_files);
        report("Building and writing index...");
        write_index_file(call_map, dep_map);
    } else if (cfg.opts & u8(CLIOption::INTERACTIVE_MODE) || cfg.opts & u8(CLIOption::QUERY_FUNCTION)) {
        report("Reading index...");
        auto[call_map, dep_map] = read_index_file();
        if (cfg.opts & u8(CLIOption::QUERY_FUNCTION)) {
            query_function(cfg, call_map, dep_map);
        } else {
            run_interactive(cfg, call_map, dep_map);
        }
    }

    return EXIT_SUCCESS;
}
