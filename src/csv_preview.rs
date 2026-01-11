use anyhow::{Context, Result};
use csv::ReaderBuilder;
use std::path::Path;

const SORT_SCRIPT: &str = r#"<script>
(function () {
  if (window.__rendarCsvSort) {
    return;
  }
  window.__rendarCsvSort = true;

  function getCellValue(row, index) {
    var cell = row.children[index];
    return cell ? cell.textContent.trim() : "";
  }

  function isNumeric(value) {
    if (value === "") {
      return false;
    }
    var number = Number(value);
    return !Number.isNaN(number);
  }

  function setupTable(table) {
    var tbody = table.tBodies[0];
    if (!tbody) {
      return;
    }
    var headers = table.tHead ? table.tHead.rows[0].cells : table.rows[0].cells;
    Array.prototype.forEach.call(headers, function (th, index) {
      th.setAttribute("role", "button");
      th.tabIndex = 0;
      function sort() {
        var rows = Array.prototype.slice.call(tbody.rows);
        var values = rows.map(function (row) {
          return getCellValue(row, index);
        });
        var numeric = values.filter(function (value) { return value !== ""; }).every(isNumeric);
        var current = th.getAttribute("data-sort");
        var next = current === "asc" ? "desc" : "asc";
        Array.prototype.forEach.call(headers, function (header) {
          header.removeAttribute("data-sort");
          header.removeAttribute("aria-sort");
        });
        th.setAttribute("data-sort", next);
        th.setAttribute("aria-sort", next === "asc" ? "ascending" : "descending");
        rows.sort(function (a, b) {
          var aValue = getCellValue(a, index);
          var bValue = getCellValue(b, index);
          if (numeric && isNumeric(aValue) && isNumeric(bValue)) {
            var diff = Number(aValue) - Number(bValue);
            return next === "asc" ? diff : -diff;
          }
          var order = aValue.localeCompare(bValue);
          return next === "asc" ? order : -order;
        });
        rows.forEach(function (row) {
          tbody.appendChild(row);
        });
      }
      th.addEventListener("click", sort);
      th.addEventListener("keydown", function (event) {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          sort();
        }
      });
    });
  }

  function init() {
    var tables = document.querySelectorAll("table.csv-table");
    Array.prototype.forEach.call(tables, setupTable);
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
})();
</script>
"#;

