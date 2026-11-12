use std::collections::HashSet;
use std::io::{self, Stdout};
use std::panic;

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Clear, Paragraph, Tabs, Wrap,
    },
    Frame, Terminal,
};
use tui_tree_widget::{Tree, TreeItem, TreeState};

use crate::common::readable_canonical_path;
use crate::executable::{DllName, Executable, Executables};
use crate::pe::demangle_symbol;
use crate::skim::skim_symbols;

/// Details panel tabs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DetailTab {
    Overview = 0,
    ExportedSymbols = 1,
    ImportedSymbols = 2,
    Diagnostics = 3,
}

impl DetailTab {
    fn titles() -> Vec<&'static str> {
        vec![
            "[1] Overview",
            "[2] Exports",
            "[3] Imports",
            "[4] Diagnostics",
        ]
    }

    #[allow(dead_code)]
    fn from_index(index: usize) -> Self {
        match index {
            0 => DetailTab::Overview,
            1 => DetailTab::ExportedSymbols,
            2 => DetailTab::ImportedSymbols,
            3 => DetailTab::Diagnostics,
            _ => DetailTab::Overview,
        }
    }

    fn next(self) -> Self {
        match self {
            DetailTab::Overview => DetailTab::ExportedSymbols,
            DetailTab::ExportedSymbols => DetailTab::ImportedSymbols,
            DetailTab::ImportedSymbols => DetailTab::Diagnostics,
            DetailTab::Diagnostics => DetailTab::Overview,
        }
    }

    fn prev(self) -> Self {
        match self {
            DetailTab::Overview => DetailTab::Diagnostics,
            DetailTab::ExportedSymbols => DetailTab::Overview,
            DetailTab::ImportedSymbols => DetailTab::ExportedSymbols,
            DetailTab::Diagnostics => DetailTab::ImportedSymbols,
        }
    }
}

/// Active focused pane
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActivePane {
    Tree,
    Details,
}

/// TUI Application state
struct App<'a> {
    executables: &'a Executables,
    print_system_dlls: bool,
    tree_state: TreeState<String>,
    active_pane: ActivePane,
    selected_tab: DetailTab,
    details_scroll: usize,

    // Search mode state
    search_active: bool,
    search_query: String,
    details_search_query: String,
    matcher: SkimMatcherV2,
    match_count: usize,
    details_match_count: usize,
    tree_matching_paths: Vec<Vec<String>>,
    details_match_lines: Vec<usize>,

    // Help popup
    show_help: bool,

    // Cached tree items and diagnostics for high-performance rendering
    cached_tree_items: Vec<TreeItem<'static, String>>,
    tree_dirty: bool,
    cached_details_lines: Vec<Line<'static>>,
    cached_details_key: Option<(Option<String>, DetailTab, String)>,
    check_report: Option<crate::executable::ExecutablesCheckReport>,
}

impl<'a> App<'a> {
    fn new(executables: &'a Executables, print_system_dlls: bool) -> Self {
        let mut tree_state = TreeState::default();

        // Default open the root node if available
        if let Ok(Some(root)) = executables.get_root() {
            tree_state.open(vec![root.dllname.clone()]);
            tree_state.select(vec![root.dllname.clone()]);
        } else if let Some(first) = executables.sorted_by_first_appearance().first() {
            tree_state.open(vec![first.dllname.clone()]);
            tree_state.select(vec![first.dllname.clone()]);
        }

        let check_report = executables.check(true).ok();

        let mut app = Self {
            executables,
            print_system_dlls,
            tree_state,
            active_pane: ActivePane::Tree,
            selected_tab: DetailTab::Overview,
            details_scroll: 0,
            search_active: false,
            search_query: String::new(),
            details_search_query: String::new(),
            matcher: SkimMatcherV2::default(),
            match_count: 0,
            details_match_count: 0,
            tree_matching_paths: Vec::new(),
            details_match_lines: Vec::new(),
            show_help: false,
            cached_tree_items: Vec::new(),
            tree_dirty: true,
            cached_details_lines: Vec::new(),
            cached_details_key: None,
            check_report,
        };

        app.ensure_tree_updated();
        app.ensure_details_updated();
        app
    }

    fn mark_tree_dirty(&mut self) {
        self.tree_dirty = true;
    }

    fn ensure_tree_updated(&mut self) {
        if self.tree_dirty {
            self.cached_tree_items = self.build_tree_items();
            self.tree_dirty = false;
        }
    }

    fn ensure_details_updated(&mut self) {
        let key = (
            self.get_selected_dll_name(),
            self.selected_tab,
            self.details_search_query.clone(),
        );
        if self.cached_details_key.as_ref() != Some(&key) {
            let (lines, count, match_lines) = self.build_details_lines();
            self.cached_details_lines = lines;
            self.details_match_count = count;
            self.details_match_lines = match_lines;
            self.cached_details_key = Some(key);
        }
    }

    fn highlight_matches(
        text: &str,
        indices: &[usize],
        base_style: Style,
        match_style: Style,
    ) -> Vec<Span<'static>> {
        let idx_set: HashSet<usize> = indices.iter().copied().collect();
        let mut spans = Vec::new();
        let mut current_matched = false;
        let mut segment = String::new();

        for (i, ch) in text.chars().enumerate() {
            let is_m = idx_set.contains(&i);
            if is_m != current_matched && !segment.is_empty() {
                let style = if current_matched { match_style } else { base_style };
                spans.push(Span::styled(segment.clone(), style));
                segment.clear();
            }
            current_matched = is_m;
            segment.push(ch);
        }

        if !segment.is_empty() {
            let style = if current_matched { match_style } else { base_style };
            spans.push(Span::styled(segment, style));
        }

