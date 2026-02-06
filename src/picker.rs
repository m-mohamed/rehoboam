//! Fuzzy project picker for interactive `rehoboam init`
//!
//! Uses nucleo-picker (built-in) for a fast, Unicode-aware fuzzy picker.
//! Falls back to a numbered list when compiled without the `builtin-picker` feature.

use crate::init::ProjectInfo;
use std::path::PathBuf;

/// Pick projects interactively using the best available method.
///
/// With `builtin-picker` feature (default): launches nucleo-picker with multi-select.
/// Without: falls back to a numbered list selection.
pub fn pick_projects(projects: &[ProjectInfo]) -> Vec<PathBuf> {
    if projects.is_empty() {
        return Vec::new();
    }

    #[cfg(feature = "builtin-picker")]
    {
        pick_with_nucleo(projects)
    }

    #[cfg(not(feature = "builtin-picker"))]
    {
        pick_with_numbered_list(projects)
    }
}

/// Nucleo-picker based fuzzy multi-select
#[cfg(feature = "builtin-picker")]
fn pick_with_nucleo(projects: &[ProjectInfo]) -> Vec<PathBuf> {
    use nucleo_picker::render::StrRenderer;
    use nucleo_picker::Picker;

    // Build display lines with embedded index for mapping back
    // Format: "idx\tcheck name (branch)  ~/path"
    // StrRenderer matches on the full string; we parse the index prefix after selection.
    let items: Vec<String> = projects
        .iter()
        .enumerate()
        .map(|(i, p)| format!("{}\t{}", i, p.picker_line()))
        .collect();

    let mut picker = Picker::new(StrRenderer);

    // Inject all items
    let injector = picker.injector();
    for item in items {
        injector.push(item);
    }

    // Run multi-select picker (Tab to select, Enter to confirm)
    match picker.pick_multi() {
        Ok(selection) => selection
            .iter()
            .filter_map(|line| {
                // Parse index from "idx\t..." prefix
                line.split('\t')
                    .next()
                    .and_then(|idx_str| idx_str.parse::<usize>().ok())
                    .and_then(|idx| projects.get(idx))
                    .map(|p| p.path.clone())
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// Numbered list fallback (no fuzzy matching)
#[cfg(not(feature = "builtin-picker"))]
fn pick_with_numbered_list(projects: &[ProjectInfo]) -> Vec<PathBuf> {
    use std::io::{self, BufRead, Write};

    println!(
        "Select projects to initialize (enter numbers separated by spaces, or 'all'):\n"
    );

    for (i, project) in projects.iter().enumerate() {
        let status = if project.has_hooks {
            " [already initialized]"
        } else {
            ""
        };
        println!("  [{}] {}{}", i + 1, project.name, status);
    }

    print!("\nSelection: ");
    let _ = io::stdout().flush();

    let mut input = String::new();
    if io::stdin().lock().read_line(&mut input).is_err() {
        return Vec::new();
    }

    let input = input.trim().to_lowercase();

    if input == "all" {
        return projects.iter().map(|p| p.path.clone()).collect();
    }

    let mut selected = Vec::new();
    for part in input.split_whitespace() {
        if let Ok(num) = part.parse::<usize>() {
            if num > 0 && num <= projects.len() {
                selected.push(projects[num - 1].path.clone());
            }
        }
    }

    selected
}
