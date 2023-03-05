extern crate skim;

use skim::prelude::*;

use crate::common::readable_canonical_path;
use crate::executable::Executables;
use crate::pe::demangle_symbol;

struct SymbolItem {
    symbol: String,
    dllname: String,
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
        ItemPreview::Text(format!("dll: {}\nsymbol: {}", self.dllname, &demangled))
    }
}

struct ExecutableItem {
    name: String,
    path: Option<String>,
    kind: Option<String>,
    dependencies: Option<Vec<String>>,
}

impl SkimItem for ExecutableItem {
    fn text(&self) -> Cow<str> {
        Cow::Borrowed(&self.name)
    }

    fn preview(&self, _context: PreviewContext) -> ItemPreview {
        ItemPreview::Text(format!(
            "dll:\n\t{}\nkind:\n\t{},\npath:\n\t{}\nimports:\n{}",
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
                .unwrap_or("".to_owned())
        ))
    }
}

pub fn skim_symbols(exes: &Executables, selected_dlls: Option<Vec<String>>) -> Option<Vec<String>> {
    let options = SkimOptionsBuilder::default()
        // .height(Some("50%"))  // enabling this causes a bug where the console is not cleaned up upon exit
        .preview(Some("left:10:wrap")) // preview should be specified to enable preview window
        .build()
        .unwrap();

    let (tx_item, rx_item): (SkimItemSender, SkimItemReceiver) = unbounded();
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
                        }));
                    }
                }
            }
        }
    }
    drop(tx_item); // so that skim could know when to stop waiting for more items.

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
}

pub fn skim_dlls(exes: &Executables) -> Option<Vec<String>> {
    let options = SkimOptionsBuilder::default()
        // .height(Some("50%")) // enabling this causes a bug where the console is not cleaned up upon exit
        .preview(Some("")) // preview should be specified to enable preview window
        .multi(true)
        .header(Some(
            "Ctrl+g or ESC to quit\nSelect DLLs with TAB and press Enter to inspect symbols",
        ))
        .prompt(Some("Fuzzy query: >"))
        .build()
        .unwrap();

    let (tx_item, rx_item): (SkimItemSender, SkimItemReceiver) = unbounded();
    for e in exes.sorted_by_first_appearance() {
        let name = e.dllname.clone();
        let path = e
            .details
            .as_ref()
            .map(|d| readable_canonical_path(&d.full_path).ok())
            .unwrap_or(None);
        let dependencies = e
            .details
            .as_ref()
            .map(|d| d.dependencies.clone())
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
        let _ = tx_item.send(Arc::new(ExecutableItem {
            name,
            path,
            kind,
            dependencies,
        }));
    }
    drop(tx_item); // so that skim could know when to stop waiting for more items.

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
}