pub fn render_csv_file(path: &Path, max_rows: Option<usize>) -> Result<String> {
    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read CSV file {}", path.display()))?;
    let delimiter = detect_delimiter(&contents);

    let mut reader = ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(false)
        .from_reader(contents.as_bytes());

    let read_cap = max_rows.map(|limit| limit.saturating_add(2));
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut truncated = false;

    for result in reader.records() {
        let record = result.context("Failed to parse CSV record")?;
        if let Some(limit) = read_cap {
            if rows.len() >= limit {
                truncated = true;
                break;
            }
        }
        rows.push(record.iter().map(|cell| cell.to_string()).collect());
    }

    if rows.is_empty() {
        return Ok(r#"<div class="csv-preview"><div class="csv-empty">Empty CSV.</div></div>"#
            .to_string());
    }

    let header_row = if rows.len() >= 2 && is_header_row(&rows[0], &rows[1]) {
        Some(rows[0].clone())
    } else {
        None
    };

    let data_start = if header_row.is_some() { 1 } else { 0 };
    let mut data_rows: Vec<Vec<String>> = rows.into_iter().skip(data_start).collect();

    let mut data_truncated = false;
    if let Some(limit) = max_rows {
        if data_rows.len() > limit {
            data_rows.truncate(limit);
            data_truncated = true;
        }
    }
    if truncated {
        data_truncated = true;
    }

    let mut max_cols = header_row.as_ref().map(|row| row.len()).unwrap_or(0);
    for row in &data_rows {
        max_cols = max_cols.max(row.len());
    }
    if max_cols == 0 {
        max_cols = 1;
    }

    let header = header_row.unwrap_or_else(|| {
        (1..=max_cols)
            .map(|idx| format!("Column {}", idx))
            .collect()
    });

    let mut html = String::new();
    html.push_str(r#"<div class="csv-preview">"#);
    if data_truncated {
        html.push_str(&format!(
            r#"<div class="csv-notice">Showing first {} rows.</div>"#,
            data_rows.len()
        ));
    }
    html.push_str(r#"<div class="csv-table-wrap">"#);
    html.push_str(r#"<table class="csv-table">"#);
    html.push_str("<thead><tr>");
    for idx in 0..max_cols {
        let label = header.get(idx).cloned().unwrap_or_default();
        html.push_str(&format!(
            r#"<th scope="col">{}</th>"#,
            html_escape(&label)
        ));
    }
    html.push_str("</tr></thead><tbody>");
    for row in data_rows {
        html.push_str("<tr>");
        for idx in 0..max_cols {
            let value = row.get(idx).cloned().unwrap_or_default();
            html.push_str(&format!(r#"<td>{}</td>"#, html_escape(&value)));
        }
        html.push_str("</tr>");
    }
    html.push_str("</tbody></table></div>");
    html.push_str("</div>");
    html.push_str(SORT_SCRIPT);

    Ok(html)
}

fn detect_delimiter(sample: &str) -> u8 {
    let candidates = [b',', b';', b'\t', b'|'];
    let mut best = b',';
    let mut best_score = i32::MIN;
    for &delim in &candidates {
        let score = delimiter_score(sample.as_bytes(), delim);
        if score > best_score {
            best_score = score;
            best = delim;
        }
    }
    best
}

fn delimiter_score(bytes: &[u8], delimiter: u8) -> i32 {
    let mut counts = Vec::new();
    let mut current = 0usize;
    let mut in_quotes = false;
    let mut idx = 0usize;
    while idx < bytes.len() {
        let byte = bytes[idx];
        if byte == b'"' {
            if in_quotes && idx + 1 < bytes.len() && bytes[idx + 1] == b'"' {
                idx += 1;
            } else {
                in_quotes = !in_quotes;
            }
        } else if !in_quotes && byte == delimiter {
            current += 1;
        } else if byte == b'\n' {
            counts.push(current);
            current = 0;
        }
        idx += 1;
    }
    if current > 0 || !counts.is_empty() {
        counts.push(current);
    }

    if counts.is_empty() {
        return 0;
    }
    if counts.iter().all(|count| *count == 0) {
        return 0;
    }

    let sum: usize = counts.iter().sum();
    let mean = sum as f32 / counts.len() as f32;
    let max = *counts.iter().max().unwrap_or(&0);
    let min = *counts.iter().min().unwrap_or(&0);
    let zero_lines = counts.iter().filter(|count| **count == 0).count();
    (mean * 100.0) as i32 - ((max - min) as i32 * 10) - (zero_lines as i32 * 25)
}

fn is_header_row(first: &[String], second: &[String]) -> bool {
    if first.is_empty() || second.is_empty() {
        return false;
    }
    let cols = first.len().max(second.len());
    let first_numeric = first.iter().filter(|cell| is_numeric(cell)).count();
    let second_numeric = second.iter().filter(|cell| is_numeric(cell)).count();
    let first_text = first
        .iter()
        .filter(|cell| !cell.trim().is_empty() && !is_numeric(cell))
        .count();
    let second_text = second
        .iter()
        .filter(|cell| !cell.trim().is_empty() && !is_numeric(cell))
        .count();

    let strong_header = first_text >= (cols + 1) / 2 && first_numeric < second_numeric;
    let text_heavier = first_text > second_text && first_numeric <= second_numeric;
    strong_header || text_heavier
}

fn is_numeric(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return false;
    }
    trimmed.parse::<f64>().is_ok()
}

fn html_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_comma_delimiter() {
        let sample = "name,age\nAda,36\n";
        assert_eq!(detect_delimiter(sample), b',');
    }

    #[test]
    fn detects_tab_delimiter() {
        let sample = "name\tage\nAda\t36\n";
        assert_eq!(detect_delimiter(sample), b'\t');
    }

    #[test]
    fn detects_header_row() {
        let first = vec!["Name".to_string(), "Age".to_string()];
        let second = vec!["Ada".to_string(), "36".to_string()];
        assert!(is_header_row(&first, &second));
    }
}
