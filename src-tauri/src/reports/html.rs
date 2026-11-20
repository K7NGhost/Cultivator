use super::types::{ReportCase, ReportRequest, ReportSection};
use serde_json::Value;
use std::io::Write;

pub fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

pub fn timestamp(value: &str) -> String {
    value
        .parse::<i64>()
        .ok()
        .and_then(chrono::DateTime::from_timestamp_millis)
        .map(|time| time.to_rfc3339())
        .unwrap_or_else(|| value.to_owned())
}

pub struct HtmlReport<W: Write> {
    output: W,
}

impl<W: Write> HtmlReport<W> {
    pub fn new(
        mut output: W,
        request: &ReportRequest,
        case: &ReportCase,
        generated_at: &str,
        id: &str,
    ) -> Result<Self, String> {
        write!(output, "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'\"><title>{}</title><style>{}</style></head><body><header id=\"top\"><div class=\"brand\">CULTIVATOR <span>CASE REPORT</span></div><h1>{}</h1><p>{}</p><dl><dt>Report ID</dt><dd>{}</dd><dt>Prepared by</dt><dd>{}</dd><dt>Generated</dt><dd>{}</dd></dl></header><div class=\"layout\"><nav aria-label=\"Report contents\"><h2>Contents</h2>", escape(&request.title), include_str!("report.css"), escape(&request.title), escape(&case.name), escape(id), escape(&request.examiner), escape(generated_at)).map_err(|e| e.to_string())?;
        for section in &request.sections {
            write!(
                output,
                "<a href=\"#{}\">{}</a>",
                section.id(),
                section.title()
            )
            .map_err(|e| e.to_string())?;
        }
        write!(output, "</nav><main><aside class=\"scope\"><strong>Report scope</strong><p>This report contains the selected sections and datasources from saved case data. Known timestamps are shown in UTC; unspecified timestamps and original record values are retained as recorded. Stored hashes are reported as recorded; generating this report does not verify or rehash source evidence. Original evidence files and media are referenced, not embedded.</p></aside>").map_err(|e| e.to_string())?;
        if !request.notes.trim().is_empty() {
            write!(
                output,
                "<section><h2>Examiner notes</h2><p class=\"notes\">{}</p></section>",
                escape(&request.notes)
            )
            .map_err(|e| e.to_string())?;
        }
        Ok(Self { output })
    }
    pub fn section(&mut self, section: ReportSection, columns: &[&str]) -> Result<(), String> {
        self.start_section(section)?;
        self.table(columns)
    }
    pub fn start_section(&mut self, section: ReportSection) -> Result<(), String> {
        write!(
            self.output,
            "<section id=\"{}\"><h2>{}</h2>",
            section.id(),
            section.title()
        )
        .map_err(|e| e.to_string())
    }
    pub fn table(&mut self, columns: &[&str]) -> Result<(), String> {
        self.output
            .write_all(b"<div class=\"table-wrap\"><table><thead><tr>")
            .map_err(|e| e.to_string())?;
        for column in columns {
            write!(self.output, "<th scope=\"col\">{}</th>", escape(column))
                .map_err(|e| e.to_string())?;
        }
        self.output
            .write_all(b"</tr></thead><tbody>")
            .map_err(|e| e.to_string())
    }
    pub fn row(&mut self, cells: &[String]) -> Result<(), String> {
        self.output.write_all(b"<tr>").map_err(|e| e.to_string())?;
        for cell in cells {
            write!(
                self.output,
                "<td><div class=\"value\">{}</div></td>",
                escape(cell)
            )
            .map_err(|e| e.to_string())?;
        }
        self.output.write_all(b"</tr>").map_err(|e| e.to_string())
    }
    pub fn end_section(&mut self, rows: u64, columns: usize) -> Result<(), String> {
        self.end_table(rows, columns)?;
        self.finish_section(rows)
    }
    pub fn end_table(&mut self, rows: u64, columns: usize) -> Result<(), String> {
        if rows == 0 {
            write!(self.output, "<tr><td colspan=\"{columns}\" class=\"empty\">No saved records in the selected scope.</td></tr>").map_err(|e| e.to_string())?;
        }
        self.output
            .write_all(b"</tbody></table></div>")
            .map_err(|e| e.to_string())
    }
    pub fn finish_section(&mut self, rows: u64) -> Result<(), String> {
        write!(self.output, "<p class=\"count\">{rows} record(s) included · <a href=\"#top\">Back to top</a></p></section>").map_err(|e| e.to_string())
    }
    pub fn artifact_contents(&mut self, groups: &[(String, u64)]) -> Result<(), String> {
        if groups.is_empty() {
            return self
                .output
                .write_all(b"<p class=\"empty\">No saved records in the selected scope.</p>")
                .map_err(|e| e.to_string());
        }
        self.output
            .write_all(b"<ul class=\"artifact-index\" aria-label=\"Artifact tables\">")
            .map_err(|e| e.to_string())?;
        for (index, (name, count)) in groups.iter().enumerate() {
            write!(
                self.output,
                "<li><a href=\"#artifact-{}\">{}</a><span>{count} entries</span></li>",
                index + 1,
                escape(name)
            )
            .map_err(|e| e.to_string())?;
        }
        self.output.write_all(b"</ul>").map_err(|e| e.to_string())
    }
    pub fn artifact_heading(
        &mut self,
        index: usize,
        name: &str,
        datasource: &str,
        parser: &str,
        path: &str,
        rows: u64,
    ) -> Result<(), String> {
        write!(self.output, "<article class=\"artifact-group\" id=\"artifact-{index}\"><div class=\"artifact-heading\"><h3>{}</h3><span>{rows} entries</span></div><dl class=\"artifact-meta\"><dt>Datasource</dt><dd>{}</dd><dt>Parser</dt><dd>{}</dd><dt>Source</dt><dd>{}</dd></dl>", escape(name), escape(datasource), escape(parser), escape(path)).map_err(|e| e.to_string())
    }
    pub fn end_artifact(&mut self) -> Result<(), String> {
        self.output
            .write_all(b"</article>")
            .map_err(|e| e.to_string())
    }
    pub fn finish(mut self) -> Result<W, String> {
        self.output.write_all(b"</main></div><footer>Generated by Cultivator. This report is a snapshot of saved case data, not a certification of source authenticity.</footer></body></html>").map_err(|e| e.to_string())?;
        self.output.flush().map_err(|e| e.to_string())?;
        Ok(self.output)
    }
}

pub fn record_text(value: &str) -> String {
    serde_json::from_str::<Value>(value)
        .ok()
        .and_then(|value| serde_json::to_string_pretty(&value).ok())
        .unwrap_or_else(|| value.to_owned())
}
