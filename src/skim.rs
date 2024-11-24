extern crate crossbeam;
extern crate crossbeam_channel;
extern crate skim;

use skim::prelude::*;

use crate::common::readable_canonical_path;
use crate::executable::Executables;
use crate::pe::demangle_symbol;

enum SymbolLocation {
    Exported,
    Imported(String),
}

struct SymbolItem {
    symbol: String,
    dllname: String,
    location: SymbolLocation,
}

impl SkimItem for SymbolItem {
    fn text(&self) -> Cow<str> {
        if let Ok(demangled) = demangle_symbol(&self.symbol) {
            Cow::from(demangled)
        } else {
            Cow::Borrowed(&self.symbol)
        }
    }

    fn preview(&self, _context: PreviewContext) -> ItemPreview {
        let demangled = demangle_symbol(&self.symbol).unwrap_or(self.symbol.clone());
        let export_msg = if let SymbolLocation::Imported(dll) = &self.location {
            format!("imported from {}", dll)
        } else {
            "exported".to_string()
        };
        ItemPreview::Text(format!(
            "dll:\n\t{}\n\nlocation:\n\t{}\n\nsymbol:\n\t{}\n\nraw symbol:\n\t{}",
            self.dllname, export_msg, &demangled, self.symbol
        ))
    }
}

struct ExecutableItem {
    name: String,
    path: Option<String>,
    kind: Option<String>,
    dependencies: Option<Vec<String>>,
    imports: Option<Vec<String>>,
    exports: Option<Vec<String>>,
}

impl SkimItem for ExecutableItem {
    fn text(&self) -> Cow<str> {
        Cow::Borrowed(&self.name)
    }

    fn preview(&self, _context: PreviewContext) -> ItemPreview {
        ItemPreview::Text(format!(
            "dll:\n\t{}\n\nkind:\n\t{}\n\npath:\n\t{}\n\ndependencies:\n{}\n\nexported symbols:\n{}\n\nimported symbols:\n{}",
            self.name,
            self.kind.as_deref().unwrap_or("<not found>"),
            self.path.as_deref().unwrap_or("<not found>"),
            self.dependencies
                .as_ref()
                .map(|deps| deps
                    .iter()
                    .map(|d| format!("\t{}", d))
                    .collect::<Vec<_>>()
                    .join("\n"))
                .unwrap_or("".to_owned()),
            self.exports
                .as_ref()
                .map(|deps| deps
                    .iter()
                    .map(|d| format!("\t{}", d))
                    .collect::<Vec<_>>()
                    .join("\n"))
                .unwrap_or("".to_owned()),
            self.imports
                .as_ref()
                .map(|deps| deps
                    .iter()
                    .map(|d| format!("\t{}", d))
                    .collect::<Vec<_>>()
                    .join("\n"))
                .unwrap_or("".to_owned()),
        ))
    }
}

pub fn skim_symbols(exes: &Executables, selected_dlls: Option<Vec<String>>) -> Option<Vec<String>> {
    let options = SkimOptionsBuilder::default()
        // .height(Some("50%"))  // enabling this causes a bug where the console is not cleaned up upon exit
        .preview(Some("".to_string())) // preview should be specified to enable preview window
        .preview_window("wrap".to_string())
        .header(Some("Ctrl+g or ESC to quit".to_string()))
        .prompt("Fuzzy query: >".to_string())
        .build()
        .unwrap();

    let (tx_item, rx_item): (SkimItemSender, SkimItemReceiver) = unbounded();

    crossbeam::scope(|s| {
        // Producer thread
        s.spawn(|_| {
            for e in exes.sorted_by_first_appearance() {
                if selected_dlls
                    .as_ref()
                    .map(|sd| sd.contains(&e.dllname))
                    .unwrap_or(true)
                {
                    if let Some(d) = e.details.as_ref() {
                        if let Some(syms) = d.symbols.as_ref() {
                            for syms in &syms.exported {
                                let _ = tx_item.send(Arc::new(SymbolItem {
                                    symbol: syms.to_string(),
                                    dllname: e.dllname.clone(),
                                    location: SymbolLocation::Exported,
                                }));
                            }
                            for syms in &syms.imported {
                                for sym in syms.1 {
                                    let _ = tx_item.send(Arc::new(SymbolItem {
                                        symbol: sym.to_string(),
                                        dllname: e.dllname.clone(),
                                        location: SymbolLocation::Imported(syms.0.to_string()),
                                    }));
                                }
                            }
                        }
                    }
                }
            }

            drop(tx_item); // so that skim could know when to stop waiting for more items.
        });

        // `run_with` would read and show items from the stream
        let output = Skim::run_with(&options, Some(rx_item));

        let was_aborted = output.as_ref().map(|o| o.is_abort).unwrap_or(true);

        let selected_items = output
            .map(|out| out.selected_items)
            .unwrap_or_else(Vec::new);

        if !was_aborted {
            Some(
                selected_items
                    .iter()
                    .map(|si| si.output().to_string())
                    .collect(),
            )
        } else {
            None
        }
    })
    .unwrap()
}

