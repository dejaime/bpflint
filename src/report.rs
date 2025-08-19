use std::io;
use std::path::Path;

use anyhow::Result;

use crate::LintMatch;
use crate::lines::Lines;


/// Configuration options for terminal reporting.
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Opts {
    /// Extra context lines: (lines_before, lines_after).
    pub extra_lines: Option<(u8, u8)>,
}

impl Opts {
    /// Get the number of lines to show before the error.
    pub fn lines_before(self) -> usize {
        match self.extra_lines {
            None => 0,
            Some((before, _)) => usize::from(before),
        }
    }

    /// Get the number of lines to show after the error.
    pub fn lines_after(self) -> usize {
        match self.extra_lines {
            None => 0,
            Some((_, after)) => usize::from(after),
        }
    }
}


/// Find the byte position of the start of a specific line number (0-indexed)
fn find_line_start_by_row(code: &[u8], target_row: usize) -> usize {
    if target_row == 0 {
        return 0;
    }

    let mut current_row = 0;
    for (idx, &byte) in code.iter().enumerate() {
        if byte == b'\n' {
            current_row += 1;
            if current_row == target_row {
                return idx + 1;
            }
        }
    }

    // If we didn't find enough newlines, return the start of the code
    0
}

/// Find the total number of lines in the code
fn count_lines(code: &[u8]) -> usize {
    if code.is_empty() {
        return 0;
    }
    code.iter().filter(|&&b| b == b'\n').count() + 1
}

/// Find context lines before the error line
fn find_context_lines_before(code: &[u8], start_row: usize, count: usize) -> Vec<(usize, usize)> {
    if start_row == 0 || count == 0 {
        return Vec::new();
    }

    let mut context_lines = Vec::new();
    let start_search = start_row.saturating_sub(count);

    for row in start_search..start_row {
        let line_start = find_line_start_by_row(code, row);
        context_lines.push((row, line_start));
    }

    context_lines
}

/// Find context lines after the error line (including empty lines)
fn find_context_lines_after(code: &[u8], end_row: usize, count: usize) -> Vec<(usize, usize)> {
    let total_lines = count_lines(code);
    if end_row + 1 >= total_lines || count == 0 {
        return Vec::new();
    }

    let mut context_lines = Vec::new();
    let end_search = (end_row + 1 + count).min(total_lines);

    for row in (end_row + 1)..end_search {
        let line_start = find_line_start_by_row(code, row);
        context_lines.push((row, line_start));
    }

    context_lines
}



