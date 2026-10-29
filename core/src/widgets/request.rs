//! A request for a widget's module, answered: the module its address names,
//! compiled from one reading of its files, with whether it was asked for at
//! its current address — or why it can't be had.
//!
//! An address names a module by its path in the served folder and its
//! version (`?v=`). The version asked about and the bytes sent come from one
//! [`Snapshot`](super::Snapshot), so a save between the two can't be served
//! under the version before it.

use super::modules::IMPORT_EXTENSIONS;
use super::{Modules, Widgets, system_hash};
use crate::folder::Folder;

/// A widget module, compiled, as a request for it is answered.
#[derive(Debug, PartialEq)]
pub struct Compiled {
    pub js: String,
    /// Asked for at its current address: what answers it never changes.
    pub current: bool,
}

/// The system widget `name` — `Tabs.tsx` — compiled, asked for at
/// `version`; `Err` with why when it doesn't compile, and `None` for a name
/// that isn't a system widget.
pub fn system_module(
    modules: &Modules,
    name: &str,
    version: Option<&str>,
) -> Option<Result<Compiled, String>> {
    let stem = name.strip_suffix(".tsx").unwrap_or(name);
    let compiled = modules.system(stem)?;
    let current = version.is_some() && version == system_hash(stem);
    Some(
        compiled
            .map(|js| Compiled { js, current })
            .map_err(|error| format!("{name} does not compile: {error}")),
    )
}

/// The project widget or helper at `path` in `folder`'s served folder,
/// compiled, asked for at `version`. An import without an extension finds
/// the `.tsx` or `.ts` beside it. It renders by name the widgets of the
/// document whose widgets folder holds it — the entry document's when no
/// other names it. `Err` with why when it, or any helper its imports reach,
/// doesn't compile — the browser would only say an import failed; `None`
/// when no such file is in the served folder.
pub fn project_module(
    folder: &Folder,
    modules: &Modules,
    path: &str,
    version: Option<&str>,
) -> Option<Result<Compiled, String>> {
    let served = folder.served();
    let file = IMPORT_EXTENSIONS
        .iter()
        .find_map(|ext| served.file_at(&format!("{path}{ext}")))?;
    let name = served.name(&file)?;
    let owner = file
        .parent()
        .and_then(|widgets| folder.widgets_owner(widgets))
        .unwrap_or_else(|| folder.entry());
    let known = Widgets::of(&owner, modules).names().to_vec();
    let snapshot = modules.snapshot(served, &file);
    let current = version == Some(snapshot.hash(&file, &known).as_str());
    // The closure reaches only helpers inside the served folder, so each
    // has a name.
    let broken = |file: &std::path::Path| {
        let error = snapshot.compile(file, &known).err()?;
        Some(format!("{} does not compile: {error}", served.name(file)?))
    };
    let helper = snapshot
        .closure(&file)
        .into_iter()
        .skip(1)
        .find_map(|(helper, _)| broken(&helper));
    if let Some(why) = helper {
        return Some(Err(why));
    }
    Some(
        snapshot
            .compile(&file, &known)
            .map(|js| Compiled { js, current })
            .map_err(|error| format!("{name} does not compile: {error}")),
    )
}
