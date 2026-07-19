//! Rendering of an [`Analysis`] plus its [`Suggestion`]s as text, JSON, or a
//! self-contained HTML page.

use std::fmt::Write as _;

use serde::Serialize;

use movespan_model::Analysis;
use movespan_rules::Suggestion;

const MODEL_NOTE: &str = "Estimates from Movespan's Block-STM contention model. Exact for ranking \
hotspots and for the direction and magnitude of a fix; not validator-exact throughput. Aggregators \
and Aptos framework state (gas payment, sequence numbers, fungible stores) are excluded: the VM \
special-cases them and a contract author cannot refactor them.";

#[derive(Serialize)]
struct ReportView<'a> {
    analysis: &'a Analysis,
    suggestions: &'a [Suggestion],
    note: &'a str,
}

/// Machine-readable report, suitable for CI or further tooling.
pub fn to_json(analysis: &Analysis, suggestions: &[Suggestion]) -> String {
    let view = ReportView {
        analysis,
        suggestions,
        note: MODEL_NOTE,
    };
    serde_json::to_string_pretty(&view).expect("report is serializable")
}

/// Human-readable terminal report.
pub fn to_text(analysis: &Analysis, suggestions: &[Suggestion]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "MOVESPAN CONTENTION REPORT");
    let _ = writeln!(out, "==========================");
    let _ = writeln!(out);
    let _ = writeln!(out, "Transactions       : {}", analysis.txn_count);
    let _ = writeln!(out, "Threads modeled    : {}", analysis.threads);
    let _ = writeln!(
        out,
        "Parallelizability  : {:.1}%",
        analysis.parallelizability * 100.0
    );
    let _ = writeln!(
        out,
        "Estimated speedup  : {:.2}x of {}x ideal",
        analysis.estimated_speedup, analysis.threads
    );
    let _ = writeln!(
        out,
        "Work / span        : {} / {}",
        analysis.total_work, analysis.span
    );
    let _ = writeln!(out, "Conflict edges     : {}", analysis.conflict_edges);
    let _ = writeln!(out);

    let _ = writeln!(out, "TOP CONTENTION HOTSPOTS");
    let _ = writeln!(out, "-----------------------");
    if analysis.hotspots.is_empty() {
        let _ = writeln!(out, "None — the workload is already contention-free.");
    } else {
        for (i, hotspot) in analysis.hotspots.iter().take(10).enumerate() {
            let _ = writeln!(
                out,
                "{:>2}. {}  ({} deps, blocked cost {})",
                i + 1,
                hotspot.label,
                hotspot.edges_caused,
                hotspot.blocked_cost
            );
        }
    }
    let _ = writeln!(out);

    if !suggestions.is_empty() {
        let _ = writeln!(out, "RECOMMENDATIONS");
        let _ = writeln!(out, "---------------");
        for (i, suggestion) in suggestions.iter().enumerate() {
            let _ = writeln!(out);
            let _ = writeln!(out, "[{}] {}", i + 1, suggestion.target);
            let _ = writeln!(out, "    problem: {}", suggestion.problem);
            let _ = writeln!(out, "    fix    : {}", suggestion.fix);
            let _ = writeln!(
                out,
                "    impact : {:.2}x -> {:.2}x  (+{:.2}x, {:.0}% parallel)",
                analysis.estimated_speedup,
                suggestion.projected_speedup,
                suggestion.speedup_gain,
                suggestion.projected_parallelizability * 100.0
            );
        }
        let _ = writeln!(out);
    }

    let _ = writeln!(out, "{MODEL_NOTE}");
    out
}

/// Self-contained HTML report with no external assets.
pub fn to_html(analysis: &Analysis, suggestions: &[Suggestion]) -> String {
    let hotspots = if analysis.hotspots.is_empty() {
        "<tr><td colspan=\"3\">None — the workload is already contention-free.</td></tr>"
            .to_string()
    } else {
        analysis
            .hotspots
            .iter()
            .take(10)
            .map(|h| {
                format!(
                    "<tr><td>{}</td><td>{}</td><td>{}</td></tr>",
                    escape(&h.label),
                    h.edges_caused,
                    h.blocked_cost
                )
            })
            .collect()
    };

    let recommendations: String = suggestions
        .iter()
        .map(|s| {
            format!(
                "<div class=\"rec\"><h3>{}</h3><p><b>Problem:</b> {}</p><p><b>Fix:</b> {}</p>\
                 <p><b>Impact:</b> {:.2}x &rarr; {:.2}x (+{:.2}x)</p></div>",
                escape(&s.target),
                escape(&s.problem),
                escape(&s.fix),
                analysis.estimated_speedup,
                s.projected_speedup,
                s.speedup_gain
            )
        })
        .collect();

    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
<title>Movespan Contention Report</title>\
<style>body{{font:15px/1.5 system-ui,sans-serif;max-width:820px;margin:2rem auto;padding:0 1rem;color:#111}}\
h1{{margin-bottom:.25rem}}table{{border-collapse:collapse;width:100%;margin:1rem 0}}\
td,th{{border:1px solid #ddd;padding:.5rem;text-align:left}}th{{background:#f5f5f5}}\
.rec{{border-left:3px solid #333;padding:.25rem 1rem;margin:1rem 0;background:#fafafa}}\
.note{{color:#666;font-size:.85rem;margin-top:2rem}}</style></head><body>\
<h1>Movespan Contention Report</h1>\
<p>Parallelizability <b>{:.1}%</b> &middot; estimated speedup <b>{:.2}x</b> of {}x ideal \
&middot; {} transactions &middot; {} conflict edges</p>\
<h2>Hotspots</h2><table><tr><th>Location</th><th>Deps</th><th>Blocked cost</th></tr>{}</table>\
<h2>Recommendations</h2>{}\
<p class=\"note\">{}</p></body></html>",
        analysis.parallelizability * 100.0,
        analysis.estimated_speedup,
        analysis.threads,
        analysis.txn_count,
        analysis.conflict_edges,
        hotspots,
        recommendations,
        escape(MODEL_NOTE),
    )
}

fn escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}