/// Report a lint match in terminal style.
///
/// - `match` is the match to create a report for
/// - `code` is the source code in question, as passed to
///   [`lint`][crate::lint()]
/// - `path` should be the path to the file to which `code` corresponds
///   and is used to enhance the generated report
/// - `writer` is a reference to a [`io::Write`] to which to write the
///   report
///
/// # Example
/// ```text
/// warning: [probe-read] bpf_probe_read() is deprecated and replaced by
///          bpf_probe_user() and bpf_probe_kernel(); refer to bpf-helpers(7)
///   --> example.bpf.c:43:24
///    |
/// 43 |                         bpf_probe_read(event.comm, TASK_COMM_LEN, prev->comm);
///    |                         ^^^^^^^^^^^^^^
///    |
/// ```
pub fn report_terminal(
    r#match: &LintMatch,
    code: &[u8],
    path: &Path,
    writer: &mut dyn io::Write,
) -> Result<()> {
    report_terminal_opts(r#match, code, path, writer, Opts::default())
}


/// Report a lint match in terminal style with extra lines for context as configured.
///
/// - `match` is the match to create a report for
/// - `code` is the source code in question, as passed to
///   [`lint`][crate::lint()]
/// - `path` should be the path to the file to which `code` corresponds
///   and is used to enhance the generated report
/// - `writer` is a reference to a [`io::Write`] to which to write the
///   report
/// - `opts` specifies the reporting options including context lines
///
/// # Example
/// ```text
/// warning: [probe-read] bpf_probe_read() is deprecated and replaced by
///          bpf_probe_user() and bpf_probe_kernel(); refer to bpf-helpers(7)
///   --> example.bpf.c:43:24
///    |
/// 41 |     struct task_struct *prev = (struct task_struct *)ctx[1];
/// 42 |     struct event event = {0};
/// 43 |     bpf_probe_read(event.comm, TASK_COMM_LEN, prev->comm);
///    |     ^^^^^^^^^^^^^^
/// 44 |     return 0;
/// 45 | }
///    |
/// ```
pub fn report_terminal_opts(
    r#match: &LintMatch,
    code: &[u8],
    path: &Path,
    writer: &mut dyn io::Write,
    opts: Opts,
) -> Result<()> {
    let LintMatch {
        lint_name,
        message,
        range,
    } = r#match;

    writeln!(writer, "warning: [{lint_name}] {message}")?;
    let start_row = range.start_point.row;
    let end_row = range.end_point.row;
    let start_col = range.start_point.col;
    let end_col = range.end_point.col;
    writeln!(writer, "  --> {}:{start_row}:{start_col}", path.display())?;

    if range.bytes.is_empty() {
        return Ok(())
    }

    // Find context lines
    let context_lines_before = find_context_lines_before(code, start_row, opts.lines_before());
    let context_lines_after = find_context_lines_after(code, end_row, opts.lines_after());

    // Calculate the maximum row number for consistent indentation
    let max_row = context_lines_after
        .last()
        .map(|(row, _)| *row)
        .unwrap_or(end_row);
    let prefix = format!("{:width$} | ", "", width = max_row.to_string().len());
    writeln!(writer, "{prefix}")?;

    if start_row == end_row {
        // Single line error with context

        // Show context lines before (if any)
        for (context_row, context_byte) in context_lines_before {
            let mut lines = Lines::new(code, context_byte);
            if let Some(line) = lines.next() {
                let lprefix = format!("{context_row} | ");
                writeln!(writer, "{lprefix}{}", String::from_utf8_lossy(line))?;
            }
        }

        // Show the error line
        let mut lines = Lines::new(code, range.bytes.start);
        if let Some(line) = lines.next() {
            let lprefix = format!("{start_row} | ");
            writeln!(writer, "{lprefix}{}", String::from_utf8_lossy(line))?;
            writeln!(
                writer,
                "{prefix}{:indent$}{:^<width$}",
                "",
                "",
                indent = start_col,
                width = end_col.saturating_sub(start_col)
            )?;
        } else {
            // SANITY: It would be a tree-sitter bug IF the range does not
            //         map to a valid code location.
            panic!("Expected error line");
        }

        // Show context lines after (if any)
        for (context_row, context_byte) in context_lines_after {
            let mut lines = Lines::new(code, context_byte);
            if let Some(line) = lines.next() {
                let lprefix = format!("{context_row} | ");
                writeln!(writer, "{lprefix}{}", String::from_utf8_lossy(line))?;
            } else {
                // SANITY: `Lines` will always report at least a single
                //          line.
                panic!("Expected context line after error");
            }
        }
    } else {
        // Multi-line error with context

        // Show context lines before (if any)
        for (context_row, context_byte) in context_lines_before {
            let mut lines = Lines::new(code, context_byte);
            if let Some(line) = lines.next() {
                let lprefix = format!("{context_row} | ");
                writeln!(writer, "{lprefix}{}", String::from_utf8_lossy(line))?;
            }
        }

        // Show the error lines
        let mut lines = Lines::new(code, range.bytes.start);
        for (idx, row) in (start_row..=end_row).enumerate() {
            let lprefix = format!("{row} | ");
            let c = if idx == 0 { "/" } else { "|" };
            if let Some(line) = lines.next() {
                writeln!(writer, "{lprefix} {c} {}", String::from_utf8_lossy(line))?;
            }
        }
        writeln!(writer, "{prefix} |{:_<width$}^", "", width = end_col)?;

        // Show context lines after (if any)
        for (context_row, context_byte) in context_lines_after {
            let mut lines = Lines::new(code, context_byte);
            if let Some(line) = lines.next() {
                let lprefix = format!("{context_row} | ");
                writeln!(writer, "{lprefix}{}", String::from_utf8_lossy(line))?;
            }
        }
    }

    writeln!(writer, "{prefix}")?;
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;

    use indoc::indoc;

    use pretty_assertions::assert_eq;

    use crate::Point;
    use crate::Range;


    /// Tests that a match with an empty range includes no code snippet.
    #[test]
    fn empty_range_reporting() {
        let code = indoc! { r#"
          int main() {}
        "# };

        let m = LintMatch {
            lint_name: "bogus-file-extension".to_string(),
            message: "by convention BPF C code should use the file extension '.bpf.c'".to_string(),
            range: Range {
                bytes: 0..0,
                start_point: Point::default(),
                end_point: Point::default(),
            },
        };
        let mut report = Vec::new();
        let () =
            report_terminal(&m, code.as_bytes(), Path::new("./no_bytes.c"), &mut report).unwrap();
        let report = String::from_utf8(report).unwrap();
        let expected = indoc! { r#"
          warning: [bogus-file-extension] by convention BPF C code should use the file extension '.bpf.c'
            --> ./no_bytes.c:0:0
        "# };
        assert_eq!(report, expected);
    }

    /// Make sure that multi-line matches are reported correctly.
    #[test]
    fn multi_line_report() {
        let code = indoc! { r#"
          SEC("tp_btf/sched_switch")
          int handle__sched_switch(u64 *ctx) {
              bpf_probe_read(
                event.comm,
                TASK_COMM_LEN,
                prev->comm);
              return 0;
          }
        "# };

        let m = LintMatch {
            lint_name: "probe-read".to_string(),
            message: "bpf_probe_read() is deprecated".to_string(),
            range: Range {
                bytes: 68..140,
                start_point: Point { row: 2, col: 4 },
                end_point: Point { row: 5, col: 17 },
            },
        };
        let mut report = Vec::new();
        let () = report_terminal(&m, code.as_bytes(), Path::new("<stdin>"), &mut report).unwrap();
        let report = String::from_utf8(report).unwrap();

        // Build expected output programmatically to preserve trailing spaces
        let expected = format!(
            concat!(
                "warning: [probe-read] bpf_probe_read() is deprecated\n",
                "  --> <stdin>:2:4\n",
                "  | \n",
                "2 |  /     bpf_probe_read(\n",
                "3 |  |       event.comm,\n",
                "4 |  |       TASK_COMM_LEN,\n",
                "5 |  |       prev->comm);\n",
                "  |  |_________________^\n",
                "  | \n"
            )
        );
        assert_eq!(report, expected);
    }

    /// Check that our "terminal" reporting works as expected.
    #[test]
    fn terminal_reporting() {
        let code = indoc! { r#"
          SEC("tp_btf/sched_switch")
          int handle__sched_switch(u64 *ctx)
          {
              struct task_struct *prev = (struct task_struct *)ctx[1];
              struct event event = {0};
              bpf_probe_read(event.comm, TASK_COMM_LEN, prev->comm);
              return 0;
          }
        "# };

        let m = LintMatch {
            lint_name: "probe-read".to_string(),
            message: "bpf_probe_read() is deprecated".to_string(),
            range: Range {
                bytes: 160..174,
                start_point: Point { row: 5, col: 4 },
                end_point: Point { row: 5, col: 18 },
            },
        };
        let mut report = Vec::new();
        let () = report_terminal(&m, code.as_bytes(), Path::new("<stdin>"), &mut report).unwrap();
        let report = String::from_utf8(report).unwrap();

        // Build expected output programmatically to preserve trailing spaces
        let expected = format!(
            concat!(
                "warning: [probe-read] bpf_probe_read() is deprecated\n",
                "  --> <stdin>:5:4\n",
                "  | \n",
                "5 |     bpf_probe_read(event.comm, TASK_COMM_LEN, prev->comm);\n",
                "  |     ^^^^^^^^^^^^^^\n",
                "  | \n"
            )
        );
        assert_eq!(report, expected);
    }

    /// Check that reporting works properly when the match is on the
    /// very first line of input.
    #[test]
    fn report_top_most_line() {
        let code = indoc! { r#"
          SEC("kprobe/test")
          int handle__test(void)
          {
          }
        "# };

        let m = LintMatch {
            lint_name: "unstable-attach-point".to_string(),
            message: "kprobe/kretprobe/fentry/fexit are unstable".to_string(),
            range: Range {
                bytes: 4..17,
                start_point: Point { row: 0, col: 4 },
                end_point: Point { row: 0, col: 17 },
            },
        };
        let mut report = Vec::new();
        let () = report_terminal(&m, code.as_bytes(), Path::new("<stdin>"), &mut report).unwrap();
        let report = String::from_utf8(report).unwrap();

        // Build expected output programmatically to preserve trailing spaces
        let expected = format!(
            concat!(
                "warning: [unstable-attach-point] kprobe/kretprobe/fentry/fexit are unstable\n",
                "  --> <stdin>:0:4\n",
                "  | \n",
                "0 | SEC(\"kprobe/test\")\n",
                "  |     ^^^^^^^^^^^^^\n",
                "  | \n"
            )
        );
        assert_eq!(report, expected);
    }

    /// Test that `report_terminal_opts` with `Opts::default()` behaves
    /// identically to `report_terminal`.
    #[test]
    fn report_terminal_opts_none_context() {
        let code = indoc! { r#"
          SEC("tp_btf/sched_switch")
          int handle__sched_switch(u64 *ctx)
          {
              struct task_struct *prev = (struct task_struct *)ctx[1];
              struct event event = {0};
              bpf_probe_read(event.comm, TASK_COMM_LEN, prev->comm);
              return 0;
          }
        "# };

        let m = LintMatch {
            lint_name: "probe-read".to_string(),
            message: "bpf_probe_read() is deprecated".to_string(),
            range: Range {
                bytes: 160..174,
                start_point: Point { row: 5, col: 4 },
                end_point: Point { row: 5, col: 18 },
            },
        };

        let mut report_old = Vec::new();
        let mut report_new = Vec::new();

        let () = report_terminal(&m, code.as_bytes(), Path::new("<stdin>"), &mut report_old).unwrap();
        let () = report_terminal_opts(&m, code.as_bytes(), Path::new("<stdin>"), &mut report_new, Opts::default()).unwrap();

        assert_eq!(report_old, report_new);
    }

    /// Test `report_terminal_opts` with extra context lines.
    #[test]
    fn report_terminal_opts_with_context() {
        let code = indoc! { r#"
          SEC("tp_btf/sched_switch")
          int handle__sched_switch(u64 *ctx)
          {
              struct task_struct *prev = (struct task_struct *)ctx[1];
              struct event event = {0};
              bpf_probe_read(event.comm, TASK_COMM_LEN, prev->comm);
              return 0;
          }
        "# };

        let m = LintMatch {
            lint_name: "probe-read".to_string(),
            message: "bpf_probe_read() is deprecated".to_string(),
            range: Range {
                bytes: 160..174,
                start_point: Point { row: 5, col: 4 },
                end_point: Point { row: 5, col: 18 },
            },
        };
        let mut report = Vec::new();
        let () = report_terminal_opts(&m, code.as_bytes(), Path::new("<stdin>"), &mut report, Opts { extra_lines: Some((2, 1)) }).unwrap();
        let report = String::from_utf8(report).unwrap();

        // Build expected output programmatically to preserve trailing spaces
        let expected = format!(
            concat!(
                "warning: [probe-read] bpf_probe_read() is deprecated\n",
                "  --> <stdin>:5:4\n",
                "  | \n",
                "3 |     struct task_struct *prev = (struct task_struct *)ctx[1];\n",
                "4 |     struct event event = {{0}};\n",
                "5 |     bpf_probe_read(event.comm, TASK_COMM_LEN, prev->comm);\n",
                "  |     ^^^^^^^^^^^^^^\n",
                "6 |     return 0;\n",
                "  | \n"
            )
        );
        assert_eq!(report, expected);
    }

    /// Test context lines with multi-line matches.
    #[test]
    fn report_terminal_opts_multiline_with_context() {
        let code = indoc! { r#"
          SEC("tp_btf/sched_switch")
          int handle__sched_switch(u64 *ctx) {
              bpf_probe_read(
                event.comm,
                TASK_COMM_LEN,
                prev->comm);
              return 0;
          }
        "# };

        let m = LintMatch {
            lint_name: "probe-read".to_string(),
            message: "bpf_probe_read() is deprecated".to_string(),
            range: Range {
                bytes: 68..140,
                start_point: Point { row: 2, col: 4 },
                end_point: Point { row: 5, col: 17 },
            },
        };
        let mut report = Vec::new();
        let () = report_terminal_opts(&m, code.as_bytes(), Path::new("<stdin>"), &mut report, Opts { extra_lines: Some((1, 1)) }).unwrap();
        let report = String::from_utf8(report).unwrap();

        // Build expected output programmatically to preserve trailing spaces
        let expected = format!(
            concat!(
                "warning: [probe-read] bpf_probe_read() is deprecated\n",
                "  --> <stdin>:2:4\n",
                "  | \n",
                "1 | int handle__sched_switch(u64 *ctx) {{\n",
                "2 |  /     bpf_probe_read(\n",
                "3 |  |       event.comm,\n",
                "4 |  |       TASK_COMM_LEN,\n",
                "5 |  |       prev->comm);\n",
                "  |  |_________________^\n",
                "6 |     return 0;\n",
                "  | \n"
            )
        );
        assert_eq!(report, expected);
    }

    /// Test context lines when there aren't enough lines before the error.
    #[test]
    fn report_terminal_opts_insufficient_context_before() {
        let code = indoc! { r#"
          SEC("kprobe/test")
          int handle__test(void)
          {
          }
        "# };

        let m = LintMatch {
            lint_name: "unstable-attach-point".to_string(),
            message: "kprobe/kretprobe/fentry/fexit are unstable".to_string(),
            range: Range {
                bytes: 4..17,
                start_point: Point { row: 0, col: 4 },
                end_point: Point { row: 0, col: 17 },
            },
        };
        let mut report = Vec::new();
        let () = report_terminal_opts(&m, code.as_bytes(), Path::new("<stdin>"), &mut report, Opts { extra_lines: Some((5, 2)) }).unwrap();
        let report = String::from_utf8(report).unwrap();

        // Build expected output programmatically to preserve trailing spaces
        let expected = format!(
            concat!(
                "warning: [unstable-attach-point] kprobe/kretprobe/fentry/fexit are unstable\n",
                "  --> <stdin>:0:4\n",
                "  | \n",
                "0 | SEC(\"kprobe/test\")\n",
                "  |     ^^^^^^^^^^^^^\n",
                "1 | int handle__test(void)\n",
                "2 | {{\n",
                "  | \n"
            )
        );
        assert_eq!(report, expected);
    }

    /// Test context lines when there aren't enough lines after the error.
    #[test]
    fn report_terminal_opts_insufficient_context_after() {
        let code = indoc! { r#"
          SEC("tp_btf/sched_switch")
          int handle__sched_switch(u64 *ctx)
          {
              bpf_probe_read(event.comm, TASK_COMM_LEN, prev->comm);
          }
        "# };

        let m = LintMatch {
            lint_name: "probe-read".to_string(),
            message: "bpf_probe_read() is deprecated".to_string(),
            range: Range {
                bytes: 68..82,
                start_point: Point { row: 3, col: 4 },
                end_point: Point { row: 3, col: 18 },
            },
        };
        let mut report = Vec::new();
        let () = report_terminal_opts(&m, code.as_bytes(), Path::new("<stdin>"), &mut report, Opts { extra_lines: Some((1, 5)) }).unwrap();
        let report = String::from_utf8(report).unwrap();

        // Build expected output programmatically to preserve trailing spaces
        let expected = format!(
            concat!(
                "warning: [probe-read] bpf_probe_read() is deprecated\n",
                "  --> <stdin>:3:4\n",
                "  | \n",
                "2 | {{\n",
                "3 |     bpf_probe_read(event.comm, TASK_COMM_LEN, prev->comm);\n",
                "  |     ^^^^^^^^^^^^^^\n",
                "4 | }}\n",
                "5 | \n",
                "  | \n"
            )
        );
        assert_eq!(report, expected);
    }

    /// Test Opts default and methods.
    #[test]
    fn opts_behavior() {
        let default_opts = Opts::default();
        assert_eq!(default_opts, Opts { extra_lines: None });
        assert_eq!(default_opts.lines_before(), 0);
        assert_eq!(default_opts.lines_after(), 0);

        let extra_opts = Opts { extra_lines: Some((3, 5)) };
        assert_eq!(extra_opts.lines_before(), 3);
        assert_eq!(extra_opts.lines_after(), 5);
    }

    /// Test helper functions for finding context lines.
    #[test]
    fn find_line_start_by_row() {
        let code = b"line 0\nline 1\nline 2\n";
        assert_eq!(find_line_start_by_row(code, 0), 0);
        assert_eq!(find_line_start_by_row(code, 1), 7);
        assert_eq!(find_line_start_by_row(code, 2), 14);
        assert_eq!(find_line_start_by_row(code, 10), 0); // Beyond available lines
    }

    #[test]
    fn count_lines() {
        assert_eq!(count_lines(b""), 0);
        assert_eq!(count_lines(b"single line"), 1);
        assert_eq!(count_lines(b"line 1\nline 2"), 2);
        assert_eq!(count_lines(b"line 1\nline 2\n"), 3);
    }

    #[test]
    fn find_context_lines_before() {
        let code = b"line 0\nline 1\nline 2\nline 3\n";

        // No context requested
        assert_eq!(find_context_lines_before(code, 2, 0), vec![]);

        // Context from row 0 (should return empty)
        assert_eq!(find_context_lines_before(code, 0, 2), vec![]);

        // Normal context
        let result = find_context_lines_before(code, 3, 2);
        assert_eq!(result, vec![(1, 7), (2, 14)]);

        // More context than available
        let result = find_context_lines_before(code, 2, 5);
        assert_eq!(result, vec![(0, 0), (1, 7)]);
    }

    #[test]
    fn find_context_lines_after() {
        let code = b"line 0\nline 1\nline 2\nline 3\n";

        // No context requested
        assert_eq!(find_context_lines_after(code, 1, 0), vec![]);

        // Context beyond available lines - asking for 2 lines after row 3, but only row 4 (empty) exists
        assert_eq!(find_context_lines_after(code, 3, 2), vec![(4, 28)]);

        // Normal context
        let result = find_context_lines_after(code, 0, 2);
        assert_eq!(result, vec![(1, 7), (2, 14)]);

        // More context than available - asking for 5 lines after row 2, but only rows 3 and 4 exist
        let result = find_context_lines_after(code, 2, 5);
        assert_eq!(result, vec![(3, 21), (4, 28)]);
    }
}