        spans
    }

    fn build_details_lines(&self) -> (Vec<Line<'static>>, usize, Vec<usize>) {
        let selected_exe = self.get_selected_executable();
        let selected_name = self.get_selected_dll_name().unwrap_or_else(|| "<No selection>".to_string());
        let query = if self.details_search_query.is_empty() {
            None
        } else {
            Some(self.details_search_query.as_str())
        };

        match self.selected_tab {
            DetailTab::Overview => self.build_overview_lines(selected_exe, &selected_name, query),
            DetailTab::ExportedSymbols => self.build_exports_lines(selected_exe, query),
            DetailTab::ImportedSymbols => self.build_imports_lines(selected_exe, query),
            DetailTab::Diagnostics => self.build_diagnostics_lines(selected_exe, &selected_name, query),
        }
    }

    fn build_overview_lines(&self, exe: Option<&Executable>, name: &str, query: Option<&str>) -> (Vec<Line<'static>>, usize, Vec<usize>) {
        let mut lines = Vec::new();
        let mut match_count = 0;
        let mut match_lines = Vec::new();

        let base_match_style = Style::default()
            .fg(Color::LightYellow)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED);

        if let Some(e) = exe {
            if let Some(q) = query {
                let mut field_lines = Vec::new();

                // Name
                let name_match = self.matcher.fuzzy_indices(&e.dllname, q);
                if let Some((_, idx)) = &name_match {
                    match_count += 1;
                    match_lines.push(2 + field_lines.len());
                    let mut spans = vec![
                        Span::styled("Name: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    ];
                    spans.extend(Self::highlight_matches(&e.dllname, idx, Style::default().add_modifier(Modifier::BOLD), base_match_style));
                    field_lines.push(Line::from(spans));
                } else {
                    field_lines.push(Line::from(vec![
                        Span::styled("Name: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                        Span::styled(e.dllname.clone(), Style::default().add_modifier(Modifier::BOLD)),
                    ]));
                }

                // Status
                let status_text = if e.found { "Found" } else { "NOT FOUND (Missing Dependency)" };
                let status_match = self.matcher.fuzzy_indices(status_text, q);
                if let Some((_, idx)) = &status_match {
                    match_count += 1;
                    match_lines.push(2 + field_lines.len());
                    let base_st = if e.found {
                        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
                    };
                    let mut spans = vec![
                        Span::styled("Status: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    ];
                    spans.extend(Self::highlight_matches(status_text, idx, base_st, base_match_style));
                    field_lines.push(Line::from(spans));
                } else {
                    let status_span = if e.found {
                        Span::styled("Found", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
                    } else {
                        Span::styled("NOT FOUND (Missing Dependency)", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
                    };
                    field_lines.push(Line::from(vec![
                        Span::styled("Status: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                        status_span,
                    ]));
                }

                if let Some(d) = &e.details {
                    let path_str = readable_canonical_path(&d.full_path)
                        .unwrap_or_else(|_| d.full_path.to_string_lossy().to_string());
                    let path_match = self.matcher.fuzzy_indices(&path_str, q);
                    if let Some((_, idx)) = &path_match {
                        match_count += 1;
                        match_lines.push(2 + field_lines.len());
                        let mut spans = vec![
                            Span::styled("Full Path: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                        ];
                        spans.extend(Self::highlight_matches(&path_str, idx, Style::default(), base_match_style));
                        field_lines.push(Line::from(spans));
                    } else {
                        field_lines.push(Line::from(vec![
                            Span::styled("Full Path: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                            Span::raw(path_str),
                        ]));
                    }

                    let kind = if d.is_known_dll {
                        "Known DLL (Windows Preloaded/Protected)"
                    } else if d.is_api_set {
                        "API Set Extension (Virtual schema DLL)"
                    } else if d.is_system {
                        "System DLL (Windows/System32 directory)"
                    } else {
                        "User / Application DLL"
                    };
                    let kind_match = self.matcher.fuzzy_indices(kind, q);
                    if let Some((_, idx)) = &kind_match {
                        match_count += 1;
                        match_lines.push(2 + field_lines.len());
                        let mut spans = vec![
                            Span::styled("Category: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                        ];
                        spans.extend(Self::highlight_matches(kind, idx, Style::default(), base_match_style));
                        field_lines.push(Line::from(spans));
                    } else {
                        field_lines.push(Line::from(vec![
                            Span::styled("Category: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                            Span::raw(kind),
                        ]));
                    }

                    if let Some(deps) = &d.dependencies {
                        let mut matched_deps = Vec::new();
                        for dep in deps {
                            let dep_match = self.matcher.fuzzy_indices(dep, q);
                            if let Some((_, idx)) = dep_match {
                                match_count += 1;
                                matched_deps.push((dep, Some(idx)));
                            } else {
                                matched_deps.push((dep, None));
                            }
                        }

                        field_lines.push(Line::from(""));
                        field_lines.push(Line::from(vec![
                            Span::styled(format!("Direct Dependencies ({}):", deps.len()), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                        ]));

                        for (dep, match_idx) in matched_deps {
                            let mut dep_spans = vec![
                                Span::raw("  • "),
                            ];
                            if let Some(idx) = match_idx {
                                match_lines.push(2 + field_lines.len());
                                dep_spans.extend(Self::highlight_matches(dep, &idx, Style::default().fg(Color::White), base_match_style));
                            } else {
                                dep_spans.push(Span::raw(dep.clone()));
                            }
                            field_lines.push(Line::from(dep_spans));
                        }
                    }
                }

                lines.push(Line::from(vec![
                    Span::styled(format!("Overview (Filter: '{}' ─ {} matches):", q, match_count), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                ]));
                lines.push(Line::from(""));
                lines.extend(field_lines);
            } else {
                lines.push(Line::from(vec![
                    Span::styled("Name: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    Span::styled(e.dllname.clone(), Style::default().add_modifier(Modifier::BOLD)),
                ]));

                let status_span = if e.found {
                    Span::styled("Found", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
                } else {
                    Span::styled("NOT FOUND (Missing Dependency)", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
                };
                lines.push(Line::from(vec![
                    Span::styled("Status: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    status_span,
                ]));

                if let Some(d) = &e.details {
                    let path_str = readable_canonical_path(&d.full_path)
                        .unwrap_or_else(|_| d.full_path.to_string_lossy().to_string());
                    lines.push(Line::from(vec![
                        Span::styled("Full Path: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                        Span::raw(path_str),
                    ]));

                    let kind = if d.is_known_dll {
                        "Known DLL (Windows Preloaded/Protected)"
                    } else if d.is_api_set {
                        "API Set Extension (Virtual schema DLL)"
                    } else if d.is_system {
                        "System DLL (Windows/System32 directory)"
                    } else {
                        "User / Application DLL"
                    };
                    lines.push(Line::from(vec![
                        Span::styled("Category: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                        Span::raw(kind),
                    ]));

                    if let Some(deps) = &d.dependencies {
                        lines.push(Line::from(""));
                        lines.push(Line::from(vec![
                            Span::styled(format!("Direct Dependencies ({}):", deps.len()), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                        ]));
                        for dep in deps {
                            lines.push(Line::from(vec![
                                Span::raw("  • "),
                                Span::raw(dep.clone()),
                            ]));
                        }
                    }

                    if let Some(syms) = &d.symbols {
                        lines.push(Line::from(""));
                        lines.push(Line::from(vec![
                            Span::styled("Symbols Summary:", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                        ]));
                        lines.push(Line::from(format!("  • Exported functions: {}", syms.exported.len())));
                        let total_imports: usize = syms.imported.values().map(|v| v.len()).sum();
                        lines.push(Line::from(format!("  • Imported functions: {} (across {} DLLs)", total_imports, syms.imported.len())));
                    }
                } else if !e.found {
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled("This DLL could not be resolved in the search paths.", Style::default().fg(Color::Red))));
                }
            }
        } else {
            lines.push(Line::from(format!("No information available for {}", name)));
        }

        (lines, match_count, match_lines)
    }

    fn build_exports_lines(&self, exe: Option<&Executable>, query: Option<&str>) -> (Vec<Line<'static>>, usize, Vec<usize>) {
        let mut lines = Vec::new();
        let mut match_count = 0;
        let mut match_lines = Vec::new();

        let base_match_style = Style::default()
            .fg(Color::LightYellow)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED);

        if let Some(e) = exe {
            if let Some(d) = &e.details {
                if let Some(syms) = &d.symbols {
                    let mut exported: Vec<_> = syms.exported.iter().collect();
                    exported.sort();

                    let total = exported.len();

                    if let Some(q) = query {
                        let mut matched_items = Vec::new();
                        for sym in &exported {
                            let demangled_res = demangle_symbol(sym);
                            let demangled = demangled_res.as_deref().unwrap_or(sym.as_str());

                            let dem_match = self.matcher.fuzzy_indices(demangled, q);
                            let raw_match = self.matcher.fuzzy_indices(sym.as_str(), q);

                            if dem_match.is_some() || raw_match.is_some() {
                                match_count += 1;
                                matched_items.push((sym, demangled_res, dem_match, raw_match));
                            }
                        }

                        lines.push(Line::from(vec![
                            Span::styled(
                                format!("Exported Symbols: {} matches (out of {} total)", match_count, total),
                                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                            ),
                        ]));
                        lines.push(Line::from(""));

                        if matched_items.is_empty() {
                            lines.push(Line::from(Span::styled(
                                format!("No exported symbols matching '{}' (out of {} total)", q, total),
                                Style::default().fg(Color::DarkGray),
                            )));
                        } else {
                            for (idx, (sym, demangled_res, dem_match, raw_match)) in matched_items.into_iter().enumerate() {
                                let demangled = demangled_res.as_deref().unwrap_or(sym.as_str());
                                let mut line_spans = vec![
                                    Span::styled(format!("{:4}. ", idx + 1), Style::default().fg(Color::DarkGray)),
                                ];

                                if let Some((_, indices)) = dem_match {
                                    line_spans.extend(Self::highlight_matches(demangled, &indices, Style::default().fg(Color::White), base_match_style));
                                } else {
                                    line_spans.push(Span::styled(demangled.to_string(), Style::default().fg(Color::White)));
                                }
                                match_lines.push(lines.len());
                                lines.push(Line::from(line_spans));

                                if let Ok(dem) = &demangled_res {
                                    if dem != sym.as_str() {
                                        let mut raw_spans = vec![
                                            Span::raw("      raw: "),
                                        ];
                                        if let Some((_, raw_idx)) = raw_match {
                                            raw_spans.extend(Self::highlight_matches(sym, &raw_idx, Style::default().fg(Color::DarkGray), base_match_style));
                                        } else {
                                            raw_spans.push(Span::styled(sym.to_string(), Style::default().fg(Color::DarkGray)));
                                        }
                                        lines.push(Line::from(raw_spans));
                                    }
                                }
                            }
                        }
                    } else {
                        lines.push(Line::from(vec![
                            Span::styled(format!("Total Exported Symbols: {}", total), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                        ]));
                        lines.push(Line::from(""));

                        for (idx, sym) in exported.iter().enumerate() {
                            let demangled_res = demangle_symbol(sym);
                            let demangled = demangled_res.as_deref().unwrap_or(sym.as_str());
                            lines.push(Line::from(vec![
                                Span::styled(format!("{:4}. ", idx + 1), Style::default().fg(Color::DarkGray)),
                                Span::styled(demangled.to_string(), Style::default().fg(Color::White)),
                            ]));
                            if let Ok(dem) = &demangled_res {
                                if dem != sym.as_str() {
                                    lines.push(Line::from(vec![
                                        Span::raw("      raw: "),
                                        Span::styled(sym.to_string(), Style::default().fg(Color::DarkGray)),
                                    ]));
                                }
                            }
                        }
                    }
                } else {
                    lines.push(Line::from("No symbol data extracted (run with symbol inspection enabled)."));
                }
            } else {
                lines.push(Line::from("Executable details not available."));
            }
        } else {
            lines.push(Line::from("No executable selected."));
        }

        (lines, match_count, match_lines)
    }

    fn build_imports_lines(&self, exe: Option<&Executable>, query: Option<&str>) -> (Vec<Line<'static>>, usize, Vec<usize>) {
        let mut lines = Vec::new();
        let mut match_count = 0;
        let mut match_lines = Vec::new();

        let base_match_style = Style::default()
            .fg(Color::LightYellow)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED);

        if let Some(e) = exe {
            if let Some(d) = &e.details {
                if let Some(syms) = &d.symbols {
                    let mut imported_dlls: Vec<_> = syms.imported.keys().collect();
                    imported_dlls.sort();

                    let total_imports: usize = syms.imported.values().map(|v| v.len()).sum();

                    if let Some(q) = query {
                        let mut matched_groups = Vec::new();

                        for dll in &imported_dlls {
                            let dll_match = self.matcher.fuzzy_indices(dll, q);
                            let sym_set = &syms.imported[*dll];
                            let mut sym_list: Vec<_> = sym_set.iter().collect();
                            sym_list.sort();

                            let mut matched_syms = Vec::new();
                            for sym in sym_list {
                                let demangled = demangle_symbol(sym).unwrap_or_else(|_| sym.to_string());
                                let sym_match = self.matcher.fuzzy_indices(&demangled, q).or_else(|| self.matcher.fuzzy_indices(sym, q));

                                if sym_match.is_some() || dll_match.is_some() {
                                    match_count += 1;
                                    matched_syms.push((sym, demangled, sym_match));
                                }
                            }

                            if !matched_syms.is_empty() {
                                matched_groups.push((*dll, dll_match, matched_syms));
                            }
                        }

                        lines.push(Line::from(vec![
                            Span::styled(
                                format!("Imported Symbols: {} matching symbols across {} DLLs (out of {} total)", match_count, matched_groups.len(), total_imports),
                                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                            ),
                        ]));
                        lines.push(Line::from(""));

                        if matched_groups.is_empty() {
                            lines.push(Line::from(Span::styled(
                                format!("No imported symbols matching '{}' (out of {} total across {} DLLs)", q, total_imports, imported_dlls.len()),
                                Style::default().fg(Color::DarkGray),
                            )));
                        } else {
                            for (dll, dll_match, matched_syms) in matched_groups {
                                let mut dll_spans = vec![
                                    Span::raw("From "),
                                ];
                                if let Some((_, dll_idx)) = dll_match {
                                    match_lines.push(lines.len());
                                    dll_spans.extend(Self::highlight_matches(dll, &dll_idx, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD), base_match_style));
                                } else {
                                    dll_spans.push(Span::styled(dll.to_string(), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)));
                                }
                                dll_spans.push(Span::styled(format!(": ({} matching symbols)", matched_syms.len()), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)));
                                lines.push(Line::from(dll_spans));

                                for (_sym, demangled, sym_match) in matched_syms {
                                    let mut sym_spans = vec![
                                        Span::raw("  • "),
                                    ];
                                    if let Some((_, sym_idx)) = sym_match {
                                        match_lines.push(lines.len());
                                        sym_spans.extend(Self::highlight_matches(&demangled, &sym_idx, Style::default().fg(Color::White), base_match_style));
                                    } else {
                                        sym_spans.push(Span::styled(demangled, Style::default().fg(Color::White)));
                                    }
                                    lines.push(Line::from(sym_spans));
                                }
                                lines.push(Line::from(""));
                            }
                        }
                    } else {
                        lines.push(Line::from(vec![
                            Span::styled(format!("Total Imported Symbols: {} across {} DLLs", total_imports, imported_dlls.len()), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                        ]));
                        lines.push(Line::from(""));

                        for dll in imported_dlls {
                            let sym_set = &syms.imported[dll];
                            let mut sym_list: Vec<_> = sym_set.iter().collect();
                            sym_list.sort();

                            lines.push(Line::from(vec![
                                Span::styled(format!("From {}: ({} symbols)", dll, sym_list.len()), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                            ]));

                            for sym in sym_list {
                                let demangled = demangle_symbol(sym).unwrap_or_else(|_| sym.to_string());
                                lines.push(Line::from(vec![
                                    Span::raw("  • "),
                                    Span::styled(demangled, Style::default().fg(Color::White)),
                                ]));
                            }
                            lines.push(Line::from(""));
                        }
                    }
                } else {
                    lines.push(Line::from("No symbol data extracted (run with symbol inspection enabled)."));
                }
            } else {
                lines.push(Line::from("Executable details not available."));
            }
        } else {
            lines.push(Line::from("No executable selected."));
        }

        (lines, match_count, match_lines)
    }

    fn build_diagnostics_lines(&self, exe: Option<&Executable>, name: &str, query: Option<&str>) -> (Vec<Line<'static>>, usize, Vec<usize>) {
        let mut lines = Vec::new();
        let mut match_count = 0;
        let mut match_lines = Vec::new();

        let base_match_style = Style::default()
            .fg(Color::LightYellow)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED);

        lines.push(Line::from(vec![
            Span::styled("Missing Dependency Diagnostics", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]));
        lines.push(Line::from(""));

        if let Some(report) = &self.check_report {
            let not_found = &report.not_found_libraries;
            if not_found.is_empty() {
                lines.push(Line::from(vec![
                    Span::styled("✓ No missing DLL dependencies detected across entire graph.", Style::default().fg(Color::Green)),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::styled(format!("⚠ Found {} missing DLL dependencies in graph:", not_found.len()), Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                ]));
                for (importer, missing_list) in not_found {
                    let imp_match = query.and_then(|q| self.matcher.fuzzy_indices(importer, q));
                    let mut imp_spans = vec![
                        Span::raw("  • Imported by "),
                    ];
                    if let Some((_, idx)) = imp_match {
                        match_count += 1;
                        match_lines.push(lines.len());
                        imp_spans.extend(Self::highlight_matches(importer, &idx, Style::default().fg(Color::LightYellow), base_match_style));
                    } else {
                        imp_spans.push(Span::styled(importer.clone(), Style::default().fg(Color::LightYellow)));
                    }
                    imp_spans.push(Span::raw(":"));
                    lines.push(Line::from(imp_spans));

                    for m in missing_list {
                        let m_match = query.and_then(|q| self.matcher.fuzzy_indices(m, q));
                        let mut m_spans = vec![
                            Span::raw("      - "),
                        ];
                        if let Some((_, idx)) = m_match {
                            match_count += 1;
                            match_lines.push(lines.len());
                            m_spans.extend(Self::highlight_matches(m, &idx, Style::default().fg(Color::Red).add_modifier(Modifier::BOLD), base_match_style));
                        } else {
                            m_spans.push(Span::styled(m.clone(), Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)));
                        }
                        m_spans.push(Span::styled(" [MISSING]", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)));
                        lines.push(Line::from(m_spans));
                    }
                }
            }
        }

        if let Some(e) = exe {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled(format!("Specific issues for {}:", name), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            ]));
            if !e.found {
                lines.push(Line::from(vec![
                    Span::styled("  • This DLL file was not found on the search PATH.", Style::default().fg(Color::Red)),
                ]));
            } else if let Some(d) = &e.details {
                if let Some(deps) = &d.dependencies {
                    let missing_deps: Vec<_> = deps.iter().filter(|dep| {
                        self.executables.get(dep).map(|de| !de.found).unwrap_or(true)
                    }).collect();

                    if missing_deps.is_empty() {
                        lines.push(Line::from(vec![
                            Span::styled("  ✓ All direct dependencies were found.", Style::default().fg(Color::Green)),
                        ]));
                    } else {
                        for md in missing_deps {
                            let md_match = query.and_then(|q| self.matcher.fuzzy_indices(md, q));
                            let mut md_spans = vec![
                                Span::raw("  • Direct dependency '"),
                            ];
                            if let Some((_, idx)) = md_match {
                                match_count += 1;
                                match_lines.push(lines.len());
                                md_spans.extend(Self::highlight_matches(md, &idx, Style::default().fg(Color::Red).add_modifier(Modifier::BOLD), base_match_style));
                            } else {
                                md_spans.push(Span::styled(md.clone(), Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)));
                            }
                            md_spans.push(Span::styled("' is missing!", Style::default().fg(Color::Red)));
                            lines.push(Line::from(md_spans));
                        }
                    }
                }
            }
        }

        (lines, match_count, match_lines)
    }

    /// Retrieve the currently selected DLL name from the tree path
    fn get_selected_dll_name(&self) -> Option<String> {
        self.tree_state.selected().last().cloned()
    }

    /// Retrieve the currently selected `Executable`
    fn get_selected_executable(&self) -> Option<&'a Executable> {
        self.get_selected_dll_name()
            .and_then(|name| self.executables.get(&name))
    }

    /// Open all ancestors and select a given DLL in the tree
    fn select_and_reveal_dll(&mut self, dll_name: &str) {
        if let Some(path) = self.find_path_to_dll(dll_name) {
            // Open all parent nodes along the path
            for i in 1..path.len() {
                self.tree_state.open(path[0..i].to_vec());
            }
            self.tree_state.select(path);
            self.details_scroll = 0;
        }
    }

    /// Find a path in the tree from root to a given DLL name
    fn find_path_to_dll(&self, target_dll: &str) -> Option<Vec<String>> {
        let target_key = DllName::from(target_dll);
        let roots: Vec<&Executable> = if let Ok(Some(root)) = self.executables.get_root() {
            vec![root]
        } else {
            self.executables
                .sorted_by_first_appearance()
                .into_iter()
                .filter(|e| e.depth_first_appearance == 0)
                .collect()
        };

        for root in roots {
            let mut visited = HashSet::new();
            if let Some(path) = self.dfs_find_path(root, &target_key, &mut visited) {
                return Some(path);
            }
        }

        // Fallback: check if it's any known executable
        if self.executables.contains(target_dll) {
            Some(vec![target_dll.to_string()])
        } else {
            None
        }
    }

    fn dfs_find_path(
        &self,
        current: &Executable,
        target: &DllName,
        visited: &mut HashSet<DllName>,
    ) -> Option<Vec<String>> {
        let current_key = DllName::from(&current.dllname);
        if &current_key == target {
            return Some(vec![current.dllname.clone()]);
        }

        if !visited.insert(current_key.clone()) {
            return None;
        }

        if let Some(details) = &current.details {
            if let Some(dependencies) = &details.dependencies {
                for dep_name in dependencies {
                    if let Some(dep_exe) = self.executables.get(dep_name) {
                        if let Some(mut subpath) = self.dfs_find_path(dep_exe, target, visited) {
                            let mut path = vec![current.dllname.clone()];
                            path.append(&mut subpath);
                            return Some(path);
                        }
                    } else if dep_name.eq_ignore_ascii_case(target.as_str()) {
                        return Some(vec![current.dllname.clone(), dep_name.clone()]);
                    }
                }
            }
        }

        visited.remove(&current_key);
        None
    }

    /// Build the `TreeItem` list for `tui-tree-widget`
    fn build_tree_items(&mut self) -> Vec<TreeItem<'static, String>> {
        let roots: Vec<&Executable> = if let Ok(Some(root)) = self.executables.get_root() {
            vec![root]
        } else {
            let r: Vec<&Executable> = self
                .executables
                .sorted_by_first_appearance()
                .into_iter()
                .filter(|e| e.depth_first_appearance == 0)
                .collect();
            if r.is_empty() {
                self.executables.sorted_by_first_appearance()
            } else {
                r
            }
        };

        let query = if self.search_query.trim().is_empty() {
            None
        } else {
            Some(self.search_query.trim())
        };

        let mut count = 0;
        let mut items = Vec::new();
        let mut paths_to_open = Vec::new();
        let mut matching_paths = Vec::new();
        let mut first_match_path = None;

        for root in roots {
            let mut branch_ancestors = HashSet::new();
            if let Some(item) = self.build_tree_node(
                root,
                query,
                &mut branch_ancestors,
                &mut count,
                vec![],
                &mut paths_to_open,
                &mut matching_paths,
                &mut first_match_path,
            ) {
                items.push(item);
            }
        }

        self.match_count = count;
        self.tree_matching_paths = matching_paths;

        if query.is_some() {
            for path in paths_to_open {
                self.tree_state.open(path);
            }
            if let Some(first_path) = first_match_path {
                let current_selection = self.tree_state.selected();
                if current_selection.is_empty()
                    || !self.tree_matching_paths.iter().any(|p| p == current_selection)
                {
                    self.tree_state.select(first_path);
                }
            }
        }

        items
    }

    fn build_tree_node(
        &self,
        exe: &Executable,
        query: Option<&str>,
        branch_ancestors: &mut HashSet<DllName>,
        match_count: &mut usize,
        current_path: Vec<String>,
        paths_to_open: &mut Vec<Vec<String>>,
        matching_paths: &mut Vec<Vec<String>>,
        first_match_path: &mut Option<Vec<String>>,
    ) -> Option<TreeItem<'static, String>> {
        let dll_key = DllName::from(&exe.dllname);

        if !self.print_system_dlls && exe.details.as_ref().map(|d| d.is_system).unwrap_or(false) && exe.depth_first_appearance > 0 {
            return None;
        }

        let is_cyclic = branch_ancestors.contains(&dll_key);
        let mut new_path = current_path;
        new_path.push(exe.dllname.clone());

        // Check node fuzzy match
        let node_match = if let Some(q) = query {
            self.matcher.fuzzy_indices(&exe.dllname, q)
        } else {
            None
        };

        if node_match.is_some() {
            *match_count += 1;
            matching_paths.push(new_path.clone());
            if first_match_path.is_none() {
                *first_match_path = Some(new_path.clone());
            }
        }

        branch_ancestors.insert(dll_key.clone());

        let mut children = Vec::new();
        if !is_cyclic {
            if let Some(details) = &exe.details {
                if let Some(dependencies) = &details.dependencies {
                    for dep_name in dependencies {
                        if let Some(dep_exe) = self.executables.get(dep_name) {
                            if let Some(child_item) = self.build_tree_node(
                                dep_exe,
                                query,
                                branch_ancestors,
                                match_count,
                                new_path.clone(),
                                paths_to_open,
                                matching_paths,
                                first_match_path,
                            ) {
                                children.push(child_item);
                            }
                        } else {
                            // Missing dependency not in index
                            let mut dep_match = None;
                            if let Some(q) = query {
                                dep_match = self.matcher.fuzzy_indices(dep_name, q);
                                if dep_match.is_some() {
                                    *match_count += 1;
                                    let mut dep_path = new_path.clone();
                                    dep_path.push(dep_name.clone());
                                    matching_paths.push(dep_path.clone());
                                    if first_match_path.is_none() {
                                        *first_match_path = Some(dep_path);
                                    }
                                }
                            }
                            if query.is_none() || dep_match.is_some() {
                                let line = Self::format_node_line(
                                    dep_name,
                                    false,
                                    None,
                                    true,
                                    dep_match.map(|(_, idxs)| idxs),
                                );
                                children.push(TreeItem::new_leaf(dep_name.clone(), line));
                            }
                        }
                    }
                }
            }
        }

        branch_ancestors.remove(&dll_key);

        // If filtering: show if this node matches OR any child matches
        let should_include = query.is_none() || node_match.is_some() || !children.is_empty();

        if !should_include {
            return None;
        }

        if query.is_some() && !children.is_empty() {
            paths_to_open.push(new_path.clone());
        }

        let line = Self::format_node_line(
            &exe.dllname,
            exe.depth_first_appearance == 0,
            exe.details.as_ref(),
            !exe.found,
            node_match.map(|(_, idxs)| idxs),
        );

        if children.is_empty() {
            Some(TreeItem::new_leaf(exe.dllname.clone(), line))
        } else {
            Some(TreeItem::new(exe.dllname.clone(), line.clone(), children).unwrap_or_else(|_| {
                TreeItem::new_leaf(exe.dllname.clone(), line)
            }))
        }
    }

    /// Jump to next match in currently focused pane
    pub fn jump_next_match(&mut self) {
        match self.active_pane {
            ActivePane::Tree => self.jump_next_tree_match(),
            ActivePane::Details => self.jump_next_details_match(),
        }
    }

    /// Jump to previous match in currently focused pane
    pub fn jump_prev_match(&mut self) {
        match self.active_pane {
            ActivePane::Tree => self.jump_prev_tree_match(),
            ActivePane::Details => self.jump_prev_details_match(),
        }
    }

    /// Jump to next match in the dependency tree
    pub fn jump_next_tree_match(&mut self) {
        if self.tree_matching_paths.is_empty() {
            return;
        }
        let current = self.tree_state.selected();
        let next_idx = if let Some(pos) = self.tree_matching_paths.iter().position(|p| p == current) {
            (pos + 1) % self.tree_matching_paths.len()
        } else {
            self.tree_matching_paths
                .iter()
                .position(|p| p.as_slice() > current)
                .unwrap_or(0)
        };
        let target_path = self.tree_matching_paths[next_idx].clone();
        for i in 1..target_path.len() {
            self.tree_state.open(target_path[0..i].to_vec());
        }
        self.tree_state.select(target_path);
        self.details_scroll = 0;
    }

    /// Jump to previous match in the dependency tree
    pub fn jump_prev_tree_match(&mut self) {
        if self.tree_matching_paths.is_empty() {
            return;
        }
        let current = self.tree_state.selected();
        let prev_idx = if let Some(pos) = self.tree_matching_paths.iter().position(|p| p == current) {
            (pos + self.tree_matching_paths.len() - 1) % self.tree_matching_paths.len()
        } else {
            self.tree_matching_paths
                .iter()
                .rposition(|p| p.as_slice() < current)
                .unwrap_or(self.tree_matching_paths.len() - 1)
        };
        let target_path = self.tree_matching_paths[prev_idx].clone();
        for i in 1..target_path.len() {
            self.tree_state.open(target_path[0..i].to_vec());
        }
        self.tree_state.select(target_path);
        self.details_scroll = 0;
    }

    /// Jump to next match in details pane
    pub fn jump_next_details_match(&mut self) {
        if self.details_match_lines.is_empty() {
            return;
        }
        if let Some(&line) = self.details_match_lines.iter().find(|&&l| l > self.details_scroll) {
            self.details_scroll = line;
        } else {
            self.details_scroll = self.details_match_lines[0];
        }
    }

    /// Jump to previous match in details pane
    pub fn jump_prev_details_match(&mut self) {
        if self.details_match_lines.is_empty() {
            return;
        }
        if let Some(&line) = self.details_match_lines.iter().rfind(|&&l| l < self.details_scroll) {
            self.details_scroll = line;
        } else {
            self.details_scroll = *self.details_match_lines.last().unwrap();
        }
    }

    /// Returns (current_1_based_index, total_matches) for active pane if filtered
    pub fn current_match_position(&self) -> Option<(usize, usize)> {
        match self.active_pane {
            ActivePane::Tree => {
                if self.tree_matching_paths.is_empty() {
                    None
                } else {
                    let current = self.tree_state.selected();
                    let pos = self
                        .tree_matching_paths
                        .iter()
                        .position(|p| p == current)
                        .map(|i| i + 1)
                        .unwrap_or(1);
                    Some((pos, self.tree_matching_paths.len()))
                }
            }
            ActivePane::Details => {
                if self.details_match_lines.is_empty() {
                    None
                } else {
                    let pos = self
                        .details_match_lines
                        .iter()
                        .position(|&l| l >= self.details_scroll)
                        .map(|i| i + 1)
                        .unwrap_or(self.details_match_lines.len());
                    Some((pos, self.details_match_lines.len()))
                }
            }
        }
    }

    fn format_node_line(
        name: &str,
        is_root: bool,
        details: Option<&crate::executable::ExecutableDetails>,
        is_missing: bool,
        matched_indices: Option<Vec<usize>>,
    ) -> Line<'static> {
        let mut spans = Vec::new();

        let base_style = if is_missing {
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
        } else if is_root {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else if let Some(d) = details {
            if d.is_known_dll {
                Style::default().fg(Color::Yellow)
            } else if d.is_api_set {
                Style::default().fg(Color::Magenta)
            } else if d.is_system {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default().fg(Color::White)
            }
        } else {
            Style::default().fg(Color::Gray)
        };

        // Render DLL name with match highlighting if present
        if let Some(indices) = matched_indices {
            let match_style = base_style
                .fg(Color::LightYellow)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
            spans.extend(Self::highlight_matches(name, &indices, base_style, match_style));
        } else {
            spans.push(Span::styled(name.to_string(), base_style));
        }

        // Add badge tags
        if is_missing {
            spans.push(Span::styled(
                " [NOT FOUND]",
                Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD),
            ));
        } else if let Some(d) = details {
            if d.is_known_dll {
                spans.push(Span::styled(" [Known]", Style::default().fg(Color::Yellow)));
            } else if d.is_api_set {
                spans.push(Span::styled(" [API Set]", Style::default().fg(Color::Magenta)));
            } else if d.is_system {
                spans.push(Span::styled(" [System]", Style::default().fg(Color::DarkGray)));
            }
        }

        Line::from(spans)
    }

    /// Expand all nodes in the tree
    fn expand_all(&mut self) {
        for exe in self.executables.iter() {
            if let Some(path) = self.find_path_to_dll(&exe.dllname) {
                self.tree_state.open(path);
            }
        }
    }

    /// Collapse all nodes in the tree
    fn collapse_all(&mut self) {
        if let Ok(Some(root)) = self.executables.get_root() {
            self.tree_state = TreeState::default();
            self.tree_state.select(vec![root.dllname.clone()]);
        } else {
            self.tree_state = TreeState::default();
            if let Some(first) = self.executables.sorted_by_first_appearance().first() {
                self.tree_state.select(vec![first.dllname.clone()]);
            }
        }
    }
}

/// Run the interactive Ratatui application
pub fn run_tui(executables: &Executables, print_system_dlls: bool) -> anyhow::Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Setup panic hook to always restore terminal
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        default_hook(info);
    }));

    let mut app = App::new(executables, print_system_dlls);
    let res = main_loop(&mut terminal, &mut app);

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    res
}

fn main_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
) -> anyhow::Result<()> {
    let mut needs_draw = true;

    loop {
        if needs_draw {
            app.ensure_tree_updated();
            app.ensure_details_updated();
            terminal.draw(|f| ui(f, app))?;
            needs_draw = false;
        }

        if event::poll(std::time::Duration::from_millis(100))? {
            match event::read()? {
                Event::Resize(_, _) => {
                    needs_draw = true;
                }
                Event::Key(key) => {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }

                    needs_draw = true;

                    // If Help modal is open
                    if app.show_help {
                        match key.code {
                            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') | KeyCode::F(1) => {
                                app.show_help = false;
                            }
                            _ => {}
                        }
                        continue;
                    }

                    // If Search Mode is active
                    if app.search_active {
                        match key.code {
                            KeyCode::Esc => {
                                app.search_active = false;
                                match app.active_pane {
                                    ActivePane::Tree => {
                                        app.search_query.clear();
                                        app.mark_tree_dirty();
                                    }
                                    ActivePane::Details => {
                                        app.details_search_query.clear();
                                        app.details_scroll = 0;
                                    }
                                }
                            }
                            KeyCode::Enter => {
                                app.search_active = false;
                            }
                            KeyCode::Backspace => {
                                match app.active_pane {
                                    ActivePane::Tree => {
                                        app.search_query.pop();
                                        app.mark_tree_dirty();
                                    }
                                    ActivePane::Details => {
                                        app.details_search_query.pop();
                                        app.details_scroll = 0;
                                    }
                                }
                            }
                            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                match app.active_pane {
                                    ActivePane::Tree => {
                                        app.search_query.clear();
                                        app.mark_tree_dirty();
                                    }
                                    ActivePane::Details => {
                                        app.details_search_query.clear();
                                        app.details_scroll = 0;
                                    }
                                }
                            }
                            KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                app.jump_next_match();
                            }
                            KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                app.jump_prev_match();
                            }
                            KeyCode::F(3) => {
                                if key.modifiers.contains(KeyModifiers::SHIFT) {
                                    app.jump_prev_match();
                                } else {
                                    app.jump_next_match();
                                }
                            }
                            KeyCode::Up => {
                                match app.active_pane {
                                    ActivePane::Tree => {
                                        app.tree_state.key_up();
                                        app.details_scroll = 0;
                                    }
                                    ActivePane::Details => {
                                        app.details_scroll = app.details_scroll.saturating_sub(1);
                                    }
                                }
                            }
                            KeyCode::Down => {
                                match app.active_pane {
                                    ActivePane::Tree => {
                                        app.tree_state.key_down();
                                        app.details_scroll = 0;
                                    }
                                    ActivePane::Details => {
                                        app.details_scroll = app.details_scroll.saturating_add(1);
                                    }
                                }
                            }
                            KeyCode::PageUp => {
                                match app.active_pane {
                                    ActivePane::Tree => {
                                        for _ in 0..5 {
                                            app.tree_state.key_up();
                                        }
                                        app.details_scroll = 0;
                                    }
                                    ActivePane::Details => {
                                        app.details_scroll = app.details_scroll.saturating_sub(10);
                                    }
                                }
                            }
                            KeyCode::PageDown => {
                                match app.active_pane {
                                    ActivePane::Tree => {
                                        for _ in 0..5 {
                                            app.tree_state.key_down();
                                        }
                                        app.details_scroll = 0;
                                    }
                                    ActivePane::Details => {
                                        app.details_scroll = app.details_scroll.saturating_add(10);
                                    }
                                }
                            }
                            KeyCode::Tab => {
                                app.search_active = false;
                                app.active_pane = match app.active_pane {
                                    ActivePane::Tree => ActivePane::Details,
                                    ActivePane::Details => ActivePane::Tree,
                                };
                            }
                            KeyCode::Char(c) => {
                                match app.active_pane {
                                    ActivePane::Tree => {
                                        app.search_query.push(c);
                                        app.mark_tree_dirty();
                                    }
                                    ActivePane::Details => {
                                        app.details_search_query.push(c);
                                        app.details_scroll = 0;
                                    }
                                }
                            }
                            _ => {}
                        }
                        continue;
                    }

                    // Normal Mode Navigation & Shortcuts
                    match key.code {
                        // Quit
                        KeyCode::Char('q') => {
                            break;
                        }
                        KeyCode::Esc => {
                            match app.active_pane {
                                ActivePane::Tree if !app.search_query.is_empty() => {
                                    app.search_query.clear();
                                    app.mark_tree_dirty();
                                }
                                ActivePane::Details if !app.details_search_query.is_empty() => {
                                    app.details_search_query.clear();
                                    app.details_scroll = 0;
                                }
                                _ if !app.search_query.is_empty() => {
                                    app.search_query.clear();
                                    app.mark_tree_dirty();
                                }
                                _ if !app.details_search_query.is_empty() => {
                                    app.details_search_query.clear();
                                    app.details_scroll = 0;
                                }
                                _ => {
                                    break;
                                }
                            }
                        }

                        // Toggle Help
                        KeyCode::Char('?') | KeyCode::F(1) => {
                            app.show_help = !app.show_help;
                        }

                        // Jump between filter results
                        KeyCode::Char('n') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                            app.jump_next_match();
                        }
                        KeyCode::Char('N') => {
                            app.jump_prev_match();
                        }
                        KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            app.jump_next_match();
                        }
                        KeyCode::F(3) => {
                            if key.modifiers.contains(KeyModifiers::SHIFT) {
                                app.jump_prev_match();
                            } else {
                                app.jump_next_match();
                            }
                        }

                        // Skim Fuzzy Filter on Focused Pane
                        KeyCode::Char('/') | KeyCode::Char('s') | KeyCode::Char('f') => {
                            app.search_active = true;
                        }
                        KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            match app.active_pane {
                                ActivePane::Tree if !app.search_query.is_empty() => {
                                    app.jump_prev_match();
                                }
                                ActivePane::Details if !app.details_search_query.is_empty() => {
                                    app.jump_prev_match();
                                }
                                _ => {
                                    app.search_active = true;
                                }
                            }
                        }

                        // Skim Fuzzy Finder on Symbols
                        KeyCode::Char('S') => {
                            // Suspend TUI
                            disable_raw_mode()?;
                            execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

                            let selected = skim_symbols(app.executables, None);

                            // Resume TUI
                            enable_raw_mode()?;
                            execute!(terminal.backend_mut(), EnterAlternateScreen)?;
                            terminal.clear()?;

                            if let Some(symbols) = selected {
                                if let Some(_first_sym) = symbols.first() {
                                    // Jump to DLL containing this symbol
                                    if let Some(selected_dll) = app.get_selected_dll_name() {
                                        app.select_and_reveal_dll(&selected_dll);
                                    }
                                }
                            }
                        }

                        // Switch active pane
                        KeyCode::Tab => {
                            app.active_pane = match app.active_pane {
                                ActivePane::Tree => ActivePane::Details,
                                ActivePane::Details => ActivePane::Tree,
                            };
                        }

                        // Switch tabs
                        KeyCode::Char('1') => {
                            app.selected_tab = DetailTab::Overview;
                            app.details_scroll = 0;
                        }
                        KeyCode::Char('2') => {
                            app.selected_tab = DetailTab::ExportedSymbols;
                            app.details_scroll = 0;
                        }
                        KeyCode::Char('3') => {
                            app.selected_tab = DetailTab::ImportedSymbols;
                            app.details_scroll = 0;
                        }
                        KeyCode::Char('4') => {
                            app.selected_tab = DetailTab::Diagnostics;
                            app.details_scroll = 0;
                        }
                        KeyCode::Char(']') => {
                            app.selected_tab = app.selected_tab.next();
                            app.details_scroll = 0;
                        }
                        KeyCode::Char('[') => {
                            app.selected_tab = app.selected_tab.prev();
                            app.details_scroll = 0;
                        }

                        // Toggle expand/collapse all
                        KeyCode::Char('e') => {
                            app.expand_all();
                        }
                        KeyCode::Char('c') => {
                            app.collapse_all();
                        }

                        // Toggle system DLLs
                        KeyCode::Char('y') => {
                            app.print_system_dlls = !app.print_system_dlls;
                            app.mark_tree_dirty();
                        }

                        // Tree Navigation
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.active_pane == ActivePane::Tree {
                                app.tree_state.key_up();
                                app.details_scroll = 0;
                            } else {
                                app.details_scroll = app.details_scroll.saturating_sub(1);
                            }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if app.active_pane == ActivePane::Tree {
                                app.tree_state.key_down();
                                app.details_scroll = 0;
                            } else {
                                app.details_scroll = app.details_scroll.saturating_add(1);
                            }
                        }
                        KeyCode::Left | KeyCode::Char('h') => {
                            if app.active_pane == ActivePane::Tree {
                                app.tree_state.key_left();
                            }
                        }
                        KeyCode::Right | KeyCode::Char('l') => {
                            if app.active_pane == ActivePane::Tree {
                                app.tree_state.key_right();
                            }
                        }
                        KeyCode::Char(' ') | KeyCode::Enter => {
                            if app.active_pane == ActivePane::Tree {
                                app.tree_state.toggle_selected();
                            }
                        }
                        KeyCode::PageUp => {
                            app.details_scroll = app.details_scroll.saturating_sub(10);
                        }
                        KeyCode::PageDown => {
                            app.details_scroll = app.details_scroll.saturating_add(10);
                        }
                        KeyCode::Home => {
                            if app.active_pane == ActivePane::Tree {
                                app.tree_state.select_first();
                            } else {
                                app.details_scroll = 0;
                            }
                        }
                        KeyCode::End => {
                            if app.active_pane == ActivePane::Tree {
                                app.tree_state.select_last();
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }

    Ok(())
}

fn ui(f: &mut Frame, app: &mut App) {
    let size = f.area();

    // Top Header, Main Area, Bottom Status/Search bar
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(5),    // Body (Tree + Details)
            Constraint::Length(3), // Footer / Search
        ])
        .split(size);

    // 1. Header
    render_header(f, app, main_chunks[0]);

    // 2. Body Split: Left = Tree, Right = Details
    let body_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(45), // Tree
            Constraint::Percentage(55), // Details
        ])
        .split(main_chunks[1]);

    render_tree_pane(f, app, body_chunks[0]);
    render_details_pane(f, app, body_chunks[1]);

    // 3. Footer
    render_footer(f, app, main_chunks[2]);

    // 4. Modal Help Overlay
    if app.show_help {
        render_help_modal(f, size);
    }
}

