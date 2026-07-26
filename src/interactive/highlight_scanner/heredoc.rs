//! Here-document body scanner.
//!
//! Entered by scan_normal at the newline following a line with pending
//! `<<`/`<<-` redirections. Consumes whole body lines (so incremental
//! checkpoints only land on line boundaries), painting them as String
//! rather than running them through the command checker; leaves the mode
//! when the last pending delimiter line is seen.

use super::super::command_checker::CheckerEnv;
use super::super::highlight::{ColorSpan, HighlightStyle};
use super::ctx::ScanCtx;

pub(super) fn scan_heredoc(ctx: &mut ScanCtx<'_>, _env: &CheckerEnv<'_>, pos: usize) -> usize {
    let (delim, strip_tabs) = match ctx.state.pending_heredocs.first() {
        Some((d, s)) => (d.clone(), *s),
        // Defensive: no pending delimiter means the mode is stale.
        None => {
            ctx.state.pop_mode();
            return pos;
        }
    };

    let line_end = ctx.input[pos..]
        .iter()
        .position(|&c| c == '\n')
        .map(|i| pos + i)
        .unwrap_or(ctx.input.len());

    // `<<-` strips leading tabs before comparing against the delimiter.
    let mut content_start = pos;
    if strip_tabs {
        while content_start < line_end && ctx.input[content_start] == '\t' {
            content_start += 1;
        }
    }
    let is_terminator = ctx.input[content_start..line_end]
        .iter()
        .collect::<String>()
        == delim;

    if is_terminator {
        ctx.spans.push(ColorSpan {
            start: pos,
            end: line_end,
            style: HighlightStyle::String,
        });
        ctx.state.pending_heredocs.remove(0);
        if ctx.state.pending_heredocs.is_empty() {
            // Hand the trailing newline back to scan_normal so it restores
            // command position for the next line.
            ctx.state.pop_mode();
            return line_end;
        }
        // More heredocs on the same command line: the next body starts on
        // the following line.
        return (line_end + 1).min(ctx.input.len());
    }

    if line_end > pos {
        ctx.spans.push(ColorSpan {
            start: pos,
            end: line_end,
            style: HighlightStyle::String,
        });
    }
    if line_end < ctx.input.len() {
        line_end + 1
    } else {
        ctx.input.len()
    }
}