pub fn skim_dlls(exes: &Executables) -> Option<Vec<String>> {
    let options = SkimOptionsBuilder::default()
        // .height(Some("50%")) // enabling this causes a bug where the console is not cleaned up upon exit
        .preview(Some("".to_string())) // preview should be specified to enable preview window
        .preview_window("wrap".to_string())
        .multi(true)
        .header(Some(
            "Ctrl+g or ESC to quit\nSelect DLLs with TAB and press Enter to inspect symbols"
            .to_string()))
        .prompt("Fuzzy query: >".to_string())
        .build()
        .unwrap();

    let (tx_item, rx_item): (SkimItemSender, SkimItemReceiver) = unbounded();

    crossbeam::scope(|s| {
        // Producer thread
        s.spawn(|_| {
            for e in exes.sorted_by_first_appearance() {
                let name = e.dllname.clone();
                let path = e
                    .details
                    .as_ref()
                    .map(|d| readable_canonical_path(&d.full_path).ok())
                    .unwrap_or(None);
                let kind = e
                    .details
                    .as_ref()
                    .map(|d| {
                        if d.is_known_dll {
                            Some("Known DLL".to_owned())
                        } else if d.is_api_set {
                            Some("API Set DLL".to_owned())
                        } else if d.is_system {
                            Some("System DLL".to_owned())
                        } else {
                            Some("User DLL".to_owned())
                        }
                    })
                    .unwrap_or(None);
                let dependencies = e
                    .details
                    .as_ref()
                    .map(|d| d.dependencies.clone())
                    .unwrap_or(None);
                let imports = e
                    .details
                    .as_ref()
                    .map(|d| {
                        d.symbols.as_ref().map(|s| {
                            s.imported
                                .iter()
                                .flat_map(|(idll, isymbols)| {
                                    isymbols
                                        .iter()
                                        .map(|ms| {
                                            format!(
                                                "{} ({})",
                                                demangle_symbol(&ms).unwrap_or(ms.to_string()),
                                                idll
                                            )
                                        })
                                        .collect::<Vec<String>>()
                                })
                                .collect::<Vec<String>>()
                        })
                    })
                    .unwrap_or(None);
                let exports = e
                    .details
                    .as_ref()
                    .map(|d| {
                        d.symbols.as_ref().map(|s| {
                            s.exported
                                .iter()
                                .map(|ms| demangle_symbol(&ms).unwrap_or(ms.to_string()))
                                .collect::<Vec<String>>()
                        })
                    })
                    .unwrap_or(None);
                let _ = tx_item.send(Arc::new(ExecutableItem {
                    name,
                    path,
                    kind,
                    dependencies,
                    imports,
                    exports,
                }));
            }
            drop(tx_item); // so that skim could know when to stop waiting for more items.
        });

        // `run_with` would read and show items from the stream
        let output = Skim::run_with(&options, Some(rx_item));

        let was_aborted = output.as_ref().map(|o| o.is_abort).unwrap_or(true);

        let selected_items = output
            .map(|out| out.selected_items)
            .unwrap_or_else(Vec::new);

        if !was_aborted {
            Some(
                selected_items
                    .iter()
                    .map(|si| si.output().to_string())
                    .collect(),
            )
        } else {
            None
        }
    })
    .unwrap()
}