fn render_header(f: &mut Frame, app: &App, area: Rect) {
    let root_name = app
        .executables
        .get_root()
        .ok()
        .flatten()
        .map(|r| r.dllname.as_str())
        .unwrap_or("Dependencies");

    let system_dll_status = if app.print_system_dlls {
        "Showing System DLLs [y]"
    } else {
        "Hiding System DLLs [y]"
    };

    let title_line = Line::from(vec![
        Span::styled(" Dependency Runner ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw("─ Target: "),
        Span::styled(root_name, Style::default().fg(Color::LightYellow).add_modifier(Modifier::BOLD)),
        Span::raw(format!(" ({} total DLLs) ─ ", app.executables.len())),
        Span::styled(system_dll_status, Style::default().fg(Color::DarkGray)),
    ]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(title_line)
        .title_alignment(Alignment::Left);

    let help_hint = Paragraph::new(Line::from(vec![
        Span::raw("Press "),
        Span::styled("?", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::raw(" for Help  |  "),
        Span::styled("/ or s", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::raw(" Filter Focused Pane (Skim)  |  "),
        Span::styled("q", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::raw(" Quit"),
    ]))
    .alignment(Alignment::Right)
    .block(block);

    f.render_widget(help_hint, area);
}

fn render_tree_pane(f: &mut Frame, app: &mut App, area: Rect) {
    let is_focused = app.active_pane == ActivePane::Tree;
    let border_color = if is_focused { Color::Cyan } else { Color::Gray };

    let mut title = " Dependency Tree ".to_string();
    if !app.search_query.is_empty() {
        if let Some((curr, total)) = if app.active_pane == ActivePane::Tree { app.current_match_position() } else { None } {
            title = format!(" Tree (Skim Filter: '{}' ─ match {}/{} [n/N]) ", app.search_query, curr, total);
        } else {
            title = format!(" Tree (Skim Filter: '{}' ─ {} matches) ", app.search_query, app.match_count);
        }
    }

    let tree_block = Block::default()
        .borders(Borders::ALL)
        .border_type(if is_focused { BorderType::Thick } else { BorderType::Rounded })
        .border_style(Style::default().fg(border_color))
        .title(Span::styled(title, Style::default().add_modifier(Modifier::BOLD)));

    let tree_widget = Tree::new(&app.cached_tree_items)
        .expect("all item identifier paths are unique")
        .block(tree_block)
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(40, 60, 90))
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    f.render_stateful_widget(tree_widget, area, &mut app.tree_state);
}

fn render_details_pane(f: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.active_pane == ActivePane::Details;
    let border_color = if is_focused { Color::Cyan } else { Color::Gray };

    let selected_name = app.get_selected_dll_name().unwrap_or_else(|| "<No selection>".to_string());

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Tabs
            Constraint::Min(0),    // Content
        ])
        .split(area);

    // Render Tabs
    let tab_titles: Vec<Line> = DetailTab::titles()
        .into_iter()
        .map(|t| Line::from(t))
        .collect();

    let mut inspect_title = format!(" Inspect: {} ", selected_name);
    if !app.details_search_query.is_empty() {
        if let Some((curr, total)) = if app.active_pane == ActivePane::Details { app.current_match_position() } else { None } {
            inspect_title = format!(
                " Inspect: {} (Skim Filter: '{}' ─ match {}/{} [n/N]) ",
                selected_name, app.details_search_query, curr, total
            );
        } else {
            inspect_title = format!(
                " Inspect: {} (Skim Filter: '{}' ─ {} matches) ",
                selected_name, app.details_search_query, app.details_match_count
            );
        }
    }

    let tabs_widget = Tabs::new(tab_titles)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(if is_focused { BorderType::Thick } else { BorderType::Rounded })
                .border_style(Style::default().fg(border_color))
                .title(Span::styled(
                    inspect_title,
                    Style::default().add_modifier(Modifier::BOLD).fg(Color::Yellow),
                )),
        )
        .select(app.selected_tab as usize)
        .style(Style::default().fg(Color::Gray))
        .highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        );

    f.render_widget(tabs_widget, chunks[0]);

    // Render Tab Content
    let content_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color));

    let p = Paragraph::new(&app.cached_details_lines[..])
        .block(content_block)
        .wrap(Wrap { trim: false })
        .scroll((app.details_scroll as u16, 0));

    f.render_widget(p, chunks[1]);
}

fn render_footer(f: &mut Frame, app: &App, area: Rect) {
    if app.search_active {
        let (pane_label, query_str, match_count) = match app.active_pane {
            ActivePane::Tree => ("Tree", &app.search_query, app.match_count),
            ActivePane::Details => ("Details", &app.details_search_query, app.details_match_count),
        };

        let search_text = Line::from(vec![
            Span::styled(format!(" Skim Filter ({}): ", pane_label), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw("> "),
            Span::styled(query_str, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled("█", Style::default().fg(Color::Cyan)), // blinking cursor simulation
            Span::raw(format!("  ({} matches)", match_count)),
            Span::styled("   [Enter] Lock  [Ctrl+n/p or F3] Jump match  [Esc] Clear & Exit  [↑/↓] Scroll", Style::default().fg(Color::DarkGray)),
        ]);

        let p = Paragraph::new(search_text).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Yellow)),
        );
        f.render_widget(p, area);
    } else {
        let mut query_indicators = Vec::new();
        if !app.search_query.is_empty() {
            if let Some((curr, total)) = if app.active_pane == ActivePane::Tree { app.current_match_position() } else { None } {
                query_indicators.push(format!("Tree: '{}' ({}/{})", app.search_query, curr, total));
            } else {
                query_indicators.push(format!("Tree: '{}' ({})", app.search_query, app.match_count));
            }
        }
        if !app.details_search_query.is_empty() {
            if let Some((curr, total)) = if app.active_pane == ActivePane::Details { app.current_match_position() } else { None } {
                query_indicators.push(format!("Details: '{}' ({}/{})", app.details_search_query, curr, total));
            } else {
                query_indicators.push(format!("Details: '{}' ({})", app.details_search_query, app.details_match_count));
            }
        }

        let query_str = if !query_indicators.is_empty() {
            format!(" | Filter [{}] [n/N jump, Esc clear]", query_indicators.join(", "))
        } else {
            "".to_string()
        };

        let status_line = Line::from(vec![
            Span::styled("[↑/↓/j/k]", Style::default().fg(Color::Cyan)),
            Span::raw(" Nav  "),
            Span::styled("[←/→/Space]", Style::default().fg(Color::Cyan)),
            Span::raw(" Fold/Unfold  "),
            Span::styled("[/ or s]", Style::default().fg(Color::Yellow)),
            Span::raw(" Filter  "),
            Span::styled("[n/N]", Style::default().fg(Color::Yellow)),
            Span::raw(" Next/Prev Match  "),
            Span::styled("[Tab]", Style::default().fg(Color::Cyan)),
            Span::raw(" Switch Pane  "),
            Span::styled("[1-4]", Style::default().fg(Color::Cyan)),
            Span::raw(" Tabs  "),
            Span::styled("[?]", Style::default().fg(Color::Yellow)),
            Span::raw(" Help  "),
            Span::styled("[q]", Style::default().fg(Color::Red)),
            Span::raw(" Quit"),
            Span::styled(query_str, Style::default().fg(Color::LightYellow).add_modifier(Modifier::BOLD)),
        ]);

        let p = Paragraph::new(status_line).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::DarkGray)),
        );
        f.render_widget(p, area);
    }
}

fn render_help_modal(f: &mut Frame, area: Rect) {
    let popup_area = centered_rect(60, 70, area);

    f.render_widget(Clear, popup_area);

    let help_text = vec![
        Line::from(Span::styled("Interactive Tree Navigation Help", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Up / Down, j / k      ", Style::default().fg(Color::Yellow)),
            Span::raw("Move selection up/down or scroll details"),
        ]),
        Line::from(vec![
            Span::styled("  Left / Right, h / l   ", Style::default().fg(Color::Yellow)),
            Span::raw("Collapse / Expand current node or jump parent/child"),
        ]),
        Line::from(vec![
            Span::styled("  Space / Enter         ", Style::default().fg(Color::Yellow)),
            Span::raw("Toggle expand/collapse of current tree node"),
        ]),
        Line::from(vec![
            Span::styled("  e / c                 ", Style::default().fg(Color::Yellow)),
            Span::raw("Expand all / Collapse all nodes"),
        ]),
        Line::from(vec![
            Span::styled("  y                     ", Style::default().fg(Color::Yellow)),
            Span::raw("Toggle showing/hiding System DLLs"),
        ]),
        Line::from(vec![
            Span::styled("  Tab                   ", Style::default().fg(Color::Yellow)),
            Span::raw("Switch focus between Tree and Details panel"),
        ]),
        Line::from(vec![
            Span::styled("  1, 2, 3, 4            ", Style::default().fg(Color::Yellow)),
            Span::raw("Switch Details Tab (Overview, Exports, Imports, Diagnostics)"),
        ]),
        Line::from(vec![
            Span::styled("  [ / ]                 ", Style::default().fg(Color::Yellow)),
            Span::raw("Previous / Next Details Tab"),
        ]),
        Line::from(""),
        Line::from(Span::styled("Skim Fuzzy Search & Filtering", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from(vec![
            Span::styled("  / , s , f , Ctrl+P    ", Style::default().fg(Color::Yellow)),
            Span::raw("Filter the active pane (Tree or Details tab) in real time with Skim"),
        ]),
        Line::from(vec![
            Span::styled("  n / N, F3 / Shift+F3  ", Style::default().fg(Color::Yellow)),
            Span::raw("Jump to next / previous filter match in focused pane"),
        ]),
        Line::from(vec![
            Span::styled("  Ctrl+N / Ctrl+P       ", Style::default().fg(Color::Yellow)),
            Span::raw("Jump to next / previous match during search input or normal mode"),
        ]),
        Line::from(vec![
            Span::styled("  S                     ", Style::default().fg(Color::Yellow)),
            Span::raw("Launch Skim fuzzy finder on all exported/imported Symbols"),
        ]),
        Line::from(vec![
            Span::styled("  Esc                   ", Style::default().fg(Color::Yellow)),
            Span::raw("Clear active filter / Close help modal"),
        ]),
        Line::from(vec![
            Span::styled("  q                     ", Style::default().fg(Color::Yellow)),
            Span::raw("Exit application"),
        ]),
        Line::from(""),
        Line::from(Span::styled("Press Esc, ?, or q to close this help window.", Style::default().fg(Color::DarkGray))),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(" Keyboard Shortcuts ", Style::default().add_modifier(Modifier::BOLD)));

    let p = Paragraph::new(help_text)
        .block(block)
        .alignment(Alignment::Left)
        .wrap(Wrap { trim: false });

    f.render_widget(p, popup_area);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use crate::executable::{Executable, ExecutableDetails, ExecutableSymbols};
    use std::collections::HashMap;

    fn create_test_executables() -> Executables {
        let mut exes = Executables::new();

        let root_exe = Executable {
            dllname: "main.exe".to_string(),
            depth_first_appearance: 0,
            found: true,
            details: Some(ExecutableDetails {
                is_api_set: false,
                is_system: false,
                is_known_dll: false,
                full_path: PathBuf::from("C:\\app\\main.exe"),
                dependencies: Some(vec!["dep1.dll".to_string(), "dep2.dll".to_string()]),
                symbols: Some(ExecutableSymbols {
                    exported: HashSet::new(),
                    imported: HashMap::from([
                        ("dep1.dll".to_string(), HashSet::from(["fn1".to_string(), "fn2".to_string()])),
                        ("dep2.dll".to_string(), HashSet::from(["fn3".to_string()])),
                    ]),
                }),
            }),
        };

        let dep1_exe = Executable {
            dllname: "dep1.dll".to_string(),
            depth_first_appearance: 1,
            found: true,
            details: Some(ExecutableDetails {
                is_api_set: false,
                is_system: false,
                is_known_dll: false,
                full_path: PathBuf::from("C:\\app\\dep1.dll"),
                dependencies: Some(vec!["kernel32.dll".to_string()]),
                symbols: Some(ExecutableSymbols {
                    exported: HashSet::from(["fn1".to_string(), "fn2".to_string()]),
                    imported: HashMap::from([
                        ("kernel32.dll".to_string(), HashSet::from(["ExitProcess".to_string()])),
                    ]),
                }),
            }),
        };

        let dep2_missing = Executable {
            dllname: "dep2.dll".to_string(),
            depth_first_appearance: 1,
            found: false,
            details: None,
        };

        let kernel32_exe = Executable {
            dllname: "kernel32.dll".to_string(),
            depth_first_appearance: 2,
            found: true,
            details: Some(ExecutableDetails {
                is_api_set: false,
                is_system: true,
                is_known_dll: true,
                full_path: PathBuf::from("C:\\Windows\\System32\\kernel32.dll"),
                dependencies: None,
                symbols: Some(ExecutableSymbols {
                    exported: HashSet::from(["ExitProcess".to_string()]),
                    imported: HashMap::new(),
                }),
            }),
        };

        exes.insert(root_exe);
        exes.insert(dep1_exe);
        exes.insert(dep2_missing);
        exes.insert(kernel32_exe);

        exes
    }

    #[test]
    fn test_app_init() {
        let exes = create_test_executables();
        let app = App::new(&exes, true);
        assert_eq!(app.get_selected_dll_name(), Some("main.exe".to_string()));
        assert_eq!(app.active_pane, ActivePane::Tree);
        assert_eq!(app.selected_tab, DetailTab::Overview);
    }

    #[test]
    fn test_find_path_to_dll() {
        let exes = create_test_executables();
        let app = App::new(&exes, true);

        let path = app.find_path_to_dll("kernel32.dll");
        assert_eq!(
            path,
            Some(vec![
                "main.exe".to_string(),
                "dep1.dll".to_string(),
                "kernel32.dll".to_string(),
            ])
        );

        let path_missing = app.find_path_to_dll("dep2.dll");
        assert_eq!(
            path_missing,
            Some(vec!["main.exe".to_string(), "dep2.dll".to_string()])
        );
    }

    #[test]
    fn test_build_tree_and_fuzzy_search() {
        let exes = create_test_executables();
        let mut app = App::new(&exes, true);

        // Without filter
        let items = app.build_tree_items();
        assert_eq!(items.len(), 1);

        // With matching filter
        app.search_query = "kernel".to_string();
        let filtered_items = app.build_tree_items();
        assert_eq!(filtered_items.len(), 1);
        assert!(app.match_count >= 1);

        // Ensure parent paths leading to matches are already opened in tree_state
        let main_path = vec!["main.exe".to_string()];
        let dep1_path = vec!["main.exe".to_string(), "dep1.dll".to_string()];
        // open() returns false if the path was already open
        assert!(!app.tree_state.open(main_path));
        assert!(!app.tree_state.open(dep1_path));

        // With non-matching filter
        app.search_query = "nonexistent_pattern_xyz".to_string();
        let empty_items = app.build_tree_items();
        assert_eq!(empty_items.len(), 0);
        assert_eq!(app.match_count, 0);
    }

    #[test]
    fn test_tab_navigation() {
        let tab = DetailTab::Overview;
        assert_eq!(tab.next(), DetailTab::ExportedSymbols);
        assert_eq!(tab.next().next(), DetailTab::ImportedSymbols);
        assert_eq!(tab.next().next().next(), DetailTab::Diagnostics);
        assert_eq!(tab.next().next().next().next(), DetailTab::Overview);
        assert_eq!(tab.prev(), DetailTab::Diagnostics);
    }

    #[test]
    fn test_cycle_handling() {
        let mut exes = Executables::new();
        let exe_a = Executable {
            dllname: "a.dll".to_string(),
            depth_first_appearance: 0,
            found: true,
            details: Some(ExecutableDetails {
                is_api_set: false,
                is_system: false,
                is_known_dll: false,
                full_path: PathBuf::from("a.dll"),
                dependencies: Some(vec!["b.dll".to_string()]),
                symbols: None,
            }),
        };
        let exe_b = Executable {
            dllname: "b.dll".to_string(),
            depth_first_appearance: 1,
            found: true,
            details: Some(ExecutableDetails {
                is_api_set: false,
                is_system: false,
                is_known_dll: false,
                full_path: PathBuf::from("b.dll"),
                dependencies: Some(vec!["a.dll".to_string()]),
                symbols: None,
            }),
        };
        exes.insert(exe_a);
        exes.insert(exe_b);

        let mut app = App::new(&exes, true);
        // Ensure no stack overflow or infinite recursion
        let items = app.build_tree_items();
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn test_details_caching_and_large_symbols() {
        let mut exes = Executables::new();
        let mut exported = HashSet::new();
        for i in 0..2000 {
            exported.insert(format!("?symbol_{}@@YAHXZ", i));
        }

        let exe = Executable {
            dllname: "huge.dll".to_string(),
            depth_first_appearance: 0,
            found: true,
            details: Some(ExecutableDetails {
                is_api_set: false,
                is_system: false,
                is_known_dll: false,
                full_path: PathBuf::from("huge.dll"),
                dependencies: None,
                symbols: Some(ExecutableSymbols {
                    exported,
                    imported: HashMap::new(),
                }),
            }),
        };
        exes.insert(exe);

        let mut app = App::new(&exes, true);
        app.selected_tab = DetailTab::ExportedSymbols;
        app.ensure_details_updated();

        let initial_lines_len = app.cached_details_lines.len();
        assert!(initial_lines_len >= 2000);

        // Subsequent ensure_details_updated is a fast no-op
        let start = std::time::Instant::now();
        for _ in 0..100 {
            app.ensure_details_updated();
        }
        let elapsed = start.elapsed();
        assert!(elapsed.as_millis() < 50);
    }

    #[test]
    fn test_details_filtering_exports() {
        let exes = create_test_executables();
        let mut app = App::new(&exes, true);
        app.tree_state.select(vec!["main.exe".to_string(), "dep1.dll".to_string()]);
        app.selected_tab = DetailTab::ExportedSymbols;

        // No filter
        app.ensure_details_updated();
        assert_eq!(app.details_match_count, 0);

        // Filter for fn1
        app.details_search_query = "fn1".to_string();
        app.ensure_details_updated();
        assert_eq!(app.details_match_count, 1);
        let content_str: String = app.cached_details_lines.iter().flat_map(|l| l.spans.iter()).map(|s| s.content.as_ref()).collect();
        assert!(content_str.contains("fn1"));
        assert!(!content_str.contains("fn2"));

        // Non-matching filter
        app.details_search_query = "nonexistent_fn".to_string();
        app.ensure_details_updated();
        assert_eq!(app.details_match_count, 0);
        let empty_content_str: String = app.cached_details_lines.iter().flat_map(|l| l.spans.iter()).map(|s| s.content.as_ref()).collect();
        assert!(empty_content_str.contains("No exported symbols matching 'nonexistent_fn'"));
    }

    #[test]
    fn test_details_filtering_imports() {
        let exes = create_test_executables();
        let mut app = App::new(&exes, true);
        app.tree_state.select(vec!["main.exe".to_string()]);
        app.selected_tab = DetailTab::ImportedSymbols;

        // No filter
        app.ensure_details_updated();
        assert_eq!(app.details_match_count, 0);

        // Filter for fn3
        app.details_search_query = "fn3".to_string();
        app.ensure_details_updated();
        assert_eq!(app.details_match_count, 1);
        let content_str: String = app.cached_details_lines.iter().flat_map(|l| l.spans.iter()).map(|s| s.content.as_ref()).collect();
        assert!(content_str.contains("fn3"));
        assert!(content_str.contains("dep2.dll"));
        assert!(!content_str.contains("dep1.dll"));

        // Filter for dll name dep1
        app.details_search_query = "dep1".to_string();
        app.ensure_details_updated();
        assert_eq!(app.details_match_count, 2);
        let dep1_content: String = app.cached_details_lines.iter().flat_map(|l| l.spans.iter()).map(|s| s.content.as_ref()).collect();
        assert!(dep1_content.contains("dep1.dll"));
        assert!(dep1_content.contains("fn1"));
        assert!(dep1_content.contains("fn2"));
        assert!(!dep1_content.contains("fn3"));
    }

    #[test]
    fn test_dual_pane_filtering_independence() {
        let exes = create_test_executables();
        let mut app = App::new(&exes, true);

        // Filter tree on left pane
        app.search_query = "dep1".to_string();
        app.mark_tree_dirty();
        app.ensure_tree_updated();
        assert!(app.match_count >= 1);

        // Filter details on right pane
        app.tree_state.select(vec!["main.exe".to_string(), "dep1.dll".to_string()]);
        app.selected_tab = DetailTab::ExportedSymbols;
        app.details_search_query = "fn2".to_string();
        app.ensure_details_updated();
        assert_eq!(app.details_match_count, 1);

        // Verify tree filter is untouched and active
        assert_eq!(app.search_query, "dep1");
        assert_eq!(app.details_search_query, "fn2");
    }

    #[test]
    fn test_tree_jump_between_filter_results() {
        let exes = create_test_executables();
        let mut app = App::new(&exes, true);
        app.active_pane = ActivePane::Tree;

        // Filter for "dep" -> matches dep1.dll and dep2.dll
        app.search_query = "dep".to_string();
        app.mark_tree_dirty();
        app.ensure_tree_updated();

        assert_eq!(app.tree_matching_paths.len(), 2);
        assert_eq!(
            app.tree_matching_paths[0],
            vec!["main.exe".to_string(), "dep1.dll".to_string()]
        );
        assert_eq!(
            app.tree_matching_paths[1],
            vec!["main.exe".to_string(), "dep2.dll".to_string()]
        );

        // Initially at first match
        assert_eq!(app.current_match_position(), Some((1, 2)));
        assert_eq!(
            app.tree_state.selected(),
            &vec!["main.exe".to_string(), "dep1.dll".to_string()]
        );

        // Jump next -> dep2.dll
        app.jump_next_match();
        assert_eq!(app.current_match_position(), Some((2, 2)));
        assert_eq!(
            app.tree_state.selected(),
            &vec!["main.exe".to_string(), "dep2.dll".to_string()]
        );

        // Jump next -> wraps to dep1.dll
        app.jump_next_match();
        assert_eq!(app.current_match_position(), Some((1, 2)));
        assert_eq!(
            app.tree_state.selected(),
            &vec!["main.exe".to_string(), "dep1.dll".to_string()]
        );

        // Jump prev -> wraps back to dep2.dll
        app.jump_prev_match();
        assert_eq!(app.current_match_position(), Some((2, 2)));
        assert_eq!(
            app.tree_state.selected(),
            &vec!["main.exe".to_string(), "dep2.dll".to_string()]
        );

        // Jump prev -> dep1.dll
        app.jump_prev_match();
        assert_eq!(app.current_match_position(), Some((1, 2)));
        assert_eq!(
            app.tree_state.selected(),
            &vec!["main.exe".to_string(), "dep1.dll".to_string()]
        );
    }

    #[test]
    fn test_details_jump_between_filter_results() {
        let exes = create_test_executables();
        let mut app = App::new(&exes, true);
        app.active_pane = ActivePane::Details;
        app.tree_state.select(vec!["main.exe".to_string()]);
        app.selected_tab = DetailTab::ImportedSymbols;

        // Filter for "fn" -> matches fn1, fn2 (from dep1.dll), and fn3 (from dep2.dll)
        app.details_search_query = "fn".to_string();
        app.ensure_details_updated();

        assert_eq!(app.details_match_count, 3);
        assert_eq!(app.details_match_lines.len(), 3);
        assert_eq!(app.details_scroll, 0);

        // Jump next -> moves details_scroll to the first match line
        app.jump_next_match();
        let first_line = app.details_match_lines[0];
        assert_eq!(app.details_scroll, first_line);

        // Jump next -> moves to second match line
        app.jump_next_match();
        let second_line = app.details_match_lines[1];
        assert_eq!(app.details_scroll, second_line);
        assert!(second_line > first_line);

        // Jump next -> moves to third match line
        app.jump_next_match();
        let third_line = app.details_match_lines[2];
        assert_eq!(app.details_scroll, third_line);
        assert!(third_line > second_line);

        // Jump next -> wraps to first match line
        app.jump_next_match();
        assert_eq!(app.details_scroll, first_line);

        // Jump prev -> wraps to third match line
        app.jump_prev_match();
        assert_eq!(app.details_scroll, third_line);

        // Jump prev -> second match line
        app.jump_prev_match();
        assert_eq!(app.details_scroll, second_line);
    }
}
